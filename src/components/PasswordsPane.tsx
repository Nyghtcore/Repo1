import { useEffect, useState, type FormEvent } from "react";

import * as api from "../api";
import { useToast } from "../context/ToastContext";
import { useVaultChanges } from "../context/VaultContext";
import { useFileList } from "../hooks/useFileList";
import {
  EMPTY_SECRET,
  errorMessage,
  type Category,
  type Credential,
  type CredentialInput,
  type CredentialSecret,
} from "../types";
import { formatDateTime } from "../utils/format";
import { FileList } from "./FileList";

const CATEGORY: Category = "Passwords";

interface FormState {
  name: string | null;
  title: string;
  tags: string;
  secret: CredentialSecret;
  notes: string;
}

const EMPTY_FORM: FormState = {
  name: null,
  title: "",
  tags: "",
  secret: { ...EMPTY_SECRET, fields: {} },
  notes: "",
};

function toForm(credential: Credential): FormState {
  return {
    name: credential.name,
    title: credential.title,
    tags: credential.tags.join(", "),
    secret: { ...credential.secret, fields: { ...credential.secret.fields } },
    notes: credential.notes,
  };
}

function toInput(form: FormState): CredentialInput {
  return {
    name: form.name,
    title: form.title,
    tags: form.tags
      .split(",")
      .map((t) => t.trim())
      .filter(Boolean),
    secret: form.secret,
    notes: form.notes,
  };
}

