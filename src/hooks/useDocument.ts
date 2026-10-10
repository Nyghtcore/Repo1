import { useCallback, useEffect, useRef, useState } from "react";

import * as api from "../api";
import { useToast } from "../context/ToastContext";
import { useVaultChanges } from "../context/VaultContext";
import { changeCategory, errorMessage, type Category, type DocumentPayload } from "../types";

export type SaveState = "idle" | "dirty" | "saving" | "saved" | "error";

const TYPING_DEBOUNCE_MS = 400;

/**
 * Loads a markdown document, streams edits back to disk (debounced while
 * typing, immediate for checkbox toggles) and reloads when the watcher
 * reports that another peer changed the file.
 */
export function useDocument(category: Category, name: string | null) {
  const toast = useToast();
  const [doc, setDoc] = useState<DocumentPayload | null>(null);
  const [loading, setLoading] = useState(false);
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const pending = useRef<string | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const inFlight = useRef<Promise<void> | null>(null);
  const lastWritten = useRef<string>("");
  const activeName = useRef<string | null>(name);
  activeName.current = name;

  const load = useCallback(async () => {
    if (!name) {
      setDoc(null);
      return;
    }
    setLoading(true);
    try {
      const payload = await api.readDocument(category, name);
      if (activeName.current !== name) return;
      lastWritten.current = payload.body;
      setDoc(payload);
      setSaveState("idle");
    } catch (error) {
      if (activeName.current === name) {
        toast.push("error", "Could not open document", errorMessage(error));
        setDoc(null);
      }
    } finally {
      setLoading(false);
    }
  }, [category, name, toast]);

  // Initial load + reload on document switch.
  useEffect(() => {
    pending.current = null;
    window.clearTimeout(timer.current);
    void load();
  }, [load]);

  const flush = useCallback(async () => {
    window.clearTimeout(timer.current);
    const body = pending.current;
    if (body === null || !name) return;
    pending.current = null;
    setSaveState("saving");
    const write = (async () => {
      try {
        if (inFlight.current) await inFlight.current;
        await api.writeDocument({ category, name, body });
        lastWritten.current = body;
        setSaveState(pending.current === null ? "saved" : "dirty");
      } catch (error) {
        setSaveState("error");
        toast.push("error", "Save failed", errorMessage(error));
      }
    })();
    inFlight.current = write;
    await write;
    inFlight.current = null;
  }, [category, name, toast]);

  const update = useCallback(
    (body: string, immediate: boolean) => {
      pending.current = body;
      setSaveState("dirty");
      window.clearTimeout(timer.current);
      if (immediate) {
        void flush();
      } else {
        timer.current = window.setTimeout(() => void flush(), TYPING_DEBOUNCE_MS);
      }
    },
    [flush],
  );

  // Flush unsaved edits when unmounting or switching documents.
  useEffect(() => {
    return () => {
      if (pending.current !== null) void flush();
    };
  }, [flush]);

  // Reload when a peer (Syncthing) or another command changed this file.
  useVaultChanges(
    (change) => changeCategory(change) === category && change.name === name,
    () => {
      if (pending.current !== null || inFlight.current) return; // our own write
      void load();
    },
  );

  const toggleTask = useCallback(
    async (line: number, checked: boolean) => {
      if (!name) return;
      try {
        if (inFlight.current) await inFlight.current;
        const tasks = await api.toggleTask(category, name, line, checked);
        setDoc((current) => (current ? { ...current, tasks } : current));
        // Pull the rewritten body so the editor view matches the disk.
        const payload = await api.readDocument(category, name);
        lastWritten.current = payload.body;
        setDoc(payload);
        setSaveState("saved");
      } catch (error) {
        toast.push("error", "Could not update task", errorMessage(error));
      }
    },
    [category, name, toast],
  );

  return { doc, loading, saveState, update, flush, reload: load, toggleTask };
}
