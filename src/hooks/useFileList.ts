import { useCallback, useEffect, useState } from "react";

import * as api from "../api";
import { useToast } from "../context/ToastContext";
import { useVaultChanges } from "../context/VaultContext";
import { changeCategory, errorMessage, type Category, type FileEntry } from "../types";

/** Keeps a category's file list in sync with the disk and the watcher. */
export function useFileList(category: Category) {
  const toast = useToast();
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(async () => {
    try {
      setEntries(await api.listFiles(category));
    } catch (error) {
      toast.push("error", `Could not list ${category}`, errorMessage(error));
    } finally {
      setLoading(false);
    }
  }, [category, toast]);

  useEffect(() => {
    setLoading(true);
    void refresh();
  }, [refresh]);

  useVaultChanges(
    (change) => changeCategory(change) === category,
    () => void refresh(),
  );

  return { entries, loading, refresh };
}
