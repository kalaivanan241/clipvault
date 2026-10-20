//! Look & feel: colour tokens (dark/light + accent), fonts, vector icons and
//! a few small widgets (key caps, pills, buttons) shared by the UI.

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind,
    Vec2,
};
use std::sync::Arc;

// ---------- colours ----------

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub panel: Color32,
    pub panel_solid: Color32,
    pub elev: Color32,
    pub elev2: Color32,
    pub line: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub code: Color32,
    pub sel: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    /// Accent dark enough to read as text on the panel.
    pub accent_ink: Color32,
    pub blue: Color32,
    pub violet: Color32,
    pub pink: Color32,
    pub amber: Color32,
    pub green: Color32,
    pub red: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

fn white(a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(255, 255, 255, (a * 255.0).round() as u8)
}

fn black(a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(0, 0, 0, (a * 255.0).round() as u8)
}

/// `color` at `alpha` opacity (0–1).
pub fn alpha(color: Color32, alpha: f32) -> Color32 {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r, g, b, (a as f32 * alpha).round() as u8)
}

pub const DEFAULT_ACCENT: Color32 = rgb(0xc8f169);

pub fn parse_accent(s: &str) -> Color32 {
    crate::classify::parse_hex_color(s.trim())
        .map(|[r, g, b]| Color32::from_rgb(r, g, b))
        .unwrap_or(DEFAULT_ACCENT)
}

impl Palette {
    pub fn new(dark: bool, accent: Color32) -> Palette {
        if dark {
            Palette {
                dark,
                panel: rgb(0x14161b),
                panel_solid: rgb(0x1a1c22),
                elev: white(0.045),
                elev2: white(0.085),
                line: white(0.08),
                text: rgb(0xeef0f3),
                muted: rgb(0xa2a8b3),
                faint: rgb(0x80868f),
                code: black(0.28),
                sel: white(0.075),
                accent,
                accent_text: rgb(0x0e100b),
                accent_ink: accent,
                blue: rgb(0x7cb2ff),
                violet: rgb(0xb49cff),
                pink: rgb(0xff9ecb),
                amber: rgb(0xffc46b),
                green: rgb(0x8de08a),
                red: rgb(0xff8a80),
            }
        } else {
            let [r, g, b, _] = accent.to_array();
            let ink = |c: u8| (c as f32 * 0.42).round() as u8;
            Palette {
                dark,
                panel: rgb(0xfafafb),
                panel_solid: rgb(0xffffff),
                elev: black(0.035),
                elev2: black(0.065),
                line: black(0.09),
                text: rgb(0x15171c),
                muted: rgb(0x545a64),
                faint: rgb(0x646a74),
                code: black(0.03),
                sel: black(0.05),
                accent,
                accent_text: rgb(0x0e100b),
                accent_ink: Color32::from_rgb(ink(r), ink(g), ink(b)),
                blue: rgb(0x2f6fd6),
                violet: rgb(0x6a4fd8),
                pink: rgb(0xc23f86),
                amber: rgb(0x9a5c00),
                green: rgb(0x2f8a3a),
                red: rgb(0xc23a3a),
            }
        }
    }

    /// egui's own widgets (text field cursor, selection, scrollbars, menus).
    pub fn apply(&self, ctx: &egui::Context) {
        let mut v = if self.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        v.panel_fill = self.panel;
        v.window_fill = self.panel_solid;
        v.extreme_bg_color = self.code;
        v.faint_bg_color = self.elev;
        v.override_text_color = Some(self.text);
        v.selection.bg_fill = alpha(self.accent, 0.35);
        v.selection.stroke = Stroke::new(1.0, self.text);
        v.text_cursor.stroke = Stroke::new(2.0, self.accent);
        v.hyperlink_color = self.accent_ink;
        v.window_stroke = Stroke::new(1.0, self.line);
        v.window_corner_radius = CornerRadius::same(12);
        v.menu_corner_radius = CornerRadius::same(10);
        v.popup_shadow = egui::Shadow {
            offset: [0, 12],
            blur: 30,
            spread: 0,
            color: black(if self.dark { 0.35 } else { 0.15 }),
        };
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = CornerRadius::same(7);
            w.fg_stroke.color = self.text;
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.line);
        v.widgets.noninteractive.fg_stroke.color = self.text;
        v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        v.widgets.inactive.bg_fill = self.elev2;
        v.widgets.hovered.weak_bg_fill = self.elev2;
        v.widgets.hovered.bg_fill = self.elev2;
        v.widgets.hovered.bg_stroke = Stroke::NONE;
        v.widgets.active.weak_bg_fill = self.elev2;
        v.widgets.active.bg_fill = self.elev2;
        ctx.set_visuals(v);
        ctx.global_style_mut(|s| {
            s.spacing.scroll = egui::style::ScrollStyle::thin();
            s.spacing.item_spacing = Vec2::new(8.0, 4.0);
            s.spacing.button_padding = Vec2::new(10.0, 6.0);
            s.spacing.menu_margin = egui::Margin::same(6);
        });
    }
}

