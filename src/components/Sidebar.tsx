import { useState, type FormEvent } from "react";

import * as api from "../api";
import { useToast } from "../context/ToastContext";
import { useVault } from "../context/VaultContext";
import { errorMessage } from "../types";
import { isMobile } from "../platform";

export type Section = "passwords" | "notes" | "tasks" | "media" | "reminders" | "vault";

const NAV: Array<{ id: Section; label: string; icon: string }> = [
  { id: "passwords", label: "Passwords", icon: "🔐" },
  { id: "notes", label: "Notes", icon: "📝" },
  { id: "tasks", label: "Tasks", icon: "☑️" },
  { id: "media", label: "Media", icon: "🖼️" },
  { id: "reminders", label: "Reminders", icon: "⏰" },
];

/** Phone-only tab holding the actions that live in the sidebar footer on desktop. */
const MOBILE_VAULT_TAB = { id: "vault" as const, label: "Vault", icon: "🔑" };

export function Sidebar({
  section,
  onSelect,
}: {
  section: Section;
  onSelect: (section: Section) => void;
}) {
  const { vault } = useVault();
  const items = isMobile ? [...NAV, MOBILE_VAULT_TAB] : NAV;

  return (
    <nav className="sidebar">
      <div className="sidebar-brand">
        <span className="brand-mark">Z</span>
        <div className="sidebar-brand-text">
          <strong>Zynky</strong>
          <small className="muted" title={vault?.path ?? ""}>
            {vault?.name ?? "No vault"}
          </small>
        </div>
      </div>

      <ul className="sidebar-nav">
        {items.map((item) => (
          <li key={item.id}>
            <button
              type="button"
              className={`nav-item${section === item.id ? " active" : ""}`}
              onClick={() => onSelect(item.id)}
            >
              <span className="nav-icon" aria-hidden>
                {item.icon}
              </span>
              <span className="nav-label">{item.label}</span>
            </button>
          </li>
        ))}
      </ul>

      <div className="sidebar-footer">
        <VaultActions />
      </div>
    </nav>
  );
}

/** Lock, change password and open-folder actions (sidebar footer / phone Vault tab). */
export function VaultActions() {
  const { vault, lock } = useVault();
  const toast = useToast();
  const [showChange, setShowChange] = useState(isMobile);
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [working, setWorking] = useState(false);

  async function changePassword(event: FormEvent) {
    event.preventDefault();
    if (next.length < 8) {
      toast.push("error", "Use at least 8 characters");
      return;
    }
    if (next !== confirm) {
      toast.push("error", "New passwords do not match");
      return;
    }
    setWorking(true);
    try {
      await api.changeMasterPassword(current, next);
      toast.push("success", "Master password changed", "All credentials were re-encrypted.");
      setShowChange(isMobile);
      setCurrent("");
      setNext("");
      setConfirm("");
    } catch (error) {
      toast.push("error", "Could not change password", errorMessage(error));
    } finally {
      setWorking(false);
    }
  }

  return (
    <>
      {vault && !isMobile ? (
        <button
          type="button"
          className="btn btn-small btn-block"
          onClick={() => void api.revealInFolder(vault.path)}
        >
          Open vault folder
        </button>
      ) : null}
      {isMobile ? null : (
        <button
          type="button"
          className="btn btn-small btn-block"
          onClick={() => setShowChange((s) => !s)}
        >
          Change master password
        </button>
      )}
      {showChange ? (
        <form className="change-password" onSubmit={changePassword}>
          {isMobile ? <strong>Change master password</strong> : null}
          <input
            type="password"
            placeholder="Current password"
            value={current}
            onChange={(e) => setCurrent(e.target.value)}
            autoComplete="current-password"
          />
          <input
            type="password"
            placeholder="New password"
            value={next}
            onChange={(e) => setNext(e.target.value)}
            autoComplete="new-password"
          />
          <input
            type="password"
            placeholder="Confirm new password"
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            autoComplete="new-password"
          />
          <button type="submit" className="btn btn-small btn-primary btn-block" disabled={working}>
            {working ? "Re-encrypting…" : "Update"}
          </button>
        </form>
      ) : null}
      <button type="button" className="btn btn-small btn-block btn-danger" onClick={() => void lock()}>
        Lock vault
      </button>
      <small className="muted sidebar-sync">
        Sync this folder with Syncthing to share it between devices.
      </small>
    </>
  );
}

/** Phone "Vault" tab: where the vault lives plus the footer actions. */
export function VaultPane() {
  const { vault } = useVault();
  return (
    <div className="pane pane-single">
      <section className="pane-main vault-pane">
        <header className="doc-header">
          <div className="doc-header-text">
            <h2>Vault</h2>
            <small className="muted">{vault?.path}</small>
          </div>
        </header>
        <p className="hint">
          Point Syncthing on this phone at the folder above and share it with your PC. Changes
          from either side appear here automatically.
        </p>
        <VaultActions />
      </section>
    </div>
  );
}
