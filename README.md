# Zynky

Local-first, serverless credential, document and task manager built with
**Tauri v2 + Rust** on the backend and **Vite + React + TypeScript** on the
frontend. The target platform is a native Windows 11 executable; the code also
builds on Linux and macOS.

Your data is a folder of plain files. There is no server, no account and no
cloud. Put the folder under [Syncthing](https://syncthing.net/) and every device
gets the same vault, peer-to-peer.

```
ZynkyVault/
├── vault.json        # Argon2id salt + parameters + password verifier (no secrets)
├── reminders.md      # timestamped tasks watched by the background reminder engine
├── Passwords/        # one .md per credential; secrets encrypted in frontmatter
├── Notes/            # free-form markdown documents
├── Tasks/            # markdown checklists (- [ ] / - [x])
└── Media/            # attachments (images, PDFs, anything)
```

## Security model

| Layer | Implementation |
| --- | --- |
| Key derivation | Argon2id (`argon2` crate), 64 MiB / 3 passes / 1 lane, 16-byte random salt → 256-bit key |
| Encryption | AES-256-GCM (`aes-gcm` crate), fresh 96-bit nonce per blob, AAD-bound format tag |
| Storage | `MAGIC(4) ‖ NONCE(12) ‖ CIPHERTEXT+TAG` base64-encoded into the `secure_blob:` frontmatter key |
| Memory | Master key and decrypted secrets are wrapped in `zeroize` types and wiped on drop |

A credential file looks like this. Only the `secure_blob` value is ciphertext;
the body is ordinary Markdown you can read in any editor:

```markdown
---
title: GitHub
tags:
  - dev
created: 2026-10-10T09:12:44+02:00
updated: 2026-10-10T09:12:44+02:00
secure_blob: WllLMc5Q...base64...
---
Recovery codes are in the safe. 2FA is on the work phone.
```

The encrypted JSON inside the blob holds `username`, `password`, `url`,
`totpSecret` and a free-form `fields` map.

## Features

- **Passwords** – encrypted credentials with password generator, copy-to-clipboard,
  extra encrypted fields and plain-text notes. Changing the master password
  re-encrypts every credential and rewrites the manifest.
- **Notes** – TipTap rich-text editor that reads and writes Markdown
  bidirectionally (`@tiptap/markdown`). Edits are debounced and written
  atomically; checkbox toggles are written immediately.
- **Tasks** – `- [ ]` / `- [x]` checkboxes parsed by Rust. The checklist view
  rewrites the exact line on disk the moment a box is ticked.
- **Media** – import any file into the vault; images, video, audio and PDFs
  preview inline.
- **Reminders** – a native Rust thread wakes on every wall-clock minute, parses
  ISO 8601 timestamps in `reminders.md` (e.g. `- [ ] Stand-up @ 2026-10-10T09:30`)
  and fires a **Windows toast notification** through the WinRT
  `ToastNotificationManager` API (PowerShell, hidden window). On other platforms
  it falls back to the Tauri notification plugin.
- **Sync awareness** – a `notify` watcher on the vault folder forwards debounced
  `vault://changed` events to the UI, so files delivered by Syncthing show up
  without a restart. Syncthing's temp files and `.stfolder`/`.stversions` are
  ignored.
- **Atomic writes** – every file is written to a temp sibling, fsynced and
  renamed into place, so peers never observe half-written files.

## Project layout

```
src-tauri/src/
├── lib.rs          # Tauri builder, plugins, command registration
├── crypto.rs       # Argon2id + AES-256-GCM primitives (Result<T, CryptoError>)
├── frontmatter.rs  # YAML frontmatter parse / strip / render
├── tasks.rs        # Markdown checkbox parser + in-place toggling
├── vault.rs        # File broker commands: list, read, atomic write, credentials, media
├── watcher.rs      # Filesystem change tracking for Syncthing peers
├── reminder.rs     # 60-second background worker + Windows toast command
├── state.rs        # Shared AppState (vault root, master key, watcher)
└── error.rs        # Serialisable AppError for every command
src/
├── types.ts        # TypeScript mirrors of the Rust data contracts
├── api.ts          # Typed invoke() wrappers and event subscriptions
├── context/        # VaultProvider (unlock/lock/change events), ToastProvider
├── hooks/          # useDocument (load + debounced/immediate save), useFileList
└── components/     # UnlockScreen, Sidebar, Passwords/Notes/Tasks/Media/Reminders panes, MarkdownEditor
```

## Development

Prerequisites: Rust (stable), Node.js 22+, and the
[Tauri v2 prerequisites](https://tauri.app/start/prerequisites/) for your OS
(on Windows: Visual Studio Build Tools + WebView2 runtime).

```bash
npm install
npm run tauri dev        # hot-reloading desktop app
```

Verification commands used in CI / this repository:

```bash
npm run typecheck                                   # tsc --noEmit
npm run build                                       # tsc && vite build
cd src-tauri && cargo check --tests                 # backend type check
cd src-tauri && cargo test                          # 34 unit tests
cd src-tauri && cargo clippy --all-targets
cd src-tauri && cargo check --target x86_64-pc-windows-gnu   # Windows code paths
```

## Building the Windows installer

```bash
npm run tauri build
```

This produces NSIS and MSI installers under `src-tauri/target/release/bundle/`.
The app id is `com.zynky.app`. Toasts use the built-in PowerShell AUMID so they
work even when the app is run from a portable folder; after an MSI/NSIS
install Windows also accepts the app's own identity.

## Reminder file format

`reminders.md` is a normal task list. Any unchecked item containing an
ISO 8601 timestamp becomes a reminder:

```markdown
# Reminders

- [ ] Stand-up meeting @ 2026-10-10T09:30
- [ ] Renew domain 2026-11-01T08:00:00+02:00
- [ ] Backup check (2026-12-24 18:00Z)
- [x] Already done 2026-01-01T10:00
```

Timestamps without a zone are interpreted in the machine's local time. The
worker fires when the minute matches, de-duplicates within a minute and catches
up reminders missed while the machine slept (up to 24 hours back).