// ---------- fonts ----------

pub const MEDIUM: &str = "geist-medium";
pub const SEMIBOLD: &str = "geist-semibold";
pub const MONO_MEDIUM: &str = "geist-mono-medium";

pub fn sans(size: f32) -> FontId {
    FontId::proportional(size)
}
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}
pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}
pub fn mono_medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MONO_MEDIUM.into()))
}

/// Geist + Geist Mono (bundled, SIL OFL), with macOS system fonts as
/// fallbacks for symbols such as ⌘ ⇧ ↵ ⌫ and non-Latin scripts.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(name.into(), Arc::new(egui::FontData::from_static(bytes)));
    };
    add("geist", include_bytes!("../assets/fonts/Geist-Regular.ttf"));
    add(MEDIUM, include_bytes!("../assets/fonts/Geist-Medium.ttf"));
    add(
        SEMIBOLD,
        include_bytes!("../assets/fonts/Geist-SemiBold.ttf"),
    );
    add(
        "geist-mono",
        include_bytes!("../assets/fonts/GeistMono-Regular.ttf"),
    );
    add(
        MONO_MEDIUM,
        include_bytes!("../assets/fonts/GeistMono-Medium.ttf"),
    );

    let mut fallbacks = Vec::new();
    for (name, path) in [
        ("sf", "/System/Library/Fonts/SFNS.ttf"),
        ("apple-symbols", "/System/Library/Fonts/Apple Symbols.ttf"),
        ("helvetica-neue", "/System/Library/Fonts/HelveticaNeue.ttc"),
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert(name.into(), Arc::new(egui::FontData::from_owned(bytes)));
            fallbacks.push(name.to_string());
        }
    }
    // egui's built-in fonts (incl. emoji) last
    let builtin_sans = fonts.families[&FontFamily::Proportional].clone();
    let builtin_mono = fonts.families[&FontFamily::Monospace].clone();

    let family = |primary: &str, builtin: &[String]| -> Vec<String> {
        let mut v = vec![primary.to_string()];
        v.extend(fallbacks.iter().cloned());
        v.extend(builtin.iter().cloned());
        v
    };
    fonts
        .families
        .insert(FontFamily::Proportional, family("geist", &builtin_sans));
    fonts
        .families
        .insert(FontFamily::Monospace, family("geist-mono", &builtin_mono));
    for (name, builtin) in [
        (MEDIUM, &builtin_sans),
        (SEMIBOLD, &builtin_sans),
        (MONO_MEDIUM, &builtin_mono),
    ] {
        fonts
            .families
            .insert(FontFamily::Name(name.into()), family(name, builtin));
    }
    ctx.set_fonts(fonts);
}

// ---------- text helpers ----------

/// A single line of text, cut with "…" to fit `width`.
pub fn elided(
    ui: &egui::Ui,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.replace(['\n', '\r', '\t'], " "),
        egui::TextFormat::simple(font, color),
    );
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.painter().layout_job(job)
}

// ---------- icons (24×24 viewbox, like the design's SVGs) ----------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Search,
    Text,
    Code,
    Link,
    Image,
    Folder,
    Pin,
    External,
    Gear,
    Check,
    Clipboard,
    Alert,
}

