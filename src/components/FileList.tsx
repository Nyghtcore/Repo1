import { useState, type ReactNode } from "react";

import type { FileEntry } from "../types";
import { formatRelative } from "../utils/format";

export interface FileListProps {
  title: string;
  entries: FileEntry[];
  selected: string | null;
  loading?: boolean;
  onSelect: (entry: FileEntry) => void;
  onCreate?: (title: string) => void;
  createLabel?: string;
  renderMeta?: (entry: FileEntry) => ReactNode;
  emptyHint?: string;
}

/** Sidebar-style list of files inside one vault category. */
export function FileList({
  title,
  entries,
  selected,
  loading = false,
  onSelect,
  onCreate,
  createLabel = "New",
  renderMeta,
  emptyHint = "Nothing here yet.",
}: FileListProps) {
  const [filter, setFilter] = useState("");
  const [creating, setCreating] = useState(false);
  const [newTitle, setNewTitle] = useState("");

  const visible = entries.filter((entry) => {
    const q = filter.trim().toLowerCase();
    if (!q) return true;
    return (
      entry.title.toLowerCase().includes(q) ||
      entry.name.toLowerCase().includes(q) ||
      entry.tags.some((t) => t.toLowerCase().includes(q))
    );
  });

  function submitCreate() {
    const value = newTitle.trim();
    if (!value || !onCreate) return;
    onCreate(value);
    setNewTitle("");
    setCreating(false);
  }

  return (
    <aside className="file-list">
      <header className="file-list-header">
        <h2>{title}</h2>
        {onCreate ? (
          <button type="button" className="btn btn-small" onClick={() => setCreating((c) => !c)}>
            + {createLabel}
          </button>
        ) : null}
      </header>
      {creating && onCreate ? (
        <div className="file-list-create">
          <input
            autoFocus
            type="text"
            placeholder="Title"
            value={newTitle}
            onChange={(e) => setNewTitle(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") submitCreate();
              if (e.key === "Escape") setCreating(false);
            }}
          />
          <button type="button" className="btn btn-small btn-primary" onClick={submitCreate}>
            Create
          </button>
        </div>
      ) : null}
      <input
        type="search"
        className="file-list-filter"
        placeholder="Filter…"
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
      />
      <ul className="file-list-items">
        {loading && entries.length === 0 ? <li className="muted">Loading…</li> : null}
        {!loading && visible.length === 0 ? <li className="muted">{emptyHint}</li> : null}
        {visible.map((entry) => (
          <li key={entry.name}>
            <button
              type="button"
              className={`file-item${selected === entry.name ? " active" : ""}`}
              onClick={() => onSelect(entry)}
            >
              <span className="file-item-title">{entry.title}</span>
              <span className="file-item-meta">
                {renderMeta ? renderMeta(entry) : formatRelative(entry.modified)}
              </span>
              {entry.tags.length > 0 ? (
                <span className="file-item-tags">
                  {entry.tags.map((tag) => (
                    <span key={tag} className="tag">
                      {tag}
                    </span>
                  ))}
                </span>
              ) : null}
            </button>
          </li>
        ))}
      </ul>
    </aside>
  );
}
