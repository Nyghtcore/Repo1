//! Markdown checkbox (`- [ ]` / `- [x]`) parsing and in-place rewriting.
//!
//! Tasks live inside ordinary Markdown documents. The parser works on the body
//! text (frontmatter already stripped) and records the exact line number of
//! every checkbox so the frontend can toggle a task and have the physical
//! file updated without disturbing the rest of the document.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TaskError {
    #[error("line {0} does not contain a markdown checkbox")]
    NotACheckbox(usize),
    #[error("line {0} is out of range (document has {1} lines)")]
    OutOfRange(usize, usize),
}

/// A single checkbox item extracted from a Markdown document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskItem {
    /// Zero-based line index within the Markdown *body*.
    pub line: usize,
    /// Nesting depth derived from leading whitespace (2 spaces or 1 tab = 1).
    pub indent: usize,
    /// Whether the checkbox is ticked (`[x]` / `[X]`).
    pub checked: bool,
    /// Text after the checkbox, trimmed.
    pub text: String,
}

/// Result of locating a checkbox on a line.
struct CheckboxSpan {
    /// Byte offset of the character inside the brackets.
    mark_index: usize,
    checked: bool,
    indent: usize,
    text_start: usize,
}

/// Try to interpret a line as a Markdown task list item.
///
/// Accepted forms (leading whitespace allowed):
/// * `- [ ] text`, `* [ ] text`, `+ [ ] text`
/// * `1. [ ] text`, `1) [ ] text`
/// * `[x]` or `[X]` for completed items.
fn locate_checkbox(line: &str) -> Option<CheckboxSpan> {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut indent_cols = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b' ' => indent_cols += 1,
            b'\t' => indent_cols += 2,
            _ => break,
        }
        i += 1;
    }

    // List marker: "-", "*", "+" or an ordered marker like "12." / "3)".
    let marker_start = i;
    match bytes.get(i) {
        Some(b'-') | Some(b'*') | Some(b'+') => i += 1,
        Some(c) if c.is_ascii_digit() => {
            while matches!(bytes.get(i), Some(c) if c.is_ascii_digit()) {
                i += 1;
            }
            match bytes.get(i) {
                Some(b'.') | Some(b')') => i += 1,
                _ => return None,
            }
        }
        _ => return None,
    }
    if i == marker_start {
        return None;
    }
    // Exactly one or more spaces/tabs between marker and bracket.
    if !matches!(bytes.get(i), Some(b' ') | Some(b'\t')) {
        return None;
    }
    while matches!(bytes.get(i), Some(b' ') | Some(b'\t')) {
        i += 1;
    }
    if bytes.get(i) != Some(&b'[') {
        return None;
    }
    let mark_index = i + 1;
    let checked = match bytes.get(mark_index) {
        Some(b' ') => false,
        Some(b'x') | Some(b'X') => true,
        _ => return None,
    };
    if bytes.get(mark_index + 1) != Some(&b']') {
        return None;
    }
    let after = mark_index + 2;
    // A checkbox must be followed by whitespace or end of line.
    match bytes.get(after) {
        None | Some(b' ') | Some(b'\t') => {}
        _ => return None,
    }
    let mut text_start = after;
    while matches!(bytes.get(text_start), Some(b' ') | Some(b'\t')) {
        text_start += 1;
    }
    Some(CheckboxSpan {
        mark_index,
        checked,
        indent: indent_cols / 2,
        text_start,
    })
}

/// Extract every checkbox item from a Markdown body.
pub fn parse_tasks(body: &str) -> Vec<TaskItem> {
    body.lines()
        .enumerate()
        .filter_map(|(line_no, line)| {
            locate_checkbox(line).map(|span| TaskItem {
                line: line_no,
                indent: span.indent,
                checked: span.checked,
                text: line[span.text_start..].trim_end().to_string(),
            })
        })
        .collect()
}

/// Rewrite the checkbox on `line` to the requested state and return the new
/// body. Everything else in the document is preserved byte-for-byte,
/// including the original line endings.
pub fn set_task_state(body: &str, line: usize, checked: bool) -> Result<String, TaskError> {
    let total = body.split_inclusive('\n').count();
    if line >= total {
        return Err(TaskError::OutOfRange(line, total));
    }
    let mut out = String::with_capacity(body.len());
    for (idx, raw_line) in body.split_inclusive('\n').enumerate() {
        if idx != line {
            out.push_str(raw_line);
            continue;
        }
        let content = raw_line.trim_end_matches(['\r', '\n']);
        let ending = &raw_line[content.len()..];
        let span = locate_checkbox(content).ok_or(TaskError::NotACheckbox(line))?;
        out.push_str(&content[..span.mark_index]);
        out.push(if checked { 'x' } else { ' ' });
        out.push_str(&content[span.mark_index + 1..]);
        out.push_str(ending);
    }
    Ok(out)
}

/// Summary counts for a document's tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TaskSummary {
    pub total: usize,
    pub completed: usize,
}

pub fn summarize(body: &str) -> TaskSummary {
    let items = parse_tasks(body);
    TaskSummary {
        total: items.len(),
        completed: items.iter().filter(|t| t.checked).count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "# Groceries\n\n- [ ] Milk\n- [x] Eggs\n  - [ ] Free range\n* [X] Bread\n1. [ ] Numbered\n- not a task\n-[ ] missing space\n- [y] bad mark\n";

    #[test]
    fn parses_all_forms() {
        let tasks = parse_tasks(DOC);
        let texts: Vec<_> = tasks.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, ["Milk", "Eggs", "Free range", "Bread", "Numbered"]);
        assert_eq!(tasks[0].line, 2);
        assert!(!tasks[0].checked);
        assert!(tasks[1].checked);
        assert_eq!(tasks[2].indent, 1);
        assert!(tasks[3].checked);
    }

    #[test]
    fn toggles_in_place() {
        let updated = set_task_state(DOC, 2, true).unwrap();
        assert!(updated.contains("- [x] Milk\n"));
        assert!(updated.contains("- [x] Eggs\n"));
        let back = set_task_state(&updated, 3, false).unwrap();
        assert!(back.contains("- [ ] Eggs\n"));
        assert_eq!(back.len(), DOC.len());
    }

    #[test]
    fn preserves_crlf() {
        let body = "- [ ] a\r\n- [ ] b\r\n";
        let out = set_task_state(body, 1, true).unwrap();
        assert_eq!(out, "- [ ] a\r\n- [x] b\r\n");
    }

    #[test]
    fn errors_on_non_task_lines() {
        assert!(matches!(
            set_task_state(DOC, 0, true).unwrap_err(),
            TaskError::NotACheckbox(0)
        ));
        assert!(matches!(
            set_task_state(DOC, 999, true).unwrap_err(),
            TaskError::OutOfRange(999, _)
        ));
    }

    #[test]
    fn summary_counts() {
        let s = summarize(DOC);
        assert_eq!(s.total, 5);
        assert_eq!(s.completed, 2);
    }
}
