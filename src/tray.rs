//! Menu-bar icon: a way back to the window (even if the shortcut is taken by
//! another app), pause/resume recording, and quit.
//!
//! The glyphs follow the "Menu bar icon" design: an 18 × 18 pt template image,
//! 1.5 pt strokes, one colour (macOS tints it for light and dark menu bars).

use crate::watcher::Shared;
use anyhow::{anyhow, Result};
use eframe::egui;
use std::cell::Cell;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// The three concepts from the design.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Glyph {
    /// A: two stacked clipboards.
    Stack,
    /// B: a clipboard with a vault dial.
    Vault,
    /// C: a clipboard with a clock arrow.
    History,
}

impl Glyph {
    /// From `config.json`'s `menu_bar_icon`; unknown names fall back to Vault.
    pub fn parse(s: &str) -> Glyph {
        match s.trim().to_lowercase().as_str() {
            "stack" | "a" => Glyph::Stack,
            "history" | "c" => Glyph::History,
            _ => Glyph::Vault,
        }
    }

    /// (path, solid) pairs in the design's 18 × 18 viewbox.
    fn shapes(self) -> Vec<(String, bool)> {
        let line = |d: &str| (d.to_string(), false);
        match self {
            Glyph::Stack => vec![
                line("M6.75 2.75h6.5a2 2 0 0 1 2 2v7.5"),
                (rounded_rect(2.75, 5.0, 10.0, 11.0, 2.0), false),
                (rounded_rect(5.25, 3.75, 5.0, 2.75, 1.1), true),
                line("M5.5 10h4.5M5.5 12.75h2.75"),
            ],
            Glyph::Vault => vec![
                (rounded_rect(2.5, 3.75, 13.0, 12.0, 3.0), false),
                (rounded_rect(6.25, 2.25, 5.5, 3.0, 1.2), true),
                (circle(9.0, 10.25, 2.75), false),
                (circle(9.0, 10.25, 0.9), true),
                line("M9 6.4v.6M9 13.5v.6M5.15 10.25h.6M12.25 10.25h.6"),
            ],
            Glyph::History => vec![
                (rounded_rect(3.0, 3.75, 12.0, 12.5, 2.5), false),
                (rounded_rect(6.25, 2.25, 5.5, 3.0, 1.2), true),
                line("M6.15 9.2A3.1 3.1 0 1 1 6.4 12.2"),
                line("M5.9 7.6v1.75h1.75"),
                line("M9 9v1.6l1.1.7"),
            ],
        }
    }

    fn svg(self) -> String {
        let mut paths = String::new();
        for (d, solid) in self.shapes() {
            let paint = if solid {
                r#"fill="black" stroke="none""#
            } else {
                r#"fill="none" stroke="black" stroke-width="1.5""#
            };
            paths.push_str(&format!(r#"<path d="{d}" {paint}/>"#));
        }
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 18 18" width="18" height="18" stroke-linecap="round" stroke-linejoin="round">{paths}</svg>"#
        )
    }
}

/// The design's `rr()` helper: a rounded rectangle as an SVG path.
fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> String {
    format!(
        "M{} {y}h{}a{r} {r} 0 0 1 {r} {r}v{}a{r} {r} 0 0 1 {} {r}h{}a{r} {r} 0 0 1 {} {}v{}a{r} {r} 0 0 1 {r} {}z",
        x + r,
        w - 2.0 * r,
        h - 2.0 * r,
        -r,
        -(w - 2.0 * r),
        -r,
        -r,
        -(h - 2.0 * r),
        -r,
    )
}

/// The design's `circle()` helper.
fn circle(cx: f32, cy: f32, r: f32) -> String {
    format!(
        "M{} {cy}a{r} {r} 0 1 0 {} 0a{r} {r} 0 1 0 {} 0z",
        cx - r,
        2.0 * r,
        -2.0 * r
    )
}

/// Render a glyph at 2× (36 px) for Retina. `opacity` < 1 greys it out:
/// template images keep their alpha, so a faded glyph reads as "inactive".
fn render(glyph: Glyph, opacity: f32) -> Result<Icon> {
    use resvg::{tiny_skia, usvg};
    const SCALE: f32 = 2.0;
    const PX: u32 = 36;
    let tree = usvg::Tree::from_str(&glyph.svg(), &usvg::Options::default())
        .map_err(|e| anyhow!("icon svg: {e}"))?;
    let mut pixmap = tiny_skia::Pixmap::new(PX, PX).ok_or_else(|| anyhow!("icon pixmap"))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(SCALE, SCALE),
        &mut pixmap.as_mut(),
    );
    // Black only, so premultiplied and straight alpha are the same apart from alpha.
    let mut rgba = pixmap.take();
    for px in rgba.as_chunks_mut::<4>().0 {
        px[3] = (px[3] as f32 * opacity).round() as u8;
    }
    Icon::from_rgba(rgba, PX, PX).map_err(|e| anyhow!("icon: {e}"))
}