pub fn icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32, width: f32) {
    let s = rect.width() / 24.0;
    let p = |x: f32, y: f32| Pos2::new(rect.left() + x * s, rect.top() + y * s);
    let stroke = Stroke::new(width * s.max(0.6), color);
    let line = |pts: &[(f32, f32)]| {
        painter.add(Shape::line(
            pts.iter().map(|&(x, y)| p(x, y)).collect(),
            stroke,
        ));
    };
    let closed = |pts: &[(f32, f32)]| {
        painter.add(Shape::closed_line(
            pts.iter().map(|&(x, y)| p(x, y)).collect(),
            stroke,
        ));
    };
    let r = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(p(x0, y0), p(x1, y1));
    match icon {
        Icon::Search => {
            painter.circle_stroke(p(11.0, 11.0), 7.0 * s, stroke);
            line(&[(20.0, 20.0), (16.5, 16.5)]);
        }
        Icon::Text => {
            line(&[(5.0, 6.0), (19.0, 6.0)]);
            line(&[(5.0, 12.0), (19.0, 12.0)]);
            line(&[(5.0, 18.0), (14.0, 18.0)]);
        }
        Icon::Code => {
            line(&[(8.0, 8.0), (4.0, 12.0), (8.0, 16.0)]);
            line(&[(16.0, 8.0), (20.0, 12.0), (16.0, 16.0)]);
            line(&[(13.5, 5.0), (10.5, 19.0)]);
        }
        Icon::Link => {
            // two chain links at 45°
            for (cx, cy) in [(9.0, 15.0), (15.0, 9.0)] {
                let pts = capsule(cx, cy, 4.6, 2.6);
                painter.add(Shape::closed_line(
                    pts.iter().map(|&(x, y)| p(x, y)).collect(),
                    stroke,
                ));
            }
        }
        Icon::Image => {
            painter.rect_stroke(r(3.0, 4.0, 21.0, 20.0), 3.0 * s, stroke, StrokeKind::Middle);
            painter.circle_stroke(p(9.0, 10.0), 1.6 * s, stroke);
            line(&[(21.0, 16.0), (16.0, 11.0), (7.0, 20.0)]);
        }
        Icon::Folder => closed(&[
            (3.0, 7.0),
            (4.0, 5.4),
            (5.5, 5.0),
            (9.0, 5.0),
            (11.0, 7.0),
            (18.5, 7.0),
            (20.3, 7.7),
            (21.0, 9.5),
            (21.0, 16.5),
            (20.3, 18.3),
            (18.5, 19.0),
            (5.5, 19.0),
            (3.7, 18.3),
            (3.0, 16.5),
        ]),
        Icon::Pin => {
            let fill = |pts: &[(f32, f32)]| {
                painter.add(Shape::convex_polygon(
                    pts.iter().map(|&(x, y)| p(x, y)).collect(),
                    color,
                    Stroke::NONE,
                ));
            };
            fill(&[(9.0, 3.0), (15.0, 3.0), (14.0, 10.5), (10.0, 10.5)]);
            fill(&[
                (10.0, 10.0),
                (14.0, 10.0),
                (18.0, 14.0),
                (18.0, 16.0),
                (6.0, 16.0),
                (6.0, 14.0),
            ]);
            line(&[(12.0, 16.0), (12.0, 21.5)]);
        }
        Icon::External => {
            line(&[(14.0, 4.0), (20.0, 4.0), (20.0, 10.0)]);
            line(&[(20.0, 4.0), (11.0, 13.0)]);
            line(&[
                (18.0, 14.0),
                (18.0, 19.0),
                (17.0, 20.0),
                (5.0, 20.0),
                (4.0, 19.0),
                (4.0, 7.0),
                (5.0, 6.0),
                (10.0, 6.0),
            ]);
        }
        Icon::Gear => {
            painter.circle_stroke(p(12.0, 12.0), 3.0 * s, stroke);
            painter.circle_stroke(p(12.0, 12.0), 7.0 * s, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let (sn, cs) = a.sin_cos();
                line(&[
                    (12.0 + cs * 7.0, 12.0 + sn * 7.0),
                    (12.0 + cs * 9.6, 12.0 + sn * 9.6),
                ]);
            }
        }
        Icon::Check => line(&[(5.0, 12.0), (10.0, 17.0), (19.0, 7.0)]),
        Icon::Clipboard => {
            painter.rect_stroke(r(6.0, 4.0, 18.0, 21.0), 2.0 * s, stroke, StrokeKind::Middle);
            line(&[(9.0, 4.0), (9.0, 3.0), (15.0, 3.0), (15.0, 4.0)]);
            line(&[(9.0, 10.0), (15.0, 10.0)]);
            line(&[(9.0, 14.0), (13.0, 14.0)]);
        }
        Icon::Alert => {
            line(&[(12.0, 6.0), (12.0, 14.0)]);
            painter.circle_filled(p(12.0, 18.5), 1.4 * s, color);
        }
    }
}

