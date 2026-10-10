//! Background reminder engine.
//!
//! A dedicated native thread wakes up on every wall-clock minute boundary,
//! reads `reminders.md` from the active vault, extracts ISO 8601 timestamps
//! attached to unchecked task items and fires a native Windows toast
//! notification when a timestamp matches the current local minute.
//!
//! `reminders.md` example:
//!
//! ```markdown
//! # Reminders
//! - [ ] Stand-up meeting @ 2026-10-10T09:30
//! - [ ] Renew domain 2026-11-01T08:00:00+02:00
//! - [x] Already done 2026-01-01T10:00
//! ```

use std::{
    collections::HashSet,
    fs,
    path::Path,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use chrono::{DateTime, Datelike, FixedOffset, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    error::{AppError, AppResult},
    frontmatter,
    state::AppState,
    tasks,
    vault::REMINDERS_FILE,
};

/// Event emitted to the webview whenever a reminder fires.
pub const FIRED_EVENT: &str = "reminder://fired";
/// Wake-up period of the worker loop.
pub const TICK: Duration = Duration::from_secs(60);
/// Reminders older than this are never fired retroactively after a long sleep.
const CATCH_UP_LIMIT_MINUTES: i64 = 24 * 60;

/// A task item carrying a due timestamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    /// Zero-based line index in the body of `reminders.md`.
    pub line: usize,
    /// Task text with the timestamp removed.
    pub text: String,
    pub checked: bool,
    /// Due time in the machine's local timezone (RFC 3339).
    pub due: DateTime<Local>,
    /// The timestamp exactly as written in the file.
    pub raw_timestamp: String,
    /// `true` when `due` is in the past relative to the time of parsing.
    pub overdue: bool,
}

/// Runtime status of the worker, exposed to the UI.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub running: bool,
    pub last_tick: Option<DateTime<Local>>,
    pub next_tick: Option<DateTime<Local>>,
    pub fired_total: u64,
}

#[derive(Default)]
pub struct ReminderEngine {
    status: Mutex<EngineStatus>,
}

impl ReminderEngine {
    pub fn status(&self) -> EngineStatus {
        self.status
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    fn update<F: FnOnce(&mut EngineStatus)>(&self, f: F) {
        let mut guard = self.status.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut guard);
    }
}

// ---------------------------------------------------------------------------
// ISO 8601 parsing
// ---------------------------------------------------------------------------

