import { useState } from "react";

import * as api from "../api";
import { useToast } from "../context/ToastContext";
import { useDocument } from "../hooks/useDocument";
import { useFileList } from "../hooks/useFileList";
import { errorMessage, type Category } from "../types";
import { formatDateTime } from "../utils/format";
import { DocumentHeader } from "./DocumentHeader";
import { FileList } from "./FileList";
import { MarkdownEditor } from "./MarkdownEditor";

const CATEGORY: Category = "Notes";

export function NotesPane() {
  const toast = useToast();
  const { entries, loading, refresh } = useFileList(CATEGORY);
  const [selected, setSelected] = useState<string | null>(null);
  const { doc, saveState, update } = useDocument(CATEGORY, selected);

  async function create(title: string) {
    try {
      const entry = await api.createDocument(CATEGORY, title);
      await refresh();
      setSelected(entry.name);
    } catch (error) {
      toast.push("error", "Could not create note", errorMessage(error));
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
      toast.push("error", "Could not delete note", errorMessage(error));
    }
  }

  async function rename(newName: string) {
    if (!selected) return;
    try {
      const entry = await api.renameFile(CATEGORY, selected, newName);
      await refresh();
      setSelected(entry.name);
    } catch (error) {
      toast.push("error", "Could not rename note", errorMessage(error));
    }
  }

  return (
    <div className="pane">
      <FileList
        title="Notes"
        entries={entries}
        selected={selected}
        loading={loading}
        onSelect={(entry) => setSelected(entry.name)}
        onCreate={create}
        createLabel="Note"
        emptyHint="Create your first note."
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
            />
            <MarkdownEditor
              value={doc.body}
              documentKey={`${CATEGORY}/${doc.name}`}
              onChange={update}
              placeholder="Write your note in Markdown…"
            />
          </>
        ) : (
          <div className="empty-state">
            <h3>Notes</h3>
            <p>Select a note on the left or create a new one. Everything is saved as plain Markdown.</p>
          </div>
        )}
      </section>
    </div>
  );
}