/// Outline of a pill centred at (cx, cy), long axis at -45°.
fn capsule(cx: f32, cy: f32, half_len: f32, radius: f32) -> Vec<(f32, f32)> {
    let (dx, dy) = (
        std::f32::consts::FRAC_1_SQRT_2,
        -std::f32::consts::FRAC_1_SQRT_2,
    );
    let mut pts = Vec::new();
    for (end, base) in [
        (1.0f32, -std::f32::consts::FRAC_PI_2),
        (-1.0, std::f32::consts::FRAC_PI_2),
    ] {
        let ex = cx + dx * (half_len - radius) * end;
        let ey = cy + dy * (half_len - radius) * end;
        let axis = dy.atan2(dx);
        for i in 0..=8 {
            let a = axis + base + i as f32 * std::f32::consts::PI / 8.0;
            pts.push((ex + a.cos() * radius, ey + a.sin() * radius));
        }
    }
    pts
}

// ---------- small widgets ----------

/// A key cap like `⌘P`, painted at `pos` (left-centre). Returns its width.
pub fn keycap_at(
    painter: &egui::Painter,
    pal: &Palette,
    pos: Pos2,
    label: &str,
    height: f32,
    filled: bool,
) -> f32 {
    let font = mono_medium(if height > 20.0 { 11.5 } else { 10.5 });
    let color = if filled { pal.accent_text } else { pal.muted };
    let galley = painter.layout_no_wrap(label.to_string(), font, color);
    let pad = if height > 20.0 { 7.0 } else { 5.0 };
    let w = (galley.size().x + pad * 2.0).max(height);
    let rect = Rect::from_min_size(Pos2::new(pos.x, pos.y - height / 2.0), Vec2::new(w, height));
    if filled {
        painter.rect_filled(rect, 5.0, pal.accent);
    } else {
        painter.rect(
            rect,
            5.0,
            pal.elev2,
            Stroke::new(1.0, pal.line),
            StrokeKind::Inside,
        );
    }
    painter.galley(rect.center() - galley.size() / 2.0, galley, color);
    w
}

/// Width a key cap would take.
pub fn keycap_width(ui: &egui::Ui, label: &str, height: f32) -> f32 {
    let font = mono_medium(if height > 20.0 { 11.5 } else { 10.5 });
    let g = ui
        .painter()
        .layout_no_wrap(label.to_string(), font, Color32::WHITE);
    let pad = if height > 20.0 { 7.0 } else { 5.0 };
    (g.size().x + pad * 2.0).max(height)
}

/// Inline row of key caps and a label, e.g. `[↑][↓] Navigate`.
pub fn hint(ui: &mut egui::Ui, pal: &Palette, keys: &[&str], label: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for k in keys {
            let w = keycap_width(ui, k, 18.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 18.0), Sense::hover());
            keycap_at(ui.painter(), pal, rect.left_center(), k, 18.0, false);
        }
        ui.add_space(2.0);
        ui.label(egui::RichText::new(label).font(sans(12.0)).color(pal.faint));
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonStyle {
    /// Transparent until hovered.
    Ghost,
    /// Subtle fill with a hairline border.
    Outline,
    /// Accent fill.
    Primary,
}

