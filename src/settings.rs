//! The Settings window (gear icon): quick actions that apply at once, plus every
//! option in `config.json`, handed back to the app to save and apply

use crate::config::Config;
use crate::hotkey;
use crate::theme::{self, ButtonStyle, Palette};
use eframe::egui::{self, Color32, RichText, Sense, Stroke, Vec2};

const WIDTH: f32 = 520.0;
const LABEL_W: f32 = 150.0;
const ACCENTS: [&str; 4] = ["#c8f169", "#8b7cff", "#5ee6d0", "#ff9a62"];
const THEMES: [(&str, &str); 3] = [("system", "System"), ("dark", "Dark"), ("light", "Light")];
const GLYPHS: [(&str, &str); 3] = [
    ("vault", "Vault"),
    ("stack", "Stack"),
    ("history", "History"),
];

pub enum Outcome {
    Open,
    Cancel,
    Save(Saved),
    RevealFile,
    SetPaused(bool),
    EnableAutoPaste,
    ClearHistory,
}

/// Live app state the quick actions show
pub struct Status {
    pub paused: bool,
    pub ax_trusted: bool,
    pub unpinned: usize,
}

pub struct Saved {
    pub config: Config,
    pub open_at_login: bool,
}

/// What the user is editing; only applied on Save
pub struct Draft {
    cfg: Config,
    ignore_text: String,
    /// Lives in the system's login items, not in `config.json`
    open_at_login: bool,
    recording: bool,
    confirm_clear: bool,
    pub error: Option<String>,
}

impl Draft {
    pub fn new(cfg: &Config, open_at_login: bool) -> Draft {
        Draft {
            cfg: cfg.clone(),
            ignore_text: cfg.ignore_apps.join("\n"),
            open_at_login,
            recording: false,
            confirm_clear: false,
            error: None,
        }
    }

    fn to_saved(&self) -> Result<Saved, String> {
        Ok(Saved {
            config: self.to_config()?,
            open_at_login: self.open_at_login,
        })
    }

    fn to_config(&self) -> Result<Config, String> {
        hotkey::parse(&self.cfg.hotkey).map_err(|e| e.to_string())?;
        let ignore_apps = self
            .ignore_text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect();
        Ok(Config {
            hotkey: self.cfg.hotkey.trim().to_string(),
            ignore_apps,
            ..self.cfg.clone()
        }
        .sanitized())
    }
}

pub fn show(ctx: &egui::Context, pal: &Palette, draft: &mut Draft, status: &Status) -> Outcome {
    let max_h = (ctx.content_rect().height() - 48.0).max(200.0);
    let frame = egui::Frame::NONE
        .fill(pal.panel_solid)
        .stroke(Stroke::new(1.0, pal.line))
        .corner_radius(12.0)
        .inner_margin(20.0);
    let modal = egui::Modal::new(egui::Id::unique("settings"))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(WIDTH);
            ui.style_mut().override_font_id = Some(theme::sans(13.0));
            ui.label(
                RichText::new("Settings")
                    .font(theme::semibold(17.0))
                    .color(pal.text),
            );
            ui.add_space(8.0);
            let action = egui::ScrollArea::vertical()
                .max_height(max_h - 120.0)
                .auto_shrink([false, true])
                .show(ui, |ui| body(ui, pal, draft, status))
                .inner;
            ui.add_space(10.0);
            match footer(ui, pal, draft) {
                Outcome::Open => action,
                outcome => outcome,
            }
        });
    // Esc while recording is consumed by the recorder, so it doesn't close the window
    let close = modal.should_close();
    match modal.inner {
        Outcome::Open if close => Outcome::Cancel,
        outcome => outcome,
    }
}

