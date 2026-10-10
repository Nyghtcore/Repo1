import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

import * as api from "../api";
import type { AppSettings, Reminder, VaultChange, VaultInfo } from "../types";
import { errorMessage } from "../types";
import { useToast } from "./ToastContext";

type ChangeListener = (change: VaultChange) => void;

interface VaultApi {
  vault: VaultInfo | null;
  settings: AppSettings;
  busy: boolean;
  create: (path: string, masterPassword: string) => Promise<void>;
  unlock: (path: string, masterPassword: string) => Promise<void>;
  lock: () => Promise<void>;
  refreshSettings: () => Promise<void>;
  /** Subscribe to filesystem changes forwarded by the Rust watcher. */
  subscribe: (listener: ChangeListener) => () => void;
}

const VaultContext = createContext<VaultApi | null>(null);

const EMPTY_SETTINGS: AppSettings = { lastVaultPath: null, recentVaults: [] };

export function VaultProvider({ children }: { children: ReactNode }) {
  const [vault, setVault] = useState<VaultInfo | null>(null);
  const [settings, setSettings] = useState<AppSettings>(EMPTY_SETTINGS);
  const [busy, setBusy] = useState(false);
  const listeners = useRef(new Set<ChangeListener>());
  const toast = useToast();

  const refreshSettings = useCallback(async () => {
    try {
      setSettings(await api.getSettings());
    } catch (error) {
      console.warn("could not load settings", error);
    }
  }, []);

  // Restore state on mount (e.g. after a webview reload during development).
  useEffect(() => {
    void refreshSettings();
    api
      .currentVault()
      .then((info) => setVault(info && info.unlocked ? info : null))
      .catch(() => setVault(null));
  }, [refreshSettings]);

  // Forward backend events to subscribers.
  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];

    api
      .onVaultChanged((change) => {
        listeners.current.forEach((listener) => listener(change));
      })
      .then((unlisten) => {
        if (disposed) unlisten();
        else unlisteners.push(unlisten);
      })
      .catch((error) => console.warn("vault change listener failed", error));

    api
      .onReminderFired((reminder: Reminder) => {
        toast.push("reminder", reminder.text || "Reminder", reminder.rawTimestamp);
      })
      .then((unlisten) => {
        if (disposed) unlisten();
        else unlisteners.push(unlisten);
      })
      .catch((error) => console.warn("reminder listener failed", error));

    return () => {
      disposed = true;
      unlisteners.forEach((fn) => fn());
    };
  }, [toast]);

  const subscribe = useCallback((listener: ChangeListener) => {
    listeners.current.add(listener);
    return () => {
      listeners.current.delete(listener);
    };
  }, []);

  const create = useCallback(
    async (path: string, masterPassword: string) => {
      setBusy(true);
      try {
        const info = await api.createVault(path, masterPassword);
        setVault(info);
        await refreshSettings();
        toast.push("success", "Vault created", info.path);
      } finally {
        setBusy(false);
      }
    },
    [refreshSettings, toast],
  );

  const unlock = useCallback(
    async (path: string, masterPassword: string) => {
      setBusy(true);
      try {
        const info = await api.unlockVault(path, masterPassword);
        setVault(info);
        await refreshSettings();
      } finally {
        setBusy(false);
      }
    },
    [refreshSettings],
  );

  const lock = useCallback(async () => {
    try {
      await api.lockVault();
    } catch (error) {
      toast.push("error", "Could not lock vault", errorMessage(error));
    } finally {
      setVault(null);
    }
  }, [toast]);

  const value = useMemo<VaultApi>(
    () => ({ vault, settings, busy, create, unlock, lock, refreshSettings, subscribe }),
    [vault, settings, busy, create, unlock, lock, refreshSettings, subscribe],
  );

  return <VaultContext.Provider value={value}>{children}</VaultContext.Provider>;
}

export function useVault(): VaultApi {
  const ctx = useContext(VaultContext);
  if (!ctx) throw new Error("useVault must be used inside <VaultProvider>");
  return ctx;
}

/**
 * Run `handler` whenever the watcher reports a change matching `filter`.
 * The handler is debounced slightly so a burst of Syncthing writes causes a
 * single refresh.
 */
export function useVaultChanges(
  filter: (change: VaultChange) => boolean,
  handler: (change: VaultChange) => void,
) {
  const { subscribe } = useVault();
  const filterRef = useRef(filter);
  const handlerRef = useRef(handler);
  filterRef.current = filter;
  handlerRef.current = handler;

  useEffect(() => {
    let timer: number | undefined;
    let last: VaultChange | null = null;
    const unsubscribe = subscribe((change) => {
      if (!filterRef.current(change)) return;
      last = change;
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        if (last) handlerRef.current(last);
      }, 150);
    });
    return () => {
      window.clearTimeout(timer);
      unsubscribe();
    };
  }, [subscribe]);
}