/// Scan `text` for the first ISO 8601 timestamp and return it together with
/// its byte range. Accepted shapes:
///
/// * `2026-10-10T09:30`           (local time)
/// * `2026-10-10 09:30:15`        (space separator, seconds)
/// * `2026-10-10T09:30:00.123`    (fractional seconds)
/// * `2026-10-10T09:30Z`          (UTC)
/// * `2026-10-10T09:30:00+02:00`  (explicit offset)
pub fn find_iso_timestamp(text: &str) -> Option<(DateTime<Local>, std::ops::Range<usize>)> {
    let bytes = text.as_bytes();
    let digit = |i: usize| bytes.get(i).map(|b| b.is_ascii_digit()).unwrap_or(false);
    let is = |i: usize, c: u8| bytes.get(i) == Some(&c);

    let mut start = 0;
    while start + 16 <= bytes.len() {
        // YYYY-MM-DD
        let date_ok = (0..4).all(|k| digit(start + k))
            && is(start + 4, b'-')
            && digit(start + 5)
            && digit(start + 6)
            && is(start + 7, b'-')
            && digit(start + 8)
            && digit(start + 9)
            && (is(start + 10, b'T') || is(start + 10, b't') || is(start + 10, b' '))
            && digit(start + 11)
            && digit(start + 12)
            && is(start + 13, b':')
            && digit(start + 14)
            && digit(start + 15);
        if !date_ok {
            start += 1;
            continue;
        }
        // Avoid matching inside a longer digit run (e.g. "12026-...").
        if start > 0 && digit(start - 1) {
            start += 1;
            continue;
        }

        let mut end = start + 16;
        let mut seconds = 0u32;
        let mut nanos = 0u32;
        if is(end, b':') && digit(end + 1) && digit(end + 2) {
            seconds = (bytes[end + 1] - b'0') as u32 * 10 + (bytes[end + 2] - b'0') as u32;
            end += 3;
            if is(end, b'.') && digit(end + 1) {
                let frac_start = end + 1;
                let mut frac_end = frac_start;
                while digit(frac_end) {
                    frac_end += 1;
                }
                let frac = &text[frac_start..frac_end];
                let mut value: u64 = frac.chars().take(9).fold(0, |acc, c| acc * 10 + (c as u64 - '0' as u64));
                for _ in frac.len().min(9)..9 {
                    value *= 10;
                }
                nanos = value as u32;
                end = frac_end;
            }
        }

        // Optional zone designator.
        let mut offset: Option<FixedOffset> = None;
        if is(end, b'Z') || is(end, b'z') {
            offset = FixedOffset::east_opt(0);
            end += 1;
        } else if (is(end, b'+') || is(end, b'-'))
            && digit(end + 1)
            && digit(end + 2)
        {
            let sign: i32 = if is(end, b'+') { 1 } else { -1 };
            let hours = (bytes[end + 1] - b'0') as i32 * 10 + (bytes[end + 2] - b'0') as i32;
            let mut consumed = 3;
            let mut minutes = 0;
            if is(end + 3, b':') && digit(end + 4) && digit(end + 5) {
                minutes = (bytes[end + 4] - b'0') as i32 * 10 + (bytes[end + 5] - b'0') as i32;
                consumed = 6;
            } else if digit(end + 3) && digit(end + 4) {
                minutes = (bytes[end + 3] - b'0') as i32 * 10 + (bytes[end + 4] - b'0') as i32;
                consumed = 5;
            }
            if let Some(fixed) = FixedOffset::east_opt(sign * (hours * 3600 + minutes * 60)) {
                offset = Some(fixed);
                end += consumed;
            }
        }

        let year: i32 = text[start..start + 4].parse().ok()?;
        let month: u32 = text[start + 5..start + 7].parse().ok()?;
        let day: u32 = text[start + 8..start + 10].parse().ok()?;
        let hour: u32 = text[start + 11..start + 13].parse().ok()?;
        let minute: u32 = text[start + 14..start + 16].parse().ok()?;

        let Some(date) = NaiveDate::from_ymd_opt(year, month, day) else {
            start += 1;
            continue;
        };
        let Some(time) = NaiveTime::from_hms_nano_opt(hour, minute, seconds, nanos) else {
            start += 1;
            continue;
        };
        let naive = NaiveDateTime::new(date, time);

        let local = match offset {
            Some(fixed) => match fixed.from_local_datetime(&naive).single() {
                Some(dt) => dt.with_timezone(&Local),
                None => {
                    start += 1;
                    continue;
                }
            },
            None => match Local.from_local_datetime(&naive) {
                chrono::LocalResult::Single(dt) => dt,
                chrono::LocalResult::Ambiguous(earliest, _) => earliest,
                chrono::LocalResult::None => {
                    start += 1;
                    continue;
                }
            },
        };
        return Some((local, start..end));
    }
    None
}

/// Remove the timestamp (and a dangling `@` / `at` / `due` marker) from text.
fn strip_timestamp(text: &str, range: &std::ops::Range<usize>) -> String {
    let before = text[..range.start].trim_end();
    let after = text[range.end..].trim_start();
    let before = before
        .trim_end_matches('@')
        .trim_end()
        .trim_end_matches("due:")
        .trim_end_matches("due")
        .trim_end_matches(" at")
        .trim_end_matches(['(', '[', '<']);
    let after = after.trim_start_matches([')', ']', '>']);
    let joined = format!("{} {}", before.trim(), after.trim());
    joined.trim().to_string()
}

/// Extract every timestamped task from a Markdown body.
pub fn parse_reminders(body: &str, now: DateTime<Local>) -> Vec<Reminder> {
    tasks::parse_tasks(body)
        .into_iter()
        .filter_map(|task| {
            let (due, range) = find_iso_timestamp(&task.text)?;
            Some(Reminder {
                line: task.line,
                raw_timestamp: task.text[range.clone()].to_string(),
                text: strip_timestamp(&task.text, &range),
                checked: task.checked,
                due,
                overdue: due < now,
            })
        })
        .collect()
}

