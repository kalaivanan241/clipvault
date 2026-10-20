//! Menu-bar icon: a way back to the window (even if the shortcut is taken by
//! another app), pause/resume recording, and quit.

use crate::watcher::Shared;
use anyhow::{anyhow, Result};
use eframe::egui;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

pub struct Tray {
    _icon: TrayIcon,
    pause: CheckMenuItem,
}

impl Tray {
    /// Keep the menu's check mark in step with pausing from the window.
    /// Main thread only.
    pub fn sync_paused(&self, paused: bool) {
        if self.pause.is_checked() != paused {
            self.pause.set_checked(paused);
        }
    }
}

/// Must be called on the main thread after the app has launched (eframe's
/// creation closure is). Keep the returned value alive.
pub fn create(hotkey_label: &str, ctx: egui::Context, shared: Arc<Shared>) -> Result<Tray> {
    let show = MenuItem::new(format!("Show ClipVault   {hotkey_label}"), true, None);
    let pause = CheckMenuItem::new("Pause recording", true, false, None);
    let quit = MenuItem::new("Quit ClipVault", true, None);
    let menu = Menu::new();
    menu.append_items(&[&show, &pause, &PredefinedMenuItem::separator(), &quit])
        .map_err(|e| anyhow!("menu: {e}"))?;

    let tray = TrayIconBuilder::new()
        .with_icon_templated(icon()?)
        .with_tooltip("ClipVault")
        .with_menu(Box::new(menu))
        .build()
        .map_err(|e| anyhow!("menu bar icon: {e}"))?;

    let (show_id, pause_id, quit_id) = (show.id().clone(), pause.id().clone(), quit.id().clone());
    MenuEvent::set_event_handler(Some(move |ev: MenuEvent| {
        if ev.id == show_id {
            shared.show(&ctx);
        } else if ev.id == pause_id {
            // The check mark toggles itself; the UI re-syncs it each frame.
            shared.paused.fetch_xor(true, Ordering::Relaxed);
            ctx.request_repaint();
        } else if ev.id == quit_id {
            // Every write is already committed to SQLite, so exiting is safe.
            std::process::exit(0);
        }
    }));
    Ok(Tray { _icon: tray, pause })
}

/// An 18×18 clipboard glyph drawn in code (black + alpha, so macOS can tint
/// it as a template image for light/dark menu bars).
fn icon() -> Result<Icon> {
    const S: usize = 36; // drawn at 2x for Retina
    let mut px = vec![0u8; S * S * 4];
    let mut set = |x: usize, y: usize| {
        let i = (y * S + x) * 4;
        px[i + 3] = 255;
    };
    for y in 4..34 {
        for x in 6..30 {
            let edge = !(9..27).contains(&x) || !(7..31).contains(&y);
            // three "text lines" inside the board
            let line = (13..16).contains(&y) || (19..22).contains(&y) || (25..28).contains(&y);
            if edge || (line && (12..24).contains(&x)) {
                set(x, y);
            }
        }
    }
    // the clip at the top
    for y in 1..8 {
        for x in 12..24 {
            set(x, y);
        }
    }
    Icon::from_rgba(px, S as u32, S as u32).map_err(|e| anyhow!("icon: {e}"))
}
