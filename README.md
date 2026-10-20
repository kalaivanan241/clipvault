# ClipVault

**A fast, private clipboard history manager for macOS, written in Rust.**

ClipVault quietly remembers everything you copy (text, images, files and folders),
and brings it back with one shortcut. Press **⌘⇧V**, type a few letters, hit **Enter**,
and the clip is pasted straight into the app you were using.

Everything stays on your Mac. No accounts, no network, no telemetry.

---

## Contents

- [Features](#features)
- [Requirements](#requirements)
- [Install](#install)
- [First run & permissions](#first-run--permissions)
- [Using ClipVault](#using-clipvault)
- [Configuration](#configuration)
- [Privacy & data](#privacy--data)
- [How it works](#how-it-works)
- [Development](#development)
- [Troubleshooting](#troubleshooting)
- [Known limitations](#known-limitations)
- [Uninstall](#uninstall)

---

## Features

| | |
|---|---|
| 📋 **Everything you copy** | Text, images (screenshots included), and files/folders copied in Finder |
| ⚡ **Global shortcut** | **⌘⇧V** opens the history from any app (configurable) |
| ↩️ **Auto-paste** | **Enter** pastes the clip into the app you came from; **⇧Enter** just copies it |
| 🔎 **Instant search** | Type words in any order; matches the clip's text, file names/paths, and the app it came from |
| 🗂 **Filters** | All · Text · Images · Files · Pinned (cycle with **Tab**) |
| 📌 **Pinning** | Pinned clips stay on top and are never auto-deleted |
| 🧳 **Files survive** | Copied files/folders are duplicated into the vault (up to a size limit), so you can paste them even after the originals are moved or deleted |
| 🏷 **Source app** | Each clip shows which app it was copied from |
| 🔐 **Privacy aware** | Skips password-manager clips and apps you choose to ignore; recording can be paused |
| 🍎 **Menu-bar app** | No Dock icon; a menu-bar icon offers Show, Pause recording and Quit |
| 🪶 **Lightweight** | Native Rust + egui; polling reads a single counter, so it's practically idle |

## Requirements

- macOS 11 (Big Sur) or later, Apple Silicon or Intel
- [Rust](https://rustup.rs) (recent stable) to build from source

## Install

```sh
git clone <this repo> clipvault
cd clipvault

./scripts/bundle-macos.sh      # builds ClipVault.app (release build, ad-hoc signed)
open ClipVault.app
```

To keep it around:

1. Drag `ClipVault.app` into `/Applications`.
2. Open **System Settings → General → Login Items** and add ClipVault so it starts at login.

> Prefer a Dock icon? Build with `SHOW_DOCK=1 ./scripts/bundle-macos.sh`.

For a quick try without bundling:

```sh
cargo run --release
```

## First run & permissions

macOS may ask for two permissions:

| Prompt | Why | What to choose |
|---|---|---|
| *"ClipVault would like to paste from other apps"* / clipboard access | Needed to read what you copy | **Allow** / **Always Allow** |
| **Accessibility** (System Settings → Privacy & Security → Accessibility) | Needed only for auto-paste (sending ⌘V to the previous app) | Turn **ClipVault** on |

The Accessibility prompt appears the first time you press Enter on a clip. Until it's
granted, Enter only copies the clip and the bottom bar shows an **Enable auto-paste** button.

> **Rebuilt the app?** macOS ties Accessibility permission to the app's signature, and the
> bundle script signs ad hoc. After a rebuild, switch ClipVault off and on again in the
> Accessibility list. When you use `cargo run`, the permission belongs to your terminal app instead.

## Using ClipVault

### Keyboard

| Key | Action |
|---|---|
| **⌘⇧V** | Show / hide the history (works from any app) |
| *type* | Search |
| **↑ / ↓** | Move the selection |
| **Enter** / double-click | Paste the clip into the app you were using |
| **⇧Enter** | Copy the clip to the clipboard only |
| **⌘1 … ⌘9** | Paste the 1st–9th clip directly |
| **⌘P** | Pin / unpin |
| **⌘⌫** | Delete the clip |
| **Tab** | Cycle filters |
| **Esc** | Clear the search, then close the window |

### Window

- **List** (left): newest first, pinned on top. Each row shows a preview, type, size, age and source app.
- **Preview** (right): full text, image preview, or the list of files with their status
  (*saved in ClipVault*, *linked to original*, or *no longer available*) and **Show in Finder**.
- **Bottom bar**: shortcut hints, item count, **Pause/Resume**, **Clear unpinned**, **Quit**.

The window's red close button only hides it. ClipVault keeps recording in the background.

### Menu bar

Click the clipboard icon in the menu bar for:

- **Show ClipVault**: opens the window (handy if another app took the shortcut)
- **Pause recording**: new copies aren't saved until you uncheck it
- **Quit ClipVault**

## Configuration

Settings live in `~/Library/Application Support/ClipVault/config.json`. It's created on
first run; edit it and restart ClipVault to apply changes. Options you leave out use the
defaults.

```json
{
  "hotkey": "cmd+shift+v",
  "max_items": 1000,
  "max_copy_mb": 100,
  "poll_ms": 400,
  "auto_paste": true,
  "ignore_apps": [
    "com.1password.1password",
    "com.agilebits.onepassword7",
    "com.bitwarden.desktop",
    "com.lastpass.LastPass",
    "org.keepassxc.keepassxc",
    "com.apple.keychainaccess",
    "com.apple.Passwords"
  ]
}
```

| Option | Default | Description |
|---|---|---|
| `hotkey` | `cmd+shift+v` | Global shortcut. Modifiers (`cmd`, `shift`, `alt`, `ctrl`) and a key joined by `+`, e.g. `ctrl+alt+KeyC`, `cmd+shift+Digit9` |
| `max_items` | `1000` | Unpinned clips beyond this count are deleted, oldest first (minimum 10) |
| `max_copy_mb` | `100` | Copied files/folders up to this total size are duplicated into the vault; bigger ones are remembered by path only |
| `poll_ms` | `400` | How often the clipboard is checked, in milliseconds (100–5000) |
| `auto_paste` | `true` | Enter pastes into the previous app. Set `false` to only copy |
| `ignore_apps` | password managers | Bundle ids or app names. Copies made while one of these apps is in front are never recorded |

Find an app's bundle id with:

```sh
osascript -e 'id of app "Notes"'
```

## Privacy & data

- **Local only.** ClipVault makes no network connections.
- **Password managers are skipped.** Clips marked *concealed*, *transient* or *auto-generated*
  (the [nspasteboard.org](http://nspasteboard.org) convention used by 1Password, Bitwarden and
  others) are never recorded, nor is anything copied while an app in `ignore_apps` is in front.
- **Pause anytime** from the window or the menu bar.
- **Your data, plain files.** Everything is stored under
  `~/Library/Application Support/ClipVault/`:

```
ClipVault/
├── config.json          # settings
├── history.db           # SQLite: clip metadata, text, search index
├── images/              # <hash>.png + <hash>_thumb.png for each image clip
└── files/               # <hash>/<n>/<name> — vault copies of copied files/folders
```

Deleting a clip (or **Clear unpinned**) also deletes its image and vault files.

## How it works

```
 ┌──────────────┐  change count  ┌──────────────┐   insert/bump   ┌──────────────┐
 │ macOS        │ ─────────────▶ │ watcher      │ ──────────────▶ │ store        │
 │ pasteboard   │   (polled)     │ thread       │                 │ SQLite+vault │
 └──────────────┘                └──────────────┘                 └──────┬───────┘
        ▲                                                                │ list/search
        │ copy back + ⌘V                                                 ▼
 ┌──────┴───────┐   show/hide    ┌──────────────┐                 ┌──────────────┐
 │ previous app │ ◀───────────── │ hotkey /     │ ──────────────▶ │ egui window  │
 └──────────────┘                │ menu bar     │                 │ (app.rs)     │
                                 └──────────────┘                 └──────────────┘
```

- **Change detection:** macOS has no clipboard-changed notification, so ClipVault polls the
  pasteboard's *change count*, a single integer. Contents are read only when it changes, which
  catches every copy, including copying the same thing twice (that moves it back to the top).
- **De-duplication:** each clip is hashed; copying something already in the history bumps it to
  the top instead of adding a duplicate.
- **Auto-paste:** after you pick a clip, ClipVault puts it on the clipboard, hides itself so macOS
  returns focus to the previous app, then posts a ⌘V key event.

### Project layout

| File | Responsibility |
|---|---|
| `src/main.rs` | Window setup and startup |
| `src/app.rs` | The egui UI: list, preview, keyboard handling, copy/paste back |
| `src/store.rs` | SQLite history, image/file vault, search, pruning (+ tests) |
| `src/watcher.rs` | Background clipboard polling and recording; shared state |
| `src/hotkey.rs` | Global shortcut registration |
| `src/tray.rs` | Menu-bar icon and menu |
| `src/platform.rs` | macOS specifics: pasteboard, frontmost app, Accessibility, key events |
| `src/config.rs` | `config.json` loading and defaults |
| `scripts/bundle-macos.sh` | Builds and ad-hoc signs `ClipVault.app` |

## Development

```sh
cargo run              # debug build
cargo test             # storage tests (dedupe, search, pinning, pruning, vault, migrations)
cargo clippy --all-targets
cargo fmt
```

Run the binary from a terminal to see its log output:

```sh
./ClipVault.app/Contents/MacOS/ClipVault
```

Built with [egui/eframe](https://github.com/emilk/egui), [rusqlite](https://github.com/rusqlite/rusqlite),
[arboard](https://github.com/1Password/arboard), [global-hotkey](https://github.com/tauri-apps/global-hotkey),
[tray-icon](https://github.com/tauri-apps/tray-icon) and [objc2](https://github.com/madsmtm/objc2).

## Troubleshooting

**The shortcut does nothing.**
Another app may already use ⌘⇧V (some editors use it for *Paste and Match Style*). Run the
binary from a terminal to see the error, then pick another `hotkey` in `config.json`. You can
always open the window from the menu-bar icon.

**Nothing is being recorded.**
Check that recording isn't paused (menu-bar icon or bottom bar), that you allowed clipboard
access, and that the app you copied from isn't in `ignore_apps`.

**Enter copies but doesn't paste.**
Grant Accessibility permission (see [First run](#first-run--permissions)). If you rebuilt the
app, toggle ClipVault off and on in the Accessibility list.

**Start fresh.**
Quit ClipVault, then delete `~/Library/Application Support/ClipVault/`.

## Known limitations

- **The window may not appear when launched as `ClipVault.app`.** This is being
  investigated; the app keeps running and recording in the background.
- **Keyboard layouts:** auto-paste sends the key in the ⌘V position of a US (ANSI) layout. With
  Dvorak and similar layouts, use ⇧Enter and paste yourself.
- **Very large text** (over 5 MB) is not recorded, to keep the history fast.
- **Huge files/folders** (over `max_copy_mb`) are linked, not copied, so they stop working if
  the originals are moved or deleted.
- **macOS only** for now. The code keeps platform specifics in `platform.rs` and `tray.rs`.

## Uninstall

1. Quit ClipVault from the menu-bar icon.
2. Delete `ClipVault.app`, and remove it from Login Items and the Accessibility list.
3. Delete your history: `rm -rf ~/Library/Application\ Support/ClipVault`