fn body(ui: &mut egui::Ui, pal: &Palette, draft: &mut Draft, status: &Status) -> Outcome {
    // Give number fields and buttons a fill so they read as editable
    let w = &mut ui.visuals_mut().widgets;
    w.inactive.weak_bg_fill = pal.elev2;
    w.inactive.bg_fill = pal.elev2;
    let action = actions(ui, pal, draft, status);
    section(ui, pal, "General");
    row(ui, pal, "Shortcut", |ui| shortcut_ui(ui, pal, draft));
    row(ui, pal, "Open at login", |ui| {
        ui.checkbox(&mut draft.open_at_login, "Start ClipVault when you log in");
    });
    row(ui, pal, "Auto-paste", |ui| {
        ui.checkbox(
            &mut draft.cfg.auto_paste,
            "Press ⌘V in your app after picking a clip",
        );
    });
    row(ui, pal, "Text in images", |ui| {
        ui.checkbox(
            &mut draft.cfg.ocr,
            "Make screenshots searchable (on-device)",
        );
    });

    section(ui, pal, "Appearance");
    row(ui, pal, "Theme", |ui| {
        choice(ui, &mut draft.cfg.theme, &THEMES);
    });
    row(ui, pal, "Highlight colour", |ui| accent_ui(ui, draft));
    row(ui, pal, "Menu bar icon", |ui| {
        choice(ui, &mut draft.cfg.menu_bar_icon, &GLYPHS);
    });

    section(ui, pal, "History");
    row(ui, pal, "Keep up to", |ui| {
        ui.add(
            egui::DragValue::new(&mut draft.cfg.max_items)
                .range(10..=100_000)
                .suffix(" clips"),
        );
    });
    row(ui, pal, "Copy files up to", |ui| {
        ui.add(
            egui::DragValue::new(&mut draft.cfg.max_copy_mb)
                .range(0..=100_000)
                .suffix(" MB"),
        );
    });
    row(ui, pal, "Check clipboard every", |ui| {
        ui.add(
            egui::DragValue::new(&mut draft.cfg.poll_ms)
                .range(100..=5000)
                .speed(10)
                .suffix(" ms"),
        );
    });

    section(ui, pal, "Privacy");
    row(ui, pal, "Ignored apps", |ui| {
        ui.vertical(|ui| {
            ui.add(
                egui::TextEdit::multiline(&mut draft.ignore_text)
                    .font(theme::mono(12.0))
                    .desired_rows(5)
                    .desired_width(f32::INFINITY),
            );
            note(ui, pal, "One bundle id or app name per line. Clips copied in these apps are never recorded.");
        });
    });
    action
}

/// Show the actions that apply at once, not on Save
fn actions(ui: &mut egui::Ui, pal: &Palette, draft: &mut Draft, status: &Status) -> Outcome {
    let mut outcome = Outcome::Open;
    section(ui, pal, "Right now");
    row(ui, pal, "Recording", |ui| {
        let mut paused = status.paused;
        if ui
            .checkbox(&mut paused, "Pause recording new copies")
            .changed()
        {
            outcome = Outcome::SetPaused(paused);
        }
    });
    row(ui, pal, "Accessibility", |ui| {
        if status.ax_trusted {
            note(ui, pal, "Allowed, so auto-paste can press ⌘V for you");
        } else if ui.button("Enable auto-paste…").clicked() {
            outcome = Outcome::EnableAutoPaste;
        }
    });
    row(ui, pal, "History", |ui| {
        if let Some(o) = clear_ui(ui, pal, draft, status.unpinned) {
            outcome = o;
        }
    });
    outcome
}

/// Confirm before clearing, so a stray click can't wipe the history
fn clear_ui(
    ui: &mut egui::Ui,
    pal: &Palette,
    draft: &mut Draft,
    unpinned: usize,
) -> Option<Outcome> {
    if !draft.confirm_clear {
        let enabled = unpinned > 0;
        if ui
            .add_enabled(enabled, egui::Button::new("Clear unpinned history…"))
            .clicked()
        {
            draft.confirm_clear = true;
        }
        return None;
    }
    let mut outcome = None;
    ui.vertical(|ui| {
        let msg = format!("Delete {unpinned} unpinned clips? Pinned clips are kept.");
        ui.label(RichText::new(msg).color(pal.text));
        ui.horizontal(|ui| {
            if ui.button(RichText::new("Clear").color(pal.red)).clicked() {
                draft.confirm_clear = false;
                outcome = Some(Outcome::ClearHistory);
            }
            if ui.button("Keep").clicked() {
                draft.confirm_clear = false;
            }
        });
    });
    outcome
}

fn section(ui: &mut egui::Ui, pal: &Palette, title: &str) {
    ui.add_space(12.0);
    ui.label(
        RichText::new(title.to_uppercase())
            .font(theme::medium(11.0))
            .color(pal.faint),
    );
    ui.add_space(4.0);
}

fn row(ui: &mut egui::Ui, pal: &Palette, label: &str, control: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_top(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(LABEL_W, 22.0), Sense::hover());
        let g = ui
            .painter()
            .layout_no_wrap(label.to_string(), theme::sans(13.0), pal.muted);
        ui.painter().galley(
            rect.left_center() - Vec2::new(0.0, g.size().y / 2.0),
            g,
            pal.muted,
        );
        control(ui);
    });
    ui.add_space(6.0);
}

fn note(ui: &mut egui::Ui, pal: &Palette, text: &str) {
    ui.label(RichText::new(text).font(theme::sans(11.5)).color(pal.faint));
}

fn choice(ui: &mut egui::Ui, value: &mut String, options: &[(&str, &str)]) {
    let current = value.to_lowercase();
    for (key, label) in options {
        if ui.selectable_label(current == *key, *label).clicked() {
            *value = key.to_string();
        }
    }
}

