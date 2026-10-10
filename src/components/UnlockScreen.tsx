import { useEffect, useState, type FormEvent } from "react";

import * as api from "../api";
import { useToast } from "../context/ToastContext";
import { useVault } from "../context/VaultContext";
import { errorMessage, type VaultStatus } from "../types";

type Mode = "unlock" | "create";

export function UnlockScreen() {
  const { settings, busy, unlock, create } = useVault();
  const toast = useToast();
  const [path, setPath] = useState("");
  const [status, setStatus] = useState<VaultStatus | null>(null);
  const [mode, setMode] = useState<Mode>("unlock");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [error, setError] = useState<string | null>(null);

  // Pre-fill with the last vault, or a sensible default location.
  useEffect(() => {
    if (path) return;
    if (settings.lastVaultPath) {
      setPath(settings.lastVaultPath);
      return;
    }
    api
      .defaultVaultPath()
      .then((p) => setPath((current) => current || p))
      .catch(() => undefined);
  }, [settings.lastVaultPath, path]);

  // Inspect the folder whenever the path changes.
  useEffect(() => {
    if (!path.trim()) {
      setStatus(null);
      return;
    }
    let cancelled = false;
    api
      .vaultStatus(path.trim())
      .then((s) => {
        if (cancelled) return;
        setStatus(s);
        setMode(s.initialised ? "unlock" : "create");
      })
      .catch(() => {
        if (!cancelled) setStatus(null);
      });
    return () => {
      cancelled = true;
    };
  }, [path]);

  async function browse() {
    try {
      const folder = await api.pickFolder("Choose your Zynky vault folder");
      if (folder) setPath(folder);
    } catch (e) {
      toast.push("error", "Could not open folder picker", errorMessage(e));
    }
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    setError(null);
    const trimmed = path.trim();
    if (!trimmed) {
      setError("Choose a vault folder first.");
      return;
    }
    if (!password) {
      setError("Enter your master password.");
      return;
    }
    try {
      if (mode === "create") {
        if (password.length < 8) {
          setError("Use at least 8 characters for the master password.");
          return;
        }
        if (password !== confirm) {
          setError("The two passwords do not match.");
          return;
        }
        await create(trimmed, password);
      } else {
        await unlock(trimmed, password);
      }
      setPassword("");
      setConfirm("");
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  return (
    <div className="unlock-screen">
      <form className="unlock-card" onSubmit={submit}>
        <div className="brand">
          <span className="brand-mark">Z</span>
          <div>
            <h1>Zynky</h1>
            <p>Local-first passwords, notes and tasks. Synced by you, not a server.</p>
          </div>
        </div>

        <label className="field">
          <span>Vault folder</span>
          <div className="field-row">
            <input
              type="text"
              value={path}
              placeholder="C:\\Users\\you\\Documents\\ZynkyVault"
              onChange={(e) => setPath(e.target.value)}
              spellCheck={false}
            />
            <button type="button" className="btn" onClick={browse}>
              Browse…
            </button>
          </div>
          {status ? (
            <small className="hint">
              {status.initialised
                ? "Existing vault found. Enter the master password to unlock."
                : status.exists
                  ? "Folder exists but is not a vault yet. A new ZynkyVault will be created."
                  : "Folder does not exist yet. It will be created."}
            </small>
          ) : null}
        </label>

        <div className="segmented">
          <button
            type="button"
            className={mode === "unlock" ? "active" : ""}
            onClick={() => setMode("unlock")}
          >
            Unlock
          </button>
          <button
            type="button"
            className={mode === "create" ? "active" : ""}
            onClick={() => setMode("create")}
          >
            Create new
          </button>
        </div>

        <label className="field">
          <span>Master password</span>
          <input
            type="password"
            value={password}
            autoFocus
            autoComplete="current-password"
            onChange={(e) => setPassword(e.target.value)}
          />
        </label>

        {mode === "create" ? (
          <label className="field">
            <span>Confirm master password</span>
            <input
              type="password"
              value={confirm}
              autoComplete="new-password"
              onChange={(e) => setConfirm(e.target.value)}
            />
            <small className="hint">
              Argon2id stretches this into a 256-bit key. There is no recovery if you forget it.
            </small>
          </label>
        ) : null}

        {error ? <div className="error-banner">{error}</div> : null}

        <button type="submit" className="btn btn-primary btn-block" disabled={busy}>
          {busy ? "Working…" : mode === "create" ? "Create vault" : "Unlock vault"}
        </button>

        {settings.recentVaults.length > 1 ? (
          <div className="recent">
            <span>Recent vaults</span>
            <ul>
              {settings.recentVaults.map((recent) => (
                <li key={recent}>
                  <button type="button" className="link" onClick={() => setPath(recent)}>
                    {recent}
                  </button>
                </li>
              ))}
            </ul>
          </div>
        ) : null}
      </form>
    </div>
  );
}