/// A design-system button: optional leading icon, label, optional trailing key cap.
pub fn button(
    ui: &mut egui::Ui,
    pal: &Palette,
    style: ButtonStyle,
    lead: Option<Icon>,
    label: &str,
    key: Option<&str>,
) -> egui::Response {
    let font = if style == ButtonStyle::Primary {
        semibold(13.0)
    } else {
        sans(12.5)
    };
    let text_color = match style {
        ButtonStyle::Ghost => pal.muted,
        ButtonStyle::Outline => pal.text,
        ButtonStyle::Primary => pal.accent_text,
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font, text_color);
    let key_w = key
        .map(|k| keycap_width(ui, k, 18.0).max(20.0))
        .unwrap_or(0.0);
    let pad = if style == ButtonStyle::Primary {
        14.0
    } else {
        10.0
    };
    let icon_w = if lead.is_some() { 13.0 + 8.0 } else { 0.0 };
    let left = pad + icon_w;
    let right = if key.is_some() { 7.0 } else { pad };
    let gap = if key.is_some() { 9.0 } else { 0.0 };
    let size = Vec2::new(left + galley.size().x + gap + key_w + right, 32.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    let hovered = resp.hovered();
    match style {
        ButtonStyle::Ghost => {
            if hovered {
                painter.rect_filled(rect, 8.0, pal.elev2);
            }
        }
        ButtonStyle::Outline => {
            let fill = if hovered { pal.elev2 } else { pal.elev };
            painter.rect(
                rect,
                8.0,
                fill,
                Stroke::new(1.0, pal.line),
                StrokeKind::Inside,
            );
        }
        ButtonStyle::Primary => {
            let fill = if hovered {
                pal.accent.gamma_multiply(1.06)
            } else {
                pal.accent
            };
            painter.rect_filled(rect, 8.0, fill);
        }
    }
    if let Some(i) = lead {
        let ir = Rect::from_center_size(
            Pos2::new(rect.left() + pad + 6.5, rect.center().y),
            Vec2::splat(13.0),
        );
        icon(painter, ir, i, text_color, 2.0);
    }
    let ty = rect.center().y - galley.size().y / 2.0;
    painter.galley(
        Pos2::new(rect.left() + left, ty),
        galley.clone(),
        text_color,
    );
    if let Some(k) = key {
        let x = rect.left() + left + galley.size().x + gap;
        if style == ButtonStyle::Primary {
            let kr =
                Rect::from_min_size(Pos2::new(x, rect.center().y - 10.0), Vec2::new(key_w, 20.0));
            painter.rect_filled(kr, 5.0, black(0.14));
            let g = painter.layout_no_wrap(k.to_string(), sans(11.0), pal.accent_text);
            painter.galley(kr.center() - g.size() / 2.0, g, pal.accent_text);
        } else {
            keycap_at(painter, pal, Pos2::new(x, rect.center().y), k, 18.0, false);
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Square icon-only button (e.g. Show in Finder, settings).
pub fn icon_button(
    ui: &mut egui::Ui,
    pal: &Palette,
    which: Icon,
    size: f32,
    outlined: bool,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let painter = ui.painter();
    if resp.hovered() {
        painter.rect_filled(rect, 7.0, pal.elev2);
    }
    if outlined {
        painter.rect_stroke(rect, 8.0, Stroke::new(1.0, pal.line), StrokeKind::Inside);
    }
    let ir = Rect::from_center_size(rect.center(), Vec2::splat(size * 0.5));
    icon(painter, ir, which, pal.muted, 2.0);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A small rounded tag such as `3 lines` (mono) or a status badge.
pub fn pill(ui: &mut egui::Ui, text: &str, font: FontId, fg: Color32, bg: Color32, height: f32) {
    let g = ui.painter().layout_no_wrap(text.to_string(), font, fg);
    let pad = if height >= 22.0 { 8.0 } else { 7.0 };
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(g.size().x + pad * 2.0, height), Sense::hover());
    ui.painter()
        .rect_filled(rect, if height >= 22.0 { 6.0 } else { 5.0 }, bg);
    ui.painter().galley(rect.center() - g.size() / 2.0, g, fg);
}