fn shortcut_ui(ui: &mut egui::Ui, pal: &Palette, draft: &mut Draft) {
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            if draft.recording {
                record_keys(ui, draft);
                let label = RichText::new("Press keys…  (esc to cancel)").color(pal.accent_ink);
                if ui.button(label).clicked() {
                    draft.recording = false;
                }
            } else {
                ui.add(egui::TextEdit::singleline(&mut draft.cfg.hotkey).desired_width(150.0));
                if ui.button("Record").clicked() {
                    draft.recording = true;
                }
                let label = crate::app::pretty_hotkey(&draft.cfg.hotkey);
                let w = theme::keycap_width(ui, &label, 20.0);
                let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 20.0), Sense::hover());
                theme::keycap_at(ui.painter(), pal, rect.left_center(), &label, 20.0, false);
            }
        });
        match hotkey::parse(&draft.cfg.hotkey) {
            Ok(_) => note(ui, pal, "e.g. cmd+shift+v, ctrl+alt+KeyC, cmd+shift+Digit9"),
            Err(e) => {
                ui.label(
                    RichText::new(e.to_string())
                        .font(theme::sans(11.5))
                        .color(pal.red),
                );
            }
        }
    });
}

/// Turn the next key press with a modifier into a shortcut spec
fn record_keys(ui: &mut egui::Ui, draft: &mut Draft) {
    let pressed = ui.input_mut(|i| {
        if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
            return Some(None);
        }
        i.events.iter().find_map(|e| match e {
            egui::Event::Key {
                key,
                physical_key,
                pressed: true,
                modifiers,
                ..
            } => Some(Some(spec_for(physical_key.unwrap_or(*key), *modifiers))),
            _ => None,
        })
    });
    match pressed {
        Some(Some(Some(spec))) => {
            draft.cfg.hotkey = spec;
            draft.recording = false;
        }
        Some(None) => draft.recording = false,
        _ => {}
    }
}

/// `None` without a modifier: a bare key would swallow normal typing
fn spec_for(key: egui::Key, m: egui::Modifiers) -> Option<String> {
    if !(m.mac_cmd || m.command || m.ctrl || m.alt) {
        return None;
    }
    let mut parts = Vec::new();
    if m.ctrl {
        parts.push("ctrl".to_string());
    }
    if m.alt {
        parts.push("alt".to_string());
    }
    if m.shift {
        parts.push("shift".to_string());
    }
    if m.mac_cmd || (m.command && !m.ctrl) {
        parts.push("cmd".to_string());
    }
    parts.push(key.name().to_string());
    Some(parts.join("+"))
}

fn accent_ui(ui: &mut egui::Ui, draft: &mut Draft) {
    let mut color = theme::parse_accent(&draft.cfg.accent);
    for hex in ACCENTS {
        let swatch = theme::parse_accent(hex);
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
        ui.painter().circle_filled(rect.center(), 9.0, swatch);
        if swatch == color {
            ui.painter()
                .circle_stroke(rect.center(), 11.0, Stroke::new(1.5, swatch));
        }
        if resp
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            draft.cfg.accent = hex.to_string();
        }
    }
    if ui.color_edit_button_srgba(&mut color).changed() {
        draft.cfg.accent = to_hex(color);
    }
}

fn to_hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

fn footer(ui: &mut egui::Ui, pal: &Palette, draft: &mut Draft) -> Outcome {
    if let Some(e) = &draft.error {
        ui.label(RichText::new(e).font(theme::sans(12.0)).color(pal.red));
        ui.add_space(6.0);
    }
    let mut outcome = Outcome::Open;
    ui.horizontal(|ui| {
        if theme::button(
            ui,
            pal,
            ButtonStyle::Ghost,
            None,
            "Open settings file",
            None,
        )
        .clicked()
        {
            outcome = Outcome::RevealFile;
        }
        if theme::button(ui, pal, ButtonStyle::Ghost, None, "Reset to defaults", None).clicked() {
            *draft = Draft::new(&Config::default(), false);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if theme::button(ui, pal, ButtonStyle::Primary, None, "Save", None).clicked() {
                match draft.to_saved() {
                    Ok(saved) => outcome = Outcome::Save(saved),
                    Err(e) => draft.error = Some(e),
                }
            }
            if theme::button(ui, pal, ButtonStyle::Outline, None, "Cancel", None).clicked() {
                outcome = Outcome::Cancel;
            }
        });
    });
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_keys_become_parseable_specs() {
        let m = egui::Modifiers {
            shift: true,
            mac_cmd: true,
            command: true,
            ..Default::default()
        };
        let spec = spec_for(egui::Key::V, m).unwrap();
        assert_eq!(spec, "shift+cmd+V");
        assert!(hotkey::parse(&spec).is_ok());
        assert!(hotkey::parse(&spec_for(egui::Key::Num9, m).unwrap()).is_ok());
        assert_eq!(spec_for(egui::Key::A, egui::Modifiers::SHIFT), None);
    }

    #[test]
    fn draft_cleans_up_ignored_apps_and_rejects_bad_shortcuts() {
        let mut d = Draft::new(&Config::default(), false);
        d.ignore_text = "  Foo \n\n com.bar ".to_string();
        assert_eq!(d.to_config().unwrap().ignore_apps, ["Foo", "com.bar"]);
        d.cfg.hotkey = "cmd+shift+nope".to_string();
        assert!(d.to_config().is_err());
    }
}
