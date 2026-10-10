import { useState } from "react";

import * as api from "../api";
import { isMobile } from "../platform";
import type { SaveState } from "../hooks/useDocument";

export interface DocumentHeaderProps {
  title: string;
  subtitle?: string;
  saveState?: SaveState;
  path?: string;
  onRename?: (newName: string) => void;
  onDelete?: () => void;
  /** Phone layout: return from the detail view to the list. */
  onBack?: () => void;
  extra?: React.ReactNode;
}

const SAVE_LABEL: Record<SaveState, string> = {
  idle: "",
  dirty: "Unsaved changes",
  saving: "Saving…",
  saved: "Saved to disk",
  error: "Save failed",
};

export function DocumentHeader({
  title,
  subtitle,
  saveState = "idle",
  path,
  onRename,
  onDelete,
  onBack,
  extra,
}: DocumentHeaderProps) {
  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState("");

  function startRename() {
    setDraft(title);
    setRenaming(true);
  }

  function submitRename() {
    const value = draft.trim();
    setRenaming(false);
    if (value && value !== title && onRename) onRename(value);
  }

  return (
    <header className="doc-header">
      {onBack ? <BackButton onClick={onBack} /> : null}
      <div className="doc-header-text">
        {renaming ? (
          <input
            autoFocus
            className="doc-rename"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={submitRename}
            onKeyDown={(e) => {
              if (e.key === "Enter") submitRename();
              if (e.key === "Escape") setRenaming(false);
            }}
          />
        ) : (
          <h2 onDoubleClick={onRename ? startRename : undefined} title="Double-click to rename">
            {title}
          </h2>
        )}
        {subtitle ? <small className="muted">{subtitle}</small> : null}
      </div>
      <div className="doc-header-actions">
        <span className={`save-state save-${saveState}`}>{SAVE_LABEL[saveState]}</span>
        {extra}
        {path && !isMobile ? (
          <button
            type="button"
            className="btn btn-small"
            title="Reveal in file manager"
            onClick={() => void api.revealInFolder(path)}
          >
            Reveal
          </button>
        ) : null}
        {onRename ? (
          <button type="button" className="btn btn-small" onClick={startRename}>
            Rename
          </button>
        ) : null}
        {onDelete ? (
          <button type="button" className="btn btn-small btn-danger" onClick={onDelete}>
            Delete
          </button>
        ) : null}
      </div>
    </header>
  );
}

/** "‹ Back" button shown only in the narrow (phone) layout. */
export function BackButton({ onClick }: { onClick: () => void }) {
  return (
    <button type="button" className="btn btn-small back-button" onClick={onClick}>
      ‹ Back
    </button>
  );
}
