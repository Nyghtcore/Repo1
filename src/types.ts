/**
 * TypeScript mirrors of the Rust data contracts in `src-tauri/src/vault.rs`,
 * `tasks.rs`, `reminder.rs` and `watcher.rs`.
 *
 * Every Rust struct uses `#[serde(rename_all = "camelCase")]`, so the field
 * names below are exactly what crosses the Tauri IPC boundary.
 */

/**
 * The four top-level folders of a vault plus `Root` for markdown files that
 * live directly in the vault root (`reminders.md`). Serialised as the enum
 * variant name.
 */
export type Category = "Passwords" | "Notes" | "Tasks" | "Media" | "Root";

/** Folder categories (excludes the synthetic `Root`). */
export const CATEGORIES: Category[] = ["Passwords", "Notes", "Tasks", "Media"];

/** Map the watcher's `category` (null for root files) onto a `Category`. */
export function changeCategory(change: { category: string | null }): string {
  return change.category ?? "Root";
}

/** Serialised `AppError` returned by every failing command. */
export interface AppError {
  code:
    | "no_vault"
    | "locked"
    | "not_initialised"
    | "already_initialised"
    | "invalid_name"
    | "unknown_category"
    | "not_found"
    | "io"
    | "bad_password"
    | "crypto"
    | "frontmatter"
    | "task"
    | "json"
    | "watcher"
    | "other";
  message: string;
}

export function isAppError(value: unknown): value is AppError {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as AppError).code === "string" &&
    typeof (value as AppError).message === "string"
  );
}

export function errorMessage(error: unknown): string {
  if (isAppError(error)) return error.message;
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return "Unknown error";
}

export interface VaultStatus {
  path: string;
  exists: boolean;
  initialised: boolean;
  unlocked: boolean;
}

export interface VaultInfo {
  path: string;
  name: string;
  unlocked: boolean;
  /** RFC 3339 UTC timestamp. */
  created: string | null;
}

export interface TaskSummary {
  total: number;
  completed: number;
}

export interface TaskItem {
  /** Zero-based line index inside the markdown body. */
  line: number;
  indent: number;
  checked: boolean;
  text: string;
}

export interface FileEntry {
  category: Category;
  name: string;
  path: string;
  size: number;
  /** RFC 3339 UTC timestamp. */
  modified: string | null;
  title: string;
  tags: string[];
  tasks: TaskSummary | null;
}

/** Frontmatter is an arbitrary JSON object. */
export type Frontmatter = Record<string, unknown>;

export interface DocumentPayload {
  category: Category;
  name: string;
  path: string;
  frontmatter: Frontmatter;
  body: string;
  tasks: TaskItem[];
  modified: string | null;
}

export interface WriteDocumentInput {
  category: Category;
  name: string;
  body: string;
  frontmatter?: Frontmatter | null;
}

export interface CredentialSecret {
  username: string;
  password: string;
  url: string;
  totpSecret: string;
  fields: Record<string, string>;
}

export const EMPTY_SECRET: CredentialSecret = {
  username: "",
  password: "",
  url: "",
  totpSecret: "",
  fields: {},
};

export interface Credential {
  name: string;
  path: string;
  title: string;
  tags: string[];
  created: string | null;
  updated: string | null;
  secret: CredentialSecret;
  notes: string;
}

export interface CredentialInput {
  name: string | null;
  title: string;
  tags: string[];
  secret: CredentialSecret;
  notes: string;
}

export interface MediaPayload {
  name: string;
  path: string;
  mime: string;
  size: number;
  dataUrl: string | null;
}

export interface AppSettings {
  lastVaultPath: string | null;
  recentVaults: string[];
}

export interface Reminder {
  line: number;
  text: string;
  checked: boolean;
  /** RFC 3339 local timestamp. */
  due: string;
  rawTimestamp: string;
  overdue: boolean;
}

export interface EngineStatus {
  running: boolean;
  lastTick: string | null;
  nextTick: string | null;
  firedTotal: number;
}

export type ChangeKind = "created" | "modified" | "removed" | "other";

/** Payload of the `vault://changed` event emitted by the Rust watcher. */
export interface VaultChange {
  category: string | null;
  name: string;
  relativePath: string;
  kind: ChangeKind;
}

/** Event names shared with the backend. */
export const EVENTS = {
  vaultChanged: "vault://changed",
  reminderFired: "reminder://fired",
} as const;

/** Name of the reminders file in the vault root. */
export const REMINDERS_FILE = "reminders.md";
