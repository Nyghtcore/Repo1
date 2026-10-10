/**
 * Typed wrappers around every Tauri command exposed by `src-tauri/src/lib.rs`.
 * Keeping the `invoke` calls in one place means the rest of the UI never deals
 * with raw command names or argument casing.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openPath, openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";

import type {
  AppSettings,
  Category,
  Credential,
  CredentialInput,
  DocumentPayload,
  EngineStatus,
  FileEntry,
  MediaPayload,
  Reminder,
  TaskItem,
  VaultChange,
  VaultInfo,
  VaultStatus,
  WriteDocumentInput,
} from "./types";
import { EVENTS } from "./types";

// ---------------------------------------------------------------------------
// Vault lifecycle
// ---------------------------------------------------------------------------

export const vaultStatus = (path: string) =>
  invoke<VaultStatus>("vault_status", { path });

export const createVault = (path: string, masterPassword: string) =>
  invoke<VaultInfo>("create_vault", { path, masterPassword });

export const unlockVault = (path: string, masterPassword: string) =>
  invoke<VaultInfo>("unlock_vault", { path, masterPassword });

export const lockVault = () => invoke<void>("lock_vault");

export const currentVault = () => invoke<VaultInfo | null>("current_vault");

export const changeMasterPassword = (
  currentPassword: string,
  newPassword: string,
) => invoke<void>("change_master_password", { currentPassword, newPassword });

export const getSettings = () => invoke<AppSettings>("get_settings");

export const defaultVaultPath = () => invoke<string>("default_vault_path");

// ---------------------------------------------------------------------------
// Documents
// ---------------------------------------------------------------------------

export const listFiles = (category: Category) =>
  invoke<FileEntry[]>("list_files", { category });

export const readDocument = (category: Category, name: string) =>
  invoke<DocumentPayload>("read_document", { category, name });

export const writeDocument = (input: WriteDocumentInput) =>
  invoke<FileEntry>("write_document", { input });

export const createDocument = (category: Category, title: string) =>
  invoke<FileEntry>("create_document", { category, title });

export const renameFile = (category: Category, name: string, newName: string) =>
  invoke<FileEntry>("rename_file", { category, name, newName });

export const deleteFile = (category: Category, name: string) =>
  invoke<void>("delete_file", { category, name });

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

export const listTasks = (category: Category, name: string) =>
  invoke<TaskItem[]>("list_tasks", { category, name });

export const toggleTask = (
  category: Category,
  name: string,
  line: number,
  checked: boolean,
) => invoke<TaskItem[]>("toggle_task", { category, name, line, checked });

// ---------------------------------------------------------------------------
// Credentials
// ---------------------------------------------------------------------------

export const readCredential = (name: string) =>
  invoke<Credential>("read_credential", { name });

export const saveCredential = (input: CredentialInput) =>
  invoke<Credential>("save_credential", { input });

export const generatePassword = (length = 20, symbols = true) =>
  invoke<string>("generate_password", { length, symbols });

// ---------------------------------------------------------------------------
// Media
// ---------------------------------------------------------------------------

export const importMedia = (sourcePath: string) =>
  invoke<FileEntry>("import_media", { sourcePath });

/** Import file contents read by the webview (Android file picker path). */
export const importMediaBytes = (name: string, dataBase64: string) =>
  invoke<FileEntry>("import_media_bytes", { name, dataBase64 });

export const readMedia = (name: string) =>
  invoke<MediaPayload>("read_media", { name });

// ---------------------------------------------------------------------------
// Reminders
// ---------------------------------------------------------------------------

export const listReminders = () => invoke<Reminder[]>("list_reminders");

export const testNotification = (title?: string, body?: string) =>
  invoke<void>("test_notification", { title, body });

export const reminderEngineStatus = () =>
  invoke<EngineStatus>("reminder_engine_status");

export const formatReminderTimestamp = (
  year: number,
  month: number,
  day: number,
  hour: number,
  minute: number,
) =>
  invoke<string>("format_reminder_timestamp", {
    year,
    month,
    day,
    hour,
    minute,
  });

// ---------------------------------------------------------------------------
// Native dialogs & shell helpers
// ---------------------------------------------------------------------------

export async function pickFolder(title: string): Promise<string | null> {
  const selection = await openDialog({ directory: true, multiple: false, title });
  return typeof selection === "string" ? selection : null;
}

export async function pickFiles(title: string): Promise<string[]> {
  const selection = await openDialog({ directory: false, multiple: true, title });
  if (Array.isArray(selection)) return selection;
  if (typeof selection === "string") return [selection];
  return [];
}

export const openInSystem = (path: string) => openPath(path);
export const revealInFolder = (path: string) => revealItemInDir(path);

/** Open a web address in the default browser, adding https:// if missing. */
export function openWebsite(url: string): Promise<void> {
  const trimmed = url.trim();
  const full = /^[a-z][a-z0-9+.-]*:/i.test(trimmed) ? trimmed : `https://${trimmed}`;
  return openUrl(full);
}

/** Ask for notification permission (required on Android 13+). */
export async function ensureNotificationPermission(): Promise<boolean> {
  try {
    if (await isPermissionGranted()) return true;
    return (await requestPermission()) === "granted";
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

export function onVaultChanged(
  handler: (change: VaultChange) => void,
): Promise<UnlistenFn> {
  return listen<VaultChange>(EVENTS.vaultChanged, (event) => handler(event.payload));
}

export function onReminderFired(
  handler: (reminder: Reminder) => void,
): Promise<UnlistenFn> {
  return listen<Reminder>(EVENTS.reminderFired, (event) => handler(event.payload));
}
