import { useState, type FormEvent } from "react";

import * as api from "../api";
import { useToast } from "../context/ToastContext";
import { useVault } from "../context/VaultContext";
import { errorMessage } from "../types";

export type Section = "passwords" | "notes" | "tasks" | "media" | "reminders";

const NAV: Array<{ id: Section; label: string; icon: string }> = [
  { id: "passwords", label: "Passwords", icon: "🔐" },
  { id: "notes", label: "Notes", icon: "📝" },
  { id: "tasks", label: "Tasks", icon: "☑️" },
  { id: "media", label: "Media", icon: "🖼️" },
  { id: "reminders", label: "Reminders", icon: "⏰" },
];

export function Sidebar({
  section,
  onSelect,
}: {
  section: Section;
  onSelect: (section: Section) => void;
}) {
  const { vault, lock } = useVault();
  const toast = useToast();
  const [showChange, setShowChange] = useState(false);
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
      setShowChange(false);
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
        {NAV.map((item) => (
          <li key={item.id}>
            <button
              type="button"
              className={`nav-item${section === item.id ? " active" : ""}`}
              onClick={() => onSelect(item.id)}
            >
              <span className="nav-icon" aria-hidden>
                {item.icon}
              </span>
              {item.label}
            </button>
          </li>
        ))}
      </ul>

      <div className="sidebar-footer">
        {vault ? (
          <button
            type="button"
            className="btn btn-small btn-block"
            onClick={() => void api.revealInFolder(vault.path)}
          >
            Open vault folder
          </button>
        ) : null}
        <button
          type="button"
          className="btn btn-small btn-block"
          onClick={() => setShowChange((s) => !s)}
        >
          Change master password
        </button>
        {showChange ? (
          <form className="change-password" onSubmit={changePassword}>
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
      </div>
    </nav>
  );
}
