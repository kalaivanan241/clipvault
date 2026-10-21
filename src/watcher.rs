//! Background thread that polls the system clipboard and records changes.
//! macOS has no clipboard-change notification, so (like every clipboard
//! manager) we poll. On macOS each poll only reads the pasteboard's change
//! counter; the clipboard contents are read only when it moves.

use crate::config::Config;
use crate::platform;
use crate::store::{now_ms, Store};
use anyhow::{anyhow, Result};
use eframe::egui;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// State shared between the UI thread, watcher thread, hotkey and menu bar.
pub struct Shared {
    /// Bumped whenever the history changed (UI reloads its list).
    pub version: AtomicU64,
    /// Changes seen on the clipboard before this time (ms) were made by us
    /// (we just put a clip back) and are not re-recorded as new items.
    pub skip_until_ms: AtomicI64,
    /// Pasteboard change count right after our own last write (macOS).
    pub own_change: AtomicI64,
    /// When set, nothing new is recorded.
    pub paused: AtomicBool,
    /// Is the history window currently shown?
    pub visible: AtomicBool,
    /// Set when the window was just shown: UI clears search, focuses it.
    pub just_shown: AtomicBool,
    /// Set by the hotkey/menu bar when the window should be hidden.
    pub hide_requested: AtomicBool,
}

impl Shared {
    pub fn new() -> Arc<Shared> {
        Arc::new(Shared {
            version: AtomicU64::new(1),
            skip_until_ms: AtomicI64::new(0),
            own_change: AtomicI64::new(-1),
            paused: AtomicBool::new(false),
            visible: AtomicBool::new(true),
            just_shown: AtomicBool::new(true),
            hide_requested: AtomicBool::new(false),
        })
    }

