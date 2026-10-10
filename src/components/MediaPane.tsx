import { useEffect, useRef, useState } from "react";

import * as api from "../api";
import { useToast } from "../context/ToastContext";
import { useFileList } from "../hooks/useFileList";
import { errorMessage, type Category, type FileEntry, type MediaPayload } from "../types";
import { formatBytes, formatDateTime } from "../utils/format";
import { fileToBase64, isMobile } from "../platform";
import { BackButton } from "./DocumentHeader";
import { FileList } from "./FileList";

const CATEGORY: Category = "Media";

export function MediaPane() {
  const toast = useToast();
  const { entries, loading, refresh } = useFileList(CATEGORY);
  const [selected, setSelected] = useState<string | null>(null);
  const [media, setMedia] = useState<MediaPayload | null>(null);
  const [importing, setImporting] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!selected) {
      setMedia(null);
      return;
    }
    let cancelled = false;
    api
      .readMedia(selected)
      .then((payload) => {
        if (!cancelled) setMedia(payload);
      })
      .catch((error) => {
        if (!cancelled) {
          toast.push("error", "Could not read file", errorMessage(error));
          setMedia(null);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [selected, toast]);

  async function importFiles() {
    if (isMobile) {
      // Android's picker returns content:// URIs, so read the bytes in the webview.
      fileInput.current?.click();
      return;
    }
    setImporting(true);
    try {
      const paths = await api.pickFiles("Import files into the vault");
      let last: FileEntry | null = null;
      for (const path of paths) {
        last = await api.importMedia(path);
      }
      if (last) {
        await refresh();
        setSelected(last.name);
        toast.push("success", `Imported ${paths.length} file${paths.length === 1 ? "" : "s"}`);
      }
    } catch (error) {
      toast.push("error", "Import failed", errorMessage(error));
    } finally {
      setImporting(false);
    }
  }

  async function importPicked(files: FileList | null) {
    if (!files || files.length === 0) return;
    setImporting(true);
    try {
      let last: FileEntry | null = null;
      for (const file of Array.from(files)) {
        last = await api.importMediaBytes(file.name, await fileToBase64(file));
      }
      if (last) {
        await refresh();
        setSelected(last.name);
        toast.push("success", `Imported ${files.length} file${files.length === 1 ? "" : "s"}`);
      }
    } catch (error) {
      toast.push("error", "Import failed", errorMessage(error));
    } finally {
      setImporting(false);
      if (fileInput.current) fileInput.current.value = "";
    }
  }

  function openFile(path: string) {
    api
      .openInSystem(path)
      .catch((error) => toast.push("error", "Could not open file", errorMessage(error)));
  }

  async function remove() {
    if (!selected) return;
    if (!window.confirm(`Delete "${selected}" permanently?`)) return;
    try {
      await api.deleteFile(CATEGORY, selected);
      setSelected(null);
      await refresh();
    } catch (error) {
      toast.push("error", "Could not delete file", errorMessage(error));
    }
  }

  const selectedEntry = entries.find((e) => e.name === selected) ?? null;

  return (
    <div className={`pane${selected && selectedEntry ? " pane-detail-open" : ""}`}>
      <input
        ref={fileInput}
        type="file"
        multiple
        hidden
        onChange={(e) => void importPicked(e.target.files)}
      />
      <FileList
        title="Media"
        entries={entries}
        selected={selected}
        loading={loading}
        onSelect={(entry) => setSelected(entry.name)}
        onCreate={() => void importFiles()}
        askTitle={false}
        createLabel={importing ? "Importing…" : "Import"}
        renderMeta={(entry) => formatBytes(entry.size)}
        emptyHint="Import images, PDFs or any attachment."
      />
      <section className="pane-main">
        {selected && selectedEntry ? (
          <>
            <header className="doc-header">
              <BackButton onClick={() => setSelected(null)} />
              <div className="doc-header-text">
                <h2>{selectedEntry.name}</h2>
                <small className="muted">
                  {formatBytes(selectedEntry.size)} · {formatDateTime(selectedEntry.modified)}
                  {media ? ` · ${media.mime}` : ""}
                </small>
              </div>
              <div className="doc-header-actions">
                <button
                  type="button"
                  className="btn btn-small"
                  onClick={() => openFile(selectedEntry.path)}
                >
                  Open
                </button>
                {isMobile ? null : (
                  <button
                    type="button"
                    className="btn btn-small"
                    onClick={() => void api.revealInFolder(selectedEntry.path)}
                  >
                    Reveal
                  </button>
                )}
                <button type="button" className="btn btn-small btn-danger" onClick={remove}>
                  Delete
                </button>
              </div>
            </header>
            <div className="media-preview">
              {media?.dataUrl && media.mime.startsWith("image/") ? (
                <img src={media.dataUrl} alt={media.name} />
              ) : media?.dataUrl && media.mime.startsWith("video/") ? (
                <video src={media.dataUrl} controls />
              ) : media?.dataUrl && media.mime.startsWith("audio/") ? (
                <audio src={media.dataUrl} controls />
              ) : media?.dataUrl && media.mime === "application/pdf" ? (
                <iframe src={media.dataUrl} title={media.name} />
              ) : (
                <div className="empty-state">
                  <p>No inline preview for this file type. Use “Open” to view it with the default app.</p>
                </div>
              )}
            </div>
          </>
        ) : (
          <div className="empty-state">
            <h3>Media</h3>
            <p>Attachments are copied into the vault's Media folder and synced like everything else.</p>
            <button type="button" className="btn btn-primary" onClick={() => void importFiles()}>
              Import files
            </button>
          </div>
        )}
      </section>
    </div>
  );
}
