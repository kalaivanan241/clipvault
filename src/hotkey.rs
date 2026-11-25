//! Global shortcut (default ⌘⇧V) that toggles the history window.

use crate::watcher::Shared;
use anyhow::{anyhow, Result};
use eframe::egui;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// Owns the system registration; the shortcut can be swapped at runtime
pub struct Hotkey {
    manager: GlobalHotKeyManager,
    current: Option<HotKey>,
    shared: Arc<Shared>,
}

pub fn parse(spec: &str) -> Result<HotKey> {
    spec.trim()
        .parse()
        .map_err(|e| anyhow!("invalid shortcut {spec:?}: {e}"))
}

impl Hotkey {
    /// Must be called on the main thread (macOS requirement). Keep the
    /// returned value alive for as long as the shortcut should work
    pub fn new(ctx: egui::Context, shared: Arc<Shared>) -> Result<Hotkey> {
        let manager = GlobalHotKeyManager::new().map_err(|e| anyhow!("hotkey manager: {e}"))?;
        let listener = shared.clone();
        std::thread::Builder::new()
            .name("hotkey".into())
            .spawn(move || {
                let rx = GlobalHotKeyEvent::receiver();
                while let Ok(ev) = rx.recv() {
                    let id = listener.hotkey_id.load(Ordering::Relaxed);
                    if ev.id != id || ev.state != HotKeyState::Pressed {
                        continue;
                    }
                    listener.toggle(&ctx);
                }
            })?;
        Ok(Hotkey {
            manager,
            current: None,
            shared,
        })
    }

    /// Replace the shortcut; on failure the previous one stays active
    pub fn set(&mut self, spec: &str) -> Result<()> {
        let hotkey = parse(spec)?;
        if self.current == Some(hotkey) {
            return Ok(());
        }
        if let Some(old) = self.current {
            let _ = self.manager.unregister(old);
        }
        if let Err(e) = self.manager.register(hotkey) {
            if let Some(old) = self.current {
                let _ = self.manager.register(old);
            }
            return Err(anyhow!(
                "could not register {spec:?} (already used by another app?): {e}"
            ));
        }
        self.current = Some(hotkey);
        self.shared.hotkey_id.store(hotkey.id(), Ordering::Relaxed);
        Ok(())
    }
}