fn truncate_to_minute(dt: DateTime<Local>) -> DateTime<Local> {
    dt.with_second(0)
        .and_then(|d| d.with_nanosecond(0))
        .unwrap_or(dt)
}

/// Reminders that fall inside `(previous_tick, now]`, truncated to minutes,
/// and that are not completed. A `previous_tick` of `None` means only the
/// current minute is considered.
pub fn due_reminders(
    reminders: &[Reminder],
    previous_tick: Option<DateTime<Local>>,
    now: DateTime<Local>,
) -> Vec<Reminder> {
    let now_min = truncate_to_minute(now);
    let lower = previous_tick
        .map(truncate_to_minute)
        .unwrap_or(now_min - chrono::Duration::minutes(1));
    let lower = lower.max(now_min - chrono::Duration::minutes(CATCH_UP_LIMIT_MINUTES));
    reminders
        .iter()
        .filter(|r| !r.checked)
        .filter(|r| {
            let due = truncate_to_minute(r.due);
            due > lower && due <= now_min
        })
        .cloned()
        .collect()
}

fn read_reminders_file(root: &Path) -> AppResult<String> {
    let path = root.join(REMINDERS_FILE);
    match fs::read_to_string(&path) {
        Ok(raw) => Ok(frontmatter::strip(&raw)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(AppError::io(&path, e)),
    }
}

// ---------------------------------------------------------------------------
// Native notifications
// ---------------------------------------------------------------------------

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Build the PowerShell script that shows a Windows 10/11 toast through the
/// WinRT `ToastNotificationManager` API.
pub fn build_toast_script(title: &str, body: &str) -> String {
    // The PowerShell AUMID is registered on every Windows install, which lets
    // the toast appear even when the app has not been installed via MSI/NSIS.
    const APP_ID: &str = r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe";
    let title = xml_escape(title);
    let body = xml_escape(body);
    format!(
        r#"$ErrorActionPreference = 'Stop'
[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
[Windows.UI.Notifications.ToastNotification, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
[Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] | Out-Null
$template = @'
<toast scenario="reminder" duration="long">
  <visual>
    <binding template="ToastGeneric">
      <text>{title}</text>
      <text>{body}</text>
      <text placement="attribution">Zynky</text>
    </binding>
  </visual>
  <audio src="ms-winsoundevent:Notification.Reminder" />
</toast>
'@
$xml = New-Object Windows.Data.Xml.Dom.XmlDocument
$xml.LoadXml($template)
$toast = New-Object Windows.UI.Notifications.ToastNotification $xml
$toast.Tag = 'zynky'
$toast.Group = 'reminders'
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{APP_ID}').Show($toast)
"#
    )
}

/// Invoke the native Windows toast notification utility (PowerShell + WinRT).
#[cfg(windows)]
fn show_native_toast(title: &str, body: &str) -> AppResult<()> {
    use base64::Engine as _;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let script = build_toast_script(title, body);
    let utf16: Vec<u8> = script
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);

    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-NoLogo",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-EncodedCommand",
            &encoded,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| AppError::Other(format!("failed to launch powershell.exe: {e}")))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(AppError::Other(format!(
            "toast command failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

#[cfg(not(windows))]
fn show_native_toast(_title: &str, _body: &str) -> AppResult<()> {
    Err(AppError::Other(
        "native toast command is only available on Windows".into(),
    ))
}

/// Show a notification: native toast first, Tauri notification plugin as the
/// cross-platform fallback.
pub fn notify(app: &AppHandle, title: &str, body: &str) -> AppResult<()> {
    match show_native_toast(title, body) {
        Ok(()) => Ok(()),
        Err(native_err) => {
            log::debug!("native toast unavailable ({native_err}); using notification plugin");
            use tauri_plugin_notification::NotificationExt;
            app.notification()
                .builder()
                .title(title)
                .body(body)
                .show()
                .map_err(|e| AppError::Other(format!("notification failed: {e}")))
        }
    }
}

// ---------------------------------------------------------------------------
// Worker loop
// ---------------------------------------------------------------------------

/// Duration until the next wall-clock minute boundary.
fn until_next_minute(now: DateTime<Local>) -> Duration {
    let next = truncate_to_minute(now) + chrono::Duration::minutes(1);
    (next - now).to_std().unwrap_or(TICK)
}

/// Spawn the background worker. Called once from Tauri's `setup` hook.
pub fn spawn(app: AppHandle) {
    let engine: Arc<ReminderEngine> = Arc::new(ReminderEngine::default());
    app.manage(engine.clone());

    let result = thread::Builder::new()
        .name("zynky-reminder-engine".into())
        .spawn(move || worker_loop(app, engine));
    if let Err(e) = result {
        log::error!("could not start reminder engine: {e}");
    }
}

fn worker_loop(app: AppHandle, engine: Arc<ReminderEngine>) {
    let mut fired: HashSet<String> = HashSet::new();
    let mut previous_tick: Option<DateTime<Local>> = None;
    engine.update(|s| {
        s.running = true;
        s.next_tick = Some(truncate_to_minute(Local::now()) + chrono::Duration::minutes(1));
    });

    loop {
        // Align every wake-up to the start of a minute so a reminder written
        // as 09:30 fires at 09:30:00, not somewhere inside that minute.
        thread::sleep(until_next_minute(Local::now()));
        let now = Local::now();
        let now_minute = truncate_to_minute(now);
        engine.update(|s| {
            s.last_tick = Some(now);
            s.next_tick = Some(now_minute + chrono::Duration::minutes(1));
        });

        let state = app.state::<AppState>();
        let Some(root) = state.vault_root() else {
            previous_tick = Some(now);
            continue;
        };

        let body = match read_reminders_file(&root) {
            Ok(body) => body,
            Err(e) => {
                log::warn!("reminder engine: {e}");
                previous_tick = Some(now);
                continue;
            }
        };

        let reminders = parse_reminders(&body, now);
        let due = due_reminders(&reminders, previous_tick, now);
        previous_tick = Some(now);

        // Keep the dedupe set small: drop keys from earlier minutes.
        fired.retain(|key| key.starts_with(&now_minute.format("%Y-%m-%dT%H:%M").to_string()));

        for reminder in due {
            let key = format!(
                "{}|{}|{}",
                truncate_to_minute(reminder.due).format("%Y-%m-%dT%H:%M"),
                reminder.line,
                reminder.text
            );
            if !fired.insert(key) {
                continue;
            }
            let title = "Zynky reminder";
            let body = if reminder.text.is_empty() {
                format!("Due {}", reminder.due.format("%H:%M"))
            } else {
                format!("{} · {}", reminder.text, reminder.due.format("%Y-%m-%d %H:%M"))
            };
            match notify(&app, title, &body) {
                Ok(()) => {
                    engine.update(|s| s.fired_total += 1);
                    log::info!("reminder fired: {}", reminder.text);
                }
                Err(e) => log::error!("failed to show reminder '{}': {e}", reminder.text),
            }
            if let Err(e) = app.emit(FIRED_EVENT, &reminder) {
                log::warn!("failed to emit reminder event: {e}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Parse `reminders.md` of the open vault.
#[tauri::command]
pub fn list_reminders(state: State<'_, AppState>) -> AppResult<Vec<Reminder>> {
    let root = state.require_vault()?;
    let body = read_reminders_file(&root)?;
    let mut reminders = parse_reminders(&body, Local::now());
    reminders.sort_by_key(|r| r.due);
    Ok(reminders)
}

/// Fire a notification immediately so the user can verify toasts work.
#[tauri::command]
pub fn test_notification(app: AppHandle, title: Option<String>, body: Option<String>) -> AppResult<()> {
    notify(
        &app,
        title.as_deref().unwrap_or("Zynky"),
        body.as_deref().unwrap_or("Notifications are working."),
    )
}

/// Current worker status.
#[tauri::command]
pub fn reminder_engine_status(engine: State<'_, Arc<ReminderEngine>>) -> AppResult<EngineStatus> {
    Ok(engine.status())
}

/// Produce a canonical ISO 8601 string (local time, minute precision) for the
/// frontend to insert into `reminders.md`.
#[tauri::command]
pub fn format_reminder_timestamp(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> AppResult<String> {
    let date = NaiveDate::from_ymd_opt(year, month, day)
        .ok_or_else(|| AppError::Other("invalid date".into()))?;
    let time = NaiveTime::from_hms_opt(hour, minute, 0)
        .ok_or_else(|| AppError::Other("invalid time".into()))?;
    let naive = NaiveDateTime::new(date, time);
    Ok(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}",
        naive.year(),
        naive.month(),
        naive.day(),
        naive.hour(),
        naive.minute()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_local_timestamp() {
        let (dt, range) = find_iso_timestamp("Call mum @ 2026-10-10T09:30 please").unwrap();
        assert_eq!(&"Call mum @ 2026-10-10T09:30 please"[range], "2026-10-10T09:30");
        assert_eq!(dt.hour(), 9);
        assert_eq!(dt.minute(), 30);
        assert_eq!(dt.day(), 10);
    }

    #[test]
    fn finds_timestamp_with_seconds_and_offset() {
        let text = "Deploy 2026-01-05T23:59:30+02:00";
        let (dt, range) = find_iso_timestamp(text).unwrap();
        assert_eq!(&text[range], "2026-01-05T23:59:30+02:00");
        let utc = dt.with_timezone(&chrono::Utc);
        assert_eq!(utc.hour(), 21);
        assert_eq!(utc.minute(), 59);
    }

    #[test]
    fn finds_utc_timestamp() {
        let (dt, _) = find_iso_timestamp("x 2026-03-01 12:00Z").unwrap();
        assert_eq!(dt.with_timezone(&chrono::Utc).hour(), 12);
    }

    #[test]
    fn ignores_invalid_dates() {
        assert!(find_iso_timestamp("2026-13-40T99:99").is_none());
        assert!(find_iso_timestamp("no date here").is_none());
        assert!(find_iso_timestamp("2026-10-10").is_none());
    }

    #[test]
    fn parses_reminders_and_strips_text() {
        let body = "- [ ] Stand-up @ 2026-10-10T09:30\n- [x] Done (2026-10-10T08:00)\n- [ ] No date\n";
        let now = Local.with_ymd_and_hms(2026, 10, 10, 9, 0, 0).unwrap();
        let rs = parse_reminders(body, now);
        assert_eq!(rs.len(), 2);
        assert_eq!(rs[0].text, "Stand-up");
        assert!(!rs[0].checked);
        assert!(!rs[0].overdue);
        assert_eq!(rs[1].text, "Done");
        assert!(rs[1].checked);
        assert!(rs[1].overdue);
    }

    #[test]
    fn due_matches_current_minute_only() {
        let body = "- [ ] A 2026-10-10T09:30\n- [ ] B 2026-10-10T09:31\n- [x] C 2026-10-10T09:30\n";
        let now = Local.with_ymd_and_hms(2026, 10, 10, 9, 30, 42).unwrap();
        let rs = parse_reminders(body, now);
        let due = due_reminders(&rs, None, now);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].text, "A");
    }

    #[test]
    fn due_catches_up_after_gap() {
        let body = "- [ ] A 2026-10-10T09:30\n- [ ] B 2026-10-10T09:31\n- [ ] C 2026-10-10T09:33\n";
        let now = Local.with_ymd_and_hms(2026, 10, 10, 9, 32, 5).unwrap();
        let prev = Local.with_ymd_and_hms(2026, 10, 10, 9, 29, 59).unwrap();
        let rs = parse_reminders(body, now);
        let due = due_reminders(&rs, Some(prev), now);
        let names: Vec<_> = due.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(names, ["A", "B"]);
    }

    #[test]
    fn toast_script_escapes_xml() {
        let script = build_toast_script("Tom & Jerry", "<b>bold</b>");
        assert!(script.contains("Tom &amp; Jerry"));
        assert!(script.contains("&lt;b&gt;bold&lt;/b&gt;"));
        assert!(script.contains("CreateToastNotifier"));
    }

    #[test]
    fn next_minute_delay_is_bounded() {
        let d = until_next_minute(Local::now());
        assert!(d <= TICK);
    }
}
