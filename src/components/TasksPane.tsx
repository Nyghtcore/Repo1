import { useState } from "react";

import * as api from "../api";
import { useToast } from "../context/ToastContext";
import { useDocument } from "../hooks/useDocument";
import { useFileList } from "../hooks/useFileList";
import { errorMessage, type Category, type FileEntry, type TaskItem } from "../types";
import { formatDateTime } from "../utils/format";
import { DocumentHeader } from "./DocumentHeader";
import { FileList } from "./FileList";
import { MarkdownEditor } from "./MarkdownEditor";

const CATEGORY: Category = "Tasks";

type ViewMode = "checklist" | "editor";

export function TasksPane() {
  const toast = useToast();
  const { entries, loading, refresh } = useFileList(CATEGORY);
  const [selected, setSelected] = useState<string | null>(null);
  const [view, setView] = useState<ViewMode>("checklist");
  const [hideDone, setHideDone] = useState(false);
  const { doc, saveState, update, toggleTask } = useDocument(CATEGORY, selected);

  async function create(title: string) {
    try {
      const entry = await api.createDocument(CATEGORY, title);
      await refresh();
      setSelected(entry.name);
    } catch (error) {
      toast.push("error", "Could not create task list", errorMessage(error));
    }
  }

  async function remove() {
    if (!selected) return;
    if (!window.confirm(`Delete "${selected}" permanently?`)) return;
    try {
      await api.deleteFile(CATEGORY, selected);
      setSelected(null);
      await refresh();
    } catch (error) {
      toast.push("error", "Could not delete task list", errorMessage(error));
    }
  }

  async function rename(newName: string) {
    if (!selected) return;
    try {
      const entry = await api.renameFile(CATEGORY, selected, newName);
      await refresh();
      setSelected(entry.name);
    } catch (error) {
      toast.push("error", "Could not rename task list", errorMessage(error));
    }
  }

  function renderMeta(entry: FileEntry) {
    if (!entry.tasks || entry.tasks.total === 0) return "No tasks";
    return `${entry.tasks.completed}/${entry.tasks.total} done`;
  }

  const visibleTasks: TaskItem[] = doc
    ? doc.tasks.filter((task) => !hideDone || !task.checked)
    : [];
  const completed = doc ? doc.tasks.filter((t) => t.checked).length : 0;
  const total = doc ? doc.tasks.length : 0;
  const progress = total === 0 ? 0 : Math.round((completed / total) * 100);

  return (
    <div className={`pane${selected && doc ? " pane-detail-open" : ""}`}>
      <FileList
        title="Tasks"
        entries={entries}
        selected={selected}
        loading={loading}
        onSelect={(entry) => setSelected(entry.name)}
        onCreate={create}
        createLabel="List"
        renderMeta={renderMeta}
        emptyHint="Create a task list to get started."
      />
      <section className="pane-main">
        {selected && doc ? (
          <>
            <DocumentHeader
              title={String(doc.frontmatter.title ?? selected)}
              subtitle={`${doc.name} · ${formatDateTime(doc.modified)}`}
              saveState={saveState}
              path={doc.path}
              onRename={rename}
              onDelete={remove}
              onBack={() => setSelected(null)}
              extra={
                <div className="segmented segmented-small">
                  <button
                    type="button"
                    className={view === "checklist" ? "active" : ""}
                    onClick={() => setView("checklist")}
                  >
                    Checklist
                  </button>
                  <button
                    type="button"
                    className={view === "editor" ? "active" : ""}
                    onClick={() => setView("editor")}
                  >
                    Editor
                  </button>
                </div>
              }
            />

            <div className="task-progress">
              <div className="task-progress-bar">
                <div className="task-progress-fill" style={{ width: `${progress}%` }} />
              </div>
              <span className="muted">
                {completed} of {total} complete
              </span>
              <label className="checkbox-inline">
                <input
                  type="checkbox"
                  checked={hideDone}
                  onChange={(e) => setHideDone(e.target.checked)}
                />
                Hide completed
              </label>
            </div>

            {view === "checklist" ? (
              <ul className="task-checklist">
                {visibleTasks.length === 0 ? (
                  <li className="muted">
                    {total === 0
                      ? "No checkboxes in this file yet. Switch to the editor and add `- [ ] something`."
                      : "All done!"}
                  </li>
                ) : null}
                {visibleTasks.map((task) => (
                  <li
                    key={`${task.line}-${task.text}`}
                    className={`task-row${task.checked ? " done" : ""}`}
                    style={{ paddingLeft: `${task.indent * 1.5}rem` }}
                  >
                    <label>
                      <input
                        type="checkbox"
                        checked={task.checked}
                        onChange={(e) => void toggleTask(task.line, e.target.checked)}
                      />
                      <span>{task.text || <em className="muted">(empty)</em>}</span>
                    </label>
                    <small className="muted">line {task.line + 1}</small>
                  </li>
                ))}
              </ul>
            ) : (
              <MarkdownEditor
                value={doc.body}
                documentKey={`${CATEGORY}/${doc.name}`}
                onChange={update}
                placeholder="- [ ] Add a task"
              />
            )}
          </>
        ) : (
          <div className="empty-state">
            <h3>Tasks</h3>
            <p>
              Task lists are Markdown files using <code>- [ ]</code> and <code>- [x]</code>{" "}
              checkboxes. Ticking a box rewrites the file on disk immediately.
            </p>
          </div>
        )}
      </section>
    </div>
  );
}