pub struct Tray {
    icon: TrayIcon,
    pause: CheckMenuItem,
    glyph: Glyph,
    shown_paused: Cell<bool>,
}

impl Tray {
    /// Keep the menu's check mark and the icon in step with pausing (from
    /// the window or the menu). Main thread only.
    pub fn sync_paused(&self, paused: bool) {
        if self.pause.is_checked() != paused {
            self.pause.set_checked(paused);
        }
        if self.shown_paused.get() != paused {
            self.shown_paused.set(paused);
            let opacity = if paused { 0.4 } else { 1.0 };
            if let Ok(icon) = render(self.glyph, opacity) {
                let _ = self.icon.set_icon_templated(Some(icon));
            }
            let tip = if paused {
                "ClipVault (recording paused)"
            } else {
                "ClipVault"
            };
            let _ = self.icon.set_tooltip(Some(tip));
        }
    }
}

/// Must be called on the main thread after the app has launched (eframe's
/// creation closure is). Keep the returned value alive.
pub fn create(
    glyph: Glyph,
    hotkey_label: &str,
    ctx: egui::Context,
    shared: Arc<Shared>,
) -> Result<Tray> {
    let show = MenuItem::new(format!("Show ClipVault   {hotkey_label}"), true, None);
    let pause = CheckMenuItem::new("Pause recording", true, false, None);
    let quit = MenuItem::new("Quit ClipVault", true, None);
    let menu = Menu::new();
    menu.append_items(&[&show, &pause, &PredefinedMenuItem::separator(), &quit])
        .map_err(|e| anyhow!("menu: {e}"))?;

    let icon = TrayIconBuilder::new()
        .with_icon_templated(render(glyph, 1.0)?)
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
    Ok(Tray {
        icon,
        pause,
        glyph,
        shown_paused: Cell::new(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_match_the_design() {
        // rr(2.5, 3.75, 13, 12, 3) from the design's script
        assert_eq!(
            rounded_rect(2.5, 3.75, 13.0, 12.0, 3.0),
            "M5.5 3.75h7a3 3 0 0 1 3 3v6a3 3 0 0 1 -3 3h-7a3 3 0 0 1 -3 -3v-6a3 3 0 0 1 3 -3z"
        );
        assert_eq!(
            circle(9.0, 10.25, 0.9),
            "M8.1 10.25a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0z"
        );
    }

    #[test]
    fn every_glyph_renders() {
        for g in [Glyph::Stack, Glyph::Vault, Glyph::History] {
            assert!(render(g, 1.0).is_ok(), "{g:?}");
        }
        assert_eq!(Glyph::parse("Stack"), Glyph::Stack);
        assert_eq!(Glyph::parse("c"), Glyph::History);
        assert_eq!(Glyph::parse("nonsense"), Glyph::Vault);
    }
}