    /// Bring the window up. Safe to call from any thread.
    pub fn show(&self, ctx: &egui::Context) {
        if !self.visible.swap(true, Ordering::Relaxed) {
            self.just_shown.store(true, Ordering::Relaxed);
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.request_repaint();
    }

    /// Show the window, or hide it if it is up. Safe to call from any thread.
    pub fn toggle(&self, ctx: &egui::Context) {
        if self.visible.load(Ordering::Relaxed) {
            // Hiding must happen on the UI thread (needs the main thread on macOS).
            self.hide_requested.store(true, Ordering::Relaxed);
            ctx.request_repaint();
        } else {
            self.show(ctx);
        }
    }
}

enum Snap {
    Files(Vec<PathBuf>),
    Text(String),
    Image(arboard::ImageData<'static>),
    Empty,
}

fn read(cb: &mut arboard::Clipboard) -> Snap {
    if let Ok(files) = cb.get().file_list() {
        if !files.is_empty() {
            return Snap::Files(files);
        }
    }
    if let Ok(t) = cb.get().text() {
        if !t.is_empty() {
            return Snap::Text(t);
        }
    }
    if let Ok(img) = cb.get().image() {
        return Snap::Image(img);
    }
    Snap::Empty
}

/// Cheap change-detection fingerprint (images are sampled, not fully hashed).
/// Only used where there is no pasteboard change counter.
fn signature(s: &Snap) -> String {
    let mut h = Sha256::new();
    match s {
        Snap::Files(f) => {
            h.update(b"f");
            for p in f {
                h.update(p.to_string_lossy().as_bytes());
                h.update([0]);
            }
        }
        Snap::Text(t) => {
            h.update(b"t");
            h.update(t.as_bytes());
        }
        Snap::Image(i) => {
            h.update(b"i");
            h.update((i.width as u64).to_le_bytes());
            h.update((i.height as u64).to_le_bytes());
            let b = &i.bytes;
            let step = (b.len() / 65536).max(1);
            let mut idx = 0;
            while idx < b.len() {
                h.update([b[idx]]);
                idx += step;
            }
        }
        Snap::Empty => h.update(b"e"),
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn spawn(cfg: Config, dir: PathBuf, ctx: egui::Context, shared: Arc<Shared>) {
    std::thread::Builder::new()
        .name("clipboard-watcher".into())
        .spawn(move || {
            if let Err(e) = run(cfg, dir, ctx, shared) {
                eprintln!("clipboard watcher stopped: {e:#}");
            }
        })
        .expect("failed to start clipboard watcher");
}

/// Background thread that reads the text in image clips (new ones as they
/// arrive, older ones once) with on-device text recognition.
pub fn spawn_ocr(cfg: Config, dir: PathBuf, ctx: egui::Context, shared: Arc<Shared>) {
    std::thread::Builder::new()
        .name("image-text".into())
        .spawn(move || {
            if let Err(e) = run_ocr(cfg, dir, ctx, shared) {
                eprintln!("image text recognition stopped: {e:#}");
            }
        })
        .expect("failed to start image text recognition");
}

fn run_ocr(cfg: Config, dir: PathBuf, ctx: egui::Context, shared: Arc<Shared>) -> Result<()> {
    let store = Store::open(&dir, cfg.max_items, cfg.max_copy_mb)?;
    loop {
        let pending = store.pending_ocr(8)?;
        if pending.is_empty() {
            std::thread::sleep(Duration::from_secs(2));
            continue;
        }
        for clip in pending {
            // Images that can't be read are marked done (empty) so they aren't retried forever.
            let text = platform::recognize_text(&store.image_path(&clip)).unwrap_or_default();
            store.set_ocr(clip.id, text.trim())?;
            shared.version.fetch_add(1, Ordering::Relaxed);
            ctx.request_repaint();
        }
    }
}

fn run(cfg: Config, dir: PathBuf, ctx: egui::Context, shared: Arc<Shared>) -> Result<()> {
    let store = Store::open(&dir, cfg.max_items, cfg.max_copy_mb)?;
    let mut cb = arboard::Clipboard::new().map_err(|e| anyhow!("clipboard unavailable: {e}"))?;
    // Starts unset so whatever is on the clipboard at launch is recorded too.
    let mut last_count: Option<i64> = None;
    let mut last_sig = String::new();
    let mut last_was_image = false;

    loop {
        // Without a change counter images are expensive to re-read, so poll
        // more slowly while one is on the clipboard.
        let wait = if last_was_image {
            cfg.poll_ms.max(1000)
        } else {
            cfg.poll_ms
        };
        std::thread::sleep(Duration::from_millis(wait));

        let snap = match platform::change_count() {
            Some(count) => {
                if Some(count) == last_count {
                    continue;
                }
                last_count = Some(count);
                if count == shared.own_change.load(Ordering::Relaxed) {
                    continue; // our own write
                }
                if shared.paused.load(Ordering::Relaxed) {
                    continue;
                }
                read(&mut cb)
            }
            None => {
                let snap = read(&mut cb);
                last_was_image = matches!(snap, Snap::Image(_));
                let sig = signature(&snap);
                if sig == last_sig {
                    continue;
                }
                last_sig = sig;
                if shared.paused.load(Ordering::Relaxed) {
                    continue;
                }
                snap
            }
        };

        if matches!(snap, Snap::Empty) {
            continue;
        }
        if now_ms() < shared.skip_until_ms.load(Ordering::Relaxed) {
            continue; // our own write
        }
        if platform::is_sensitive() {
            continue; // password-manager style clip
        }
        let source = platform::frontmost_app().unwrap_or_default();
        if cfg.is_ignored(&source) {
            continue;
        }

        let res = match &snap {
            Snap::Files(f) => store.add_files(f),
            Snap::Text(t) => store.add_text(t),
            Snap::Image(i) => store.add_image(i.width as u32, i.height as u32, &i.bytes),
            Snap::Empty => Ok(None),
        };
        match res {
            Ok(Some(id)) => {
                if !source.name.is_empty() {
                    let _ = store.set_source(id, &source.name);
                }
                shared.version.fetch_add(1, Ordering::Relaxed);
                ctx.request_repaint();
            }
            Ok(None) => {}
            Err(e) => eprintln!("could not store clip: {e:#}"),
        }
    }
}
