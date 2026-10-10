import { useCallback, useEffect, useState } from "react";

import * as api from "../api";
import { useToast } from "../context/ToastContext";
import { useVaultChanges } from "../context/VaultContext";
import { useDocument } from "../hooks/useDocument";
import { errorMessage, REMINDERS_FILE, type EngineStatus, type Reminder } from "../types";
import { formatDateTime, formatRelative, toLocalIsoMinute } from "../utils/format";
import { MarkdownEditor } from "./MarkdownEditor";

/**
 * `reminders.md` lives in the vault root rather than in a category folder, so
 * it is addressed through the synthetic `Root` category. The parsed list comes
 * from the Rust reminder engine; edits go through the same atomic document
 * writer as every other Markdown file.
 */
export function RemindersPane() {
  const toast = useToast();
  const [reminders, setReminders] = useState<Reminder[]>([]);
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [newText, setNewText] = useState("");
  const [newWhen, setNewWhen] = useState(() => {
    const d = new Date(Date.now() + 60 * 60 * 1000);
    d.setSeconds(0, 0);
    return toLocalIsoMinute(d);
  });
  const [showEditor, setShowEditor] = useState(false);
  const { doc, saveState, update, reload } = useDocument("Root", REMINDERS_FILE);

  const refresh = useCallback(async () => {
    try {
      const [list, engine] = await Promise.all([api.listReminders(), api.reminderEngineStatus()]);
      setReminders(list);
      setStatus(engine);
    } catch (error) {
      toast.push("error", "Could not load reminders", errorMessage(error));
    }
  }, [toast]);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 30_000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  useVaultChanges(
    (change) => change.category === null && change.name === REMINDERS_FILE,
    () => void refresh(),
  );

  async function addReminder() {
    const text = newText.trim();
    if (!text) return;
    const stamp = newWhen.trim();
    if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}/.test(stamp)) {
      toast.push("error", "Enter the time as YYYY-MM-DDTHH:MM");
      return;
    }
    const body = doc?.body ?? "";
    const line = `- [ ] ${text} @ ${stamp}`;
    const next = body.endsWith("\n") || body === "" ? `${body}${line}\n` : `${body}\n${line}\n`;
    update(next, true);
    setNewText("");
    window.setTimeout(() => void refresh(), 500);
  }

  async function complete(reminder: Reminder, checked: boolean) {
    try {
      await api.toggleTask("Root", REMINDERS_FILE, reminder.line, checked);
      await reload();
      await refresh();
    } catch (error) {
      toast.push("error", "Could not update reminder", errorMessage(error));
    }
  }

  async function sendTest() {
    try {
      await api.testNotification("Zynky", "Toast notifications are working.");
      toast.push("info", "Test notification sent");
    } catch (error) {
      toast.push("error", "Notification failed", errorMessage(error));
    }
  }

  const upcoming = reminders.filter((r) => !r.checked && !r.overdue);
  const overdue = reminders.filter((r) => !r.checked && r.overdue);
  const done = reminders.filter((r) => r.checked);

  return (
    <div className="pane pane-single">
      <section className="pane-main">
        <header className="doc-header">
          <div className="doc-header-text">
            <h2>Reminders</h2>
            <small className="muted">
              {status?.running
                ? `Engine running · last tick ${formatDateTime(status.lastTick) || "pending"} · fired ${status.firedTotal}`
                : "Engine not running"}
            </small>
          </div>
          <div className="doc-header-actions">
            <span className={`save-state save-${saveState}`}>
              {saveState === "saving" ? "Saving…" : saveState === "saved" ? "Saved" : ""}
            </span>
            <button type="button" className="btn btn-small" onClick={() => void sendTest()}>
              Test toast
            </button>
            <button
              type="button"
              className="btn btn-small"
              onClick={() => setShowEditor((s) => !s)}
            >
              {showEditor ? "Hide file" : "Edit file"}
            </button>
          </div>
        </header>

        <div className="reminder-add">
          <input
            type="text"
            placeholder="What should I remind you about?"
            value={newText}
            onChange={(e) => setNewText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void addReminder();
            }}
          />
          <input
            type="datetime-local"
            value={newWhen}
            onChange={(e) => setNewWhen(e.target.value)}
          />
          <button type="button" className="btn btn-primary" onClick={() => void addReminder()}>
            Add
          </button>
        </div>
        <p className="hint">
          Reminders are lines in <code>{REMINDERS_FILE}</code> such as{" "}
          <code>- [ ] Call the bank @ 2026-10-10T09:30</code>. The background worker checks the
          file every 60 seconds and shows a Windows toast when the minute matches.
        </p>

        {showEditor && doc ? (
          <MarkdownEditor
            value={doc.body}
            documentKey={`Root/${REMINDERS_FILE}`}
            onChange={update}
            placeholder="- [ ] Something @ 2026-01-01T09:00"
          />
        ) : (
          <div className="reminder-groups">
            <ReminderGroup title="Overdue" items={overdue} tone="overdue" onToggle={complete} />
            <ReminderGroup title="Upcoming" items={upcoming} tone="upcoming" onToggle={complete} />
            <ReminderGroup title="Completed" items={done} tone="done" onToggle={complete} />
          </div>
        )}
      </section>
    </div>
  );
}

function ReminderGroup({
  title,
  items,
  tone,
  onToggle,
}: {
  title: string;
  items: Reminder[];
  tone: "overdue" | "upcoming" | "done";
  onToggle: (reminder: Reminder, checked: boolean) => void;
}) {
  return (
    <section className={`reminder-group reminder-${tone}`}>
      <h3>
        {title} <span className="count">{items.length}</span>
      </h3>
      {items.length === 0 ? <p className="muted">None</p> : null}
      <ul>
        {items.map((reminder) => (
          <li key={`${reminder.line}-${reminder.rawTimestamp}`} className="reminder-row">
            <label>
              <input
                type="checkbox"
                checked={reminder.checked}
                onChange={(e) => onToggle(reminder, e.target.checked)}
              />
              <span className="reminder-text">{reminder.text || "(untitled)"}</span>
            </label>
            <span className="reminder-when" title={reminder.rawTimestamp}>
              {formatDateTime(reminder.due)} · {formatRelative(reminder.due)}
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}