export function PasswordsPane() {
  const toast = useToast();
  const { entries, loading, refresh } = useFileList(CATEGORY);
  const [selected, setSelected] = useState<string | null>(null);
  const [form, setForm] = useState<FormState | null>(null);
  const [loadingDoc, setLoadingDoc] = useState(false);
  const [saving, setSaving] = useState(false);
  const [reveal, setReveal] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [lastSaved, setLastSaved] = useState<string | null>(null);

  async function openCredential(name: string) {
    setLoadingDoc(true);
    try {
      const credential = await api.readCredential(name);
      setSelected(name);
      setForm(toForm(credential));
      setLastSaved(credential.updated);
      setDirty(false);
      setReveal(false);
    } catch (error) {
      toast.push("error", "Could not decrypt credential", errorMessage(error));
    } finally {
      setLoadingDoc(false);
    }
  }

  // Reload the open credential if a peer changed it and we have no local edits.
  useVaultChanges(
    (change) => change.category === CATEGORY && change.name === selected,
    () => {
      if (selected && !dirty && !saving) void openCredential(selected);
    },
  );

  useEffect(() => {
    if (selected && !entries.some((e) => e.name === selected) && !loading) {
      // The file was deleted elsewhere.
      setSelected(null);
      setForm(null);
    }
  }, [entries, selected, loading]);

  function startNew() {
    setSelected(null);
    setForm({ ...EMPTY_FORM, secret: { ...EMPTY_SECRET, fields: {} } });
    setDirty(false);
    setReveal(true);
    setLastSaved(null);
  }

  function patch(update: Partial<FormState>) {
    setForm((current) => (current ? { ...current, ...update } : current));
    setDirty(true);
  }

  function patchSecret(update: Partial<CredentialSecret>) {
    setForm((current) =>
      current ? { ...current, secret: { ...current.secret, ...update } } : current,
    );
    setDirty(true);
  }

  function setField(key: string, value: string) {
    setForm((current) => {
      if (!current) return current;
      const fields = { ...current.secret.fields, [key]: value };
      return { ...current, secret: { ...current.secret, fields } };
    });
    setDirty(true);
  }

  function removeField(key: string) {
    setForm((current) => {
      if (!current) return current;
      const fields = { ...current.secret.fields };
      delete fields[key];
      return { ...current, secret: { ...current.secret, fields } };
    });
    setDirty(true);
  }

  function addField() {
    const key = window.prompt("Field name (e.g. Security question, API key)");
    if (!key?.trim()) return;
    setField(key.trim(), "");
  }

  async function generate() {
    try {
      patchSecret({ password: await api.generatePassword(20, true) });
      setReveal(true);
    } catch (error) {
      toast.push("error", "Could not generate password", errorMessage(error));
    }
  }

  async function copy(label: string, value: string) {
    try {
      await navigator.clipboard.writeText(value);
      toast.push("success", `${label} copied`);
    } catch (error) {
      toast.push("error", "Clipboard unavailable", errorMessage(error));
    }
  }

  async function save(event: FormEvent) {
    event.preventDefault();
    if (!form) return;
    if (!form.title.trim()) {
      toast.push("error", "A title is required");
      return;
    }
    setSaving(true);
    try {
      const saved = await api.saveCredential(toInput(form));
      await refresh();
      setSelected(saved.name);
      setForm(toForm(saved));
      setLastSaved(saved.updated);
      setDirty(false);
      toast.push("success", "Credential encrypted and saved", saved.name);
    } catch (error) {
      toast.push("error", "Could not save credential", errorMessage(error));
    } finally {
      setSaving(false);
    }
  }

  async function remove() {
    if (!selected) return;
    if (!window.confirm(`Delete "${selected}" permanently?`)) return;
    try {
      await api.deleteFile(CATEGORY, selected);
      setSelected(null);
      setForm(null);
      await refresh();
    } catch (error) {
      toast.push("error", "Could not delete credential", errorMessage(error));
    }
  }

  return (
    <div className="pane">
      <FileList
        title="Passwords"
        entries={entries}
        selected={selected}
        loading={loading}
        onSelect={(entry) => void openCredential(entry.name)}
        onCreate={() => startNew()}
        askTitle={false}
        createLabel="Credential"
        emptyHint="No credentials yet. Add one to get started."
      />
      <section className="pane-main">
        {form ? (
          <form className="credential-form" onSubmit={save}>
            <header className="doc-header">
              <div className="doc-header-text">
                <h2>{form.title || "New credential"}</h2>
                <small className="muted">
                  {form.name ? `${form.name} · ` : "Not saved yet · "}
                  {lastSaved ? `updated ${formatDateTime(lastSaved)}` : ""}
                </small>
              </div>
              <div className="doc-header-actions">
                <span className={`save-state ${dirty ? "save-dirty" : "save-saved"}`}>
                  {dirty ? "Unsaved changes" : form.name ? "Encrypted on disk" : ""}
                </span>
                {form.name ? (
                  <button type="button" className="btn btn-small btn-danger" onClick={remove}>
                    Delete
                  </button>
                ) : null}
                <button type="submit" className="btn btn-small btn-primary" disabled={saving}>
                  {saving ? "Saving…" : "Save"}
                </button>
              </div>
            </header>

            {loadingDoc ? <p className="muted">Decrypting…</p> : null}

            <div className="form-grid">
              <label className="field">
                <span>Title</span>
                <input
                  type="text"
                  value={form.title}
                  onChange={(e) => patch({ title: e.target.value })}
                  placeholder="GitHub"
                  required
                />
              </label>
              <label className="field">
                <span>Tags (comma separated)</span>
                <input
                  type="text"
                  value={form.tags}
                  onChange={(e) => patch({ tags: e.target.value })}
                  placeholder="dev, work"
                />
              </label>
              <label className="field">
                <span>Username / e-mail</span>
                <div className="field-row">
                  <input
                    type="text"
                    value={form.secret.username}
                    onChange={(e) => patchSecret({ username: e.target.value })}
                    autoComplete="off"
                  />
                  <button
                    type="button"
                    className="btn"
                    onClick={() => void copy("Username", form.secret.username)}
                  >
                    Copy
                  </button>
                </div>
              </label>
              <label className="field">
                <span>Password</span>
                <div className="field-row">
                  <input
                    type={reveal ? "text" : "password"}
                    value={form.secret.password}
                    onChange={(e) => patchSecret({ password: e.target.value })}
                    autoComplete="new-password"
                    spellCheck={false}
                  />
                  <button type="button" className="btn" onClick={() => setReveal((r) => !r)}>
                    {reveal ? "Hide" : "Show"}
                  </button>
                  <button type="button" className="btn" onClick={generate}>
                    Generate
                  </button>
                  <button
                    type="button"
                    className="btn"
                    onClick={() => void copy("Password", form.secret.password)}
                  >
                    Copy
                  </button>
                </div>
              </label>
              <label className="field">
                <span>Website / URL</span>
                <div className="field-row">
                  <input
                    type="text"
                    value={form.secret.url}
                    onChange={(e) => patchSecret({ url: e.target.value })}
                    placeholder="https://"
                  />
                  {form.secret.url ? (
                    <button
                      type="button"
                      className="btn"
                      onClick={() => void api.openInSystem(form.secret.url)}
                    >
                      Open
                    </button>
                  ) : null}
                </div>
              </label>
              <label className="field">
                <span>TOTP secret (optional)</span>
                <input
                  type={reveal ? "text" : "password"}
                  value={form.secret.totpSecret}
                  onChange={(e) => patchSecret({ totpSecret: e.target.value })}
                  autoComplete="off"
                  spellCheck={false}
                />
              </label>
            </div>

            <div className="custom-fields">
              <div className="custom-fields-header">
                <span>Extra encrypted fields</span>
                <button type="button" className="btn btn-small" onClick={addField}>
                  + Field
                </button>
              </div>
              {Object.entries(form.secret.fields).map(([key, value]) => (
                <div key={key} className="field-row custom-field">
                  <code>{key}</code>
                  <input
                    type={reveal ? "text" : "password"}
                    value={value}
                    onChange={(e) => setField(key, e.target.value)}
                    autoComplete="off"
                  />
                  <button type="button" className="btn" onClick={() => void copy(key, value)}>
                    Copy
                  </button>
                  <button
                    type="button"
                    className="btn btn-danger"
                    onClick={() => removeField(key)}
                  >
                    ×
                  </button>
                </div>
              ))}
            </div>

            <label className="field">
              <span>Notes (plain Markdown, stored unencrypted in the file body)</span>
              <textarea
                rows={8}
                value={form.notes}
                onChange={(e) => patch({ notes: e.target.value })}
                placeholder="Recovery codes location, account quirks, anything you want readable without unlocking…"
              />
            </label>
          </form>
        ) : (
          <div className="empty-state">
            <h3>Passwords</h3>
            <p>
              Each credential is a Markdown file. Secrets are encrypted with AES-256-GCM into the{" "}
              <code>secure_blob</code> frontmatter key; the body stays readable.
            </p>
            <button type="button" className="btn btn-primary" onClick={startNew}>
              Add credential
            </button>
          </div>
        )}
      </section>
    </div>
  );
}
