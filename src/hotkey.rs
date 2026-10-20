//! Global shortcut (default ⌘⇧V) that toggles the history window.

use crate::watcher::Shared;
use anyhow::{anyhow, Result};
use eframe::egui;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::sync::Arc;

/// Must be called on the main thread (macOS requirement). Keep the returned
/// manager alive for as long as the shortcut should work.
pub fn register(
    spec: &str,
    ctx: egui::Context,
    shared: Arc<Shared>,
) -> Result<GlobalHotKeyManager> {
    let hotkey: HotKey = spec
        .parse()
        .map_err(|e| anyhow!("invalid hotkey {spec:?}: {e}"))?;
    let manager = GlobalHotKeyManager::new().map_err(|e| anyhow!("hotkey manager: {e}"))?;
    manager
        .register(hotkey)
        .map_err(|e| anyhow!("could not register {spec:?} (already used by another app?): {e}"))?;

    let id = hotkey.id();
    std::thread::Builder::new()
        .name("hotkey".into())
        .spawn(move || {
            let rx = GlobalHotKeyEvent::receiver();
            while let Ok(ev) = rx.recv() {
                if ev.id != id || ev.state != HotKeyState::Pressed {
                    continue;
                }
                shared.toggle(&ctx);
            }
        })?;
    Ok(manager)
}
