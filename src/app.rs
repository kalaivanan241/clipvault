//! The history window: a palette with search, filters, a grouped list, a
//! preview pane with actions, and a footer. Layout, sizes and colours follow
//! the "History window" design mockup.

use crate::classify::{self, Group, TextKind, Tok};
use crate::config::Config;
use crate::store::{now_ms, Clip, FileEntry, Filter, Kind, Store};
use crate::theme::{self, alpha, ButtonStyle, Icon, Palette};
use crate::watcher::{self, Shared};
use crate::{hotkey, platform};
use anyhow::{anyhow, Result};
use eframe::egui::{
    self, Color32, Key, Modifiers, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle,
    UiBuilder, Vec2,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

const LIST_LIMIT: usize = 300;
const DETAIL_TEXT_CHARS: usize = 20_000;
const CODE_PREVIEW_LINES: usize = 500;
const PREVIEW_IMAGE_MAX: u32 = 1400;

// Layout (logical points), from the design.
const HEADER_H: f32 = 64.0;
const FILTER_H: f32 = 54.0;
const FOOTER_H: f32 = 38.0;
const ACTION_H: f32 = 56.0;
const LIST_W: f32 = 360.0;
const ROW_H: f32 = 56.0;

const FILTERS: [(Filter, &str); 5] = [
    (Filter::All, "All"),
    (Filter::Text, "Text"),
    (Filter::Images, "Images"),
    (Filter::Files, "Files"),
    (Filter::Pinned, "Pinned"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowKind {
    Text,
    Code,
    Link,
    Color,
    Image,
    Files,
}

impl RowKind {
    fn of(clip: &Clip) -> RowKind {
        match clip.kind {
            Kind::Image => RowKind::Image,
            Kind::Files => RowKind::Files,
            Kind::Text => match classify::text_kind(&clip.content) {
                TextKind::Plain => RowKind::Text,
                TextKind::Code => RowKind::Code,
                TextKind::Link => RowKind::Link,
                TextKind::Color => RowKind::Color,
            },
        }
    }

    fn label(self) -> &'static str {
        match self {
            RowKind::Text => "Text",
            RowKind::Code => "Code",
            RowKind::Link => "Link",
            RowKind::Color => "Color",
            RowKind::Image => "Image",
            RowKind::Files => "Files",
        }
    }

    fn icon(self) -> Icon {
        match self {
            RowKind::Text | RowKind::Color => Icon::Text,
            RowKind::Code => Icon::Code,
            RowKind::Link => Icon::Link,
            RowKind::Image => Icon::Image,
            RowKind::Files => Icon::Folder,
        }
    }

    fn tint(self, pal: &Palette) -> Color32 {
        match self {
            RowKind::Text | RowKind::Color => pal.blue,
            RowKind::Code => pal.green,
            RowKind::Link => pal.violet,
            RowKind::Image => pal.pink,
            RowKind::Files => pal.amber,
        }
    }

    fn mono(self) -> bool {
        matches!(self, RowKind::Code | RowKind::Link | RowKind::Color)
    }
}

/// A list row, worked out once per reload.
struct Row {
    clip: Clip,
    kind: RowKind,
    title: String,
    group: Group,
}

impl Row {
    fn new(clip: Clip, now: i64) -> Row {
        let kind = RowKind::of(&clip);
        let title = match clip.kind {
            Kind::Text => clip
                .content
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .unwrap_or("")
                .chars()
                .take(300)
                .collect(),
            Kind::Image => format!("Image {} × {}", clip.width, clip.height),
            Kind::Files => clip.preview.clone(),
        };
        let group = classify::group_for(clip.created_at, clip.pinned, now);
        Row {
            clip,
            kind,
            title,
            group,
        }
    }
}

enum Toast {
    Done(String),
    Warn(String),
}

pub struct ClipApp {
    store: Store,
    shared: Arc<Shared>,
    clipboard: arboard::Clipboard,
    _hotkey: Option<global_hotkey::GlobalHotKeyManager>,
    #[cfg(target_os = "macos")]
    tray: Option<crate::tray::Tray>,
    hotkey_label: String,
    auto_paste: bool,
    /// Accessibility permission (needed for auto-paste), re-checked on show.
    ax_trusted: bool,
    config_path: PathBuf,

    theme_pref: String,
    accent: Color32,
    applied_dark: Option<bool>,

    query: String,
    filter: Filter,
    rows: Vec<Row>,
    counts: [usize; 5],
    selected: usize,
    scroll_to_selected: bool,
    seen_version: u64,
    reload: bool,
    focus_search: bool,
    quitting: bool,

    thumbs: HashMap<i64, Option<TextureHandle>>,
    preview_tex: Option<(i64, Option<TextureHandle>)>,
    detail_text: Option<(i64, String)>,
    toast: Option<(Toast, Instant)>,
    /// Dev aid: `CLIPVAULT_SCREENSHOT=out.png` saves the window and quits.
    screenshot: Option<(PathBuf, u32)>,
}

impl ClipApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        dir: PathBuf,
        cfg: Config,
    ) -> Result<ClipApp, Box<dyn std::error::Error + Send + Sync>> {
        Self::build(cc, dir, cfg).map_err(|e| e.into())
    }

    fn build(cc: &eframe::CreationContext<'_>, dir: PathBuf, cfg: Config) -> Result<ClipApp> {
        theme::install_fonts(&cc.egui_ctx);
        let store = Store::open(&dir, cfg.max_items, cfg.max_copy_mb)?;
        let shared = Shared::new();
        let screenshot = std::env::var_os("CLIPVAULT_SCREENSHOT").map(|p| (PathBuf::from(p), 0));
        // Screenshots show the history as it is, so don't record anything new.
        if screenshot.is_none() {
            let ctx = cc.egui_ctx.clone();
            watcher::spawn(cfg.clone(), dir.clone(), ctx, shared.clone());
        }

        let mut toast = None;
        let hotkey = match hotkey::register(&cfg.hotkey, cc.egui_ctx.clone(), shared.clone()) {
            Ok(m) => Some(m),
            Err(e) => {
                eprintln!("{e:#}");
                toast = Some((Toast::Warn(format!("{e}")), Instant::now()));
                None
            }
        };

        let hotkey_label = pretty_hotkey(&cfg.hotkey);
        #[cfg(target_os = "macos")]
        let tray = match crate::tray::create(
            crate::tray::Glyph::parse(&cfg.menu_bar_icon),
            &hotkey_label,
            cc.egui_ctx.clone(),
            shared.clone(),
        ) {
            Ok(t) => Some(t),
            Err(e) => {
                eprintln!("{e:#}");
                None
            }
        };

        let clipboard = arboard::Clipboard::new().map_err(|e| anyhow!("clipboard: {e}"))?;
        let selected = std::env::var("CLIPVAULT_SELECT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        Ok(ClipApp {
            store,
            shared,
            clipboard,
            _hotkey: hotkey,
            #[cfg(target_os = "macos")]
            tray,
            hotkey_label,
            auto_paste: cfg.auto_paste,
            ax_trusted: platform::accessibility_trusted(false),
            config_path: dir.join("config.json"),
            theme_pref: cfg.theme.to_lowercase(),
            accent: theme::parse_accent(&cfg.accent),
            applied_dark: None,
            query: String::new(),
            filter: Filter::All,
            rows: Vec::new(),
            counts: [0; 5],
            selected,
            scroll_to_selected: true,
            seen_version: 0,
            reload: true,
            focus_search: true,
            quitting: false,
            thumbs: HashMap::new(),
            preview_tex: None,
            detail_text: None,
            toast,
            screenshot,
        })
    }

    // ---------- window control ----------

    fn hide(&mut self, ctx: &egui::Context) {
        self.shared.visible.store(false, Ordering::Relaxed);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        platform::hide_app();
    }

    fn done(&mut self, msg: impl Into<String>) {
        self.toast = Some((Toast::Done(msg.into()), Instant::now()));
    }

    fn warn(&mut self, msg: impl Into<String>) {
        self.toast = Some((Toast::Warn(msg.into()), Instant::now()));
    }

    fn palette(&mut self, ctx: &egui::Context) -> Palette {
        let dark = match self.theme_pref.as_str() {
            "dark" => true,
            "light" => false,
            _ => ctx.system_theme().is_none_or(|t| t == egui::Theme::Dark),
        };
        let pal = Palette::new(dark, self.accent);
        if self.applied_dark != Some(dark) {
            ctx.set_theme(if dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            });
            pal.apply(ctx);
            self.applied_dark = Some(dark);
        }
        pal
    }

    // ---------- data ----------

    fn reload_list(&mut self) {
        let keep = self.selected_clip().map(|c| c.id);
        match self.store.list(&self.query, self.filter, LIST_LIMIT) {
            Ok(clips) => {
                let now = now_ms();
                self.rows = clips.into_iter().map(|c| Row::new(c, now)).collect();
            }
            Err(e) => self.warn(format!("Could not read history: {e}")),
        }
        self.counts = self.store.counts();
        // Stay on the same clip when new ones arrive above it.
        if let Some(id) = keep {
            if let Some(i) = self.rows.iter().position(|r| r.clip.id == id) {
                self.selected = i;
            }
        }
        if self.selected >= self.rows.len() {
            self.selected = self.rows.len().saturating_sub(1);
        }
        self.detail_text = None;
        if self.thumbs.len() > 150 {
            self.thumbs.clear();
        }
        self.reload = false;
    }

    fn selected_clip(&self) -> Option<&Clip> {
        self.rows.get(self.selected).map(|r| &r.clip)
    }

    /// Mark the clipboard change we are about to make as ours, run `write`,
    /// then remember the resulting change count.
    fn write_clipboard(
        &mut self,
        write: impl FnOnce(&mut arboard::Clipboard) -> Result<()>,
    ) -> Result<()> {
        // With a pasteboard change counter we remember the exact change below
        // and only need a short window for the moment in between; without
        // one, wait out a poll.
        let window = if platform::change_count().is_some() {
            300
        } else {
            2500
        };
        self.shared
            .skip_until_ms
            .store(now_ms() + window, Ordering::Relaxed);
        write(&mut self.clipboard)?;
        if let Some(c) = platform::change_count() {
            self.shared.own_change.store(c, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Put the clip back on the system clipboard (and move it to the top).
    fn copy_back(&mut self, idx: usize) -> Result<()> {
        let clip = self
            .rows
            .get(idx)
            .map(|r| r.clip.clone())
            .ok_or_else(|| anyhow!("nothing selected"))?;
        match clip.kind {
            Kind::Text => {
                let text = self.store.full_text(clip.id)?;
                self.write_clipboard(|cb| cb.set_text(text).map_err(|e| anyhow!("{e}")))?;
            }
            Kind::Image => {
                let img = image::open(self.store.image_path(&clip))?.to_rgba8();
                let (w, h) = img.dimensions();
                self.write_clipboard(|cb| {
                    cb.set_image(arboard::ImageData {
                        width: w as usize,
                        height: h as usize,
                        bytes: std::borrow::Cow::Owned(img.into_raw()),
                    })
                    .map_err(|e| anyhow!("{e}"))
                })?;
            }
            Kind::Files => {
                let paths: Vec<PathBuf> =
                    clip.files.iter().filter_map(|f| f.usable_path()).collect();
                if paths.is_empty() {
                    return Err(anyhow!("the files no longer exist"));
                }
                self.write_clipboard(|cb| cb.set().file_list(&paths).map_err(|e| anyhow!("{e}")))?;
            }
        }
        self.store.touch(clip.id)?;
        self.shared.version.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Put the clip on the clipboard, close the window and, with `paste` (and
    /// auto-paste on), press ⌘V in the app you came from.
    fn paste_selected(&mut self, ctx: &egui::Context, idx: usize, paste: bool) {
        if let Err(e) = self.copy_back(idx) {
            self.warn(format!("Could not copy: {e}"));
            return;
        }
        self.hide(ctx);
        if !(paste && self.auto_paste) {
            return;
        }
        self.ax_trusted = platform::accessibility_trusted(false);
        if self.ax_trusted {
            // Give macOS a moment to bring the previous app back to the front.
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(180));
                platform::send_paste();
            });
        } else {
            // Shows the system permission dialog; the clip is already copied.
            platform::accessibility_trusted(true);
            self.warn("Copied. Allow ClipVault under Accessibility to paste automatically");
        }
    }

    /// Copy a derived value (e.g. a colour's RGB) without closing the window.
    fn copy_value(&mut self, label: &str, value: String) {
        match self.write_clipboard(|cb| cb.set_text(value).map_err(|e| anyhow!("{e}"))) {
            Ok(()) => self.done(format!("Copied {label}. Press ⌘V in your app.")),
            Err(e) => self.warn(format!("Could not copy: {e}")),
        }
    }

    fn toggle_pin(&mut self, id: i64) {
        let _ = self.store.toggle_pin(id);
        self.reload = true;
    }

    fn delete(&mut self, id: i64) {
        let _ = self.store.delete(id);
        self.reload = true;
    }

    fn set_paused(&mut self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Relaxed);
    }

    fn thumb_for(&mut self, ctx: &egui::Context, clip: &Clip) -> Option<TextureHandle> {
        if let Some(t) = self.thumbs.get(&clip.id) {
            return t.clone();
        }
        let tex = image::open(self.store.thumb_path(clip))
            .ok()
            .map(|img| load_texture(ctx, &format!("thumb-{}", clip.id), img));
        self.thumbs.insert(clip.id, tex.clone());
        tex
    }

    /// Sharper image for the preview pane (one at a time).
    fn preview_for(&mut self, ctx: &egui::Context, clip: &Clip) -> Option<TextureHandle> {
        if let Some((id, t)) = &self.preview_tex {
            if *id == clip.id {
                return t.clone();
            }
        }
        let tex = image::open(self.store.image_path(clip)).ok().map(|img| {
            let img = if img.width().max(img.height()) > PREVIEW_IMAGE_MAX {
                img.thumbnail(PREVIEW_IMAGE_MAX, PREVIEW_IMAGE_MAX)
            } else {
                img
            };
            load_texture(ctx, &format!("preview-{}", clip.id), img)
        });
        self.preview_tex = Some((clip.id, tex.clone()));
        tex
    }

    // ---------- keyboard ----------

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let (down, up, enter, copy_only, esc, del, pin, tab) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::SHIFT, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
                i.consume_key(Modifiers::COMMAND, Key::Backspace)
                    || i.consume_key(Modifiers::COMMAND, Key::Delete),
                i.consume_key(Modifiers::COMMAND, Key::P),
                i.consume_key(Modifiers::NONE, Key::Tab),
            )
        });
        let digits = [
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
        ];
        let mut quick = None;
        ctx.input_mut(|i| {
            for (n, k) in digits.iter().enumerate() {
                if i.consume_key(Modifiers::COMMAND, *k) {
                    quick = Some(n);
                }
            }
        });

        if esc {
            if self.query.is_empty() {
                self.hide(ctx);
            } else {
                self.query.clear();
                self.reload = true;
            }
        }
        if down && self.selected + 1 < self.rows.len() {
            self.selected += 1;
            self.scroll_to_selected = true;
        }
        if up && self.selected > 0 {
            self.selected -= 1;
            self.scroll_to_selected = true;
        }
        if tab {
            self.filter = match self.filter {
                Filter::All => Filter::Text,
                Filter::Text => Filter::Images,
                Filter::Images => Filter::Files,
                Filter::Files => Filter::Pinned,
                Filter::Pinned => Filter::All,
            };
            self.selected = 0;
            self.reload = true;
        }
        if (enter || copy_only) && !self.rows.is_empty() {
            self.paste_selected(ctx, self.selected, enter);
        }
        if let Some(n) = quick {
            if n < self.rows.len() {
                self.paste_selected(ctx, n, true);
            }
        }
        if pin {
            if let Some(id) = self.selected_clip().map(|c| c.id) {
                self.toggle_pin(id);
            }
        }
        if del {
            if let Some(id) = self.selected_clip().map(|c| c.id) {
                self.delete(id);
            }
        }
    }
}

impl eframe::App for ClipApp {
    /// Runs before every frame and also while the window is hidden, which is
    /// what lets the global shortcut bring the window back.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // The red close button hides to the background; use Quit to really exit.
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide(ctx);
        }
        if self.shared.hide_requested.swap(false, Ordering::Relaxed) {
            self.hide(ctx);
        }
        if self.shared.just_shown.swap(false, Ordering::Relaxed) {
            if self.screenshot.is_none() {
                self.query.clear();
                self.filter = Filter::All;
                self.selected = 0;
            }
            self.focus_search = true;
            self.reload = true;
            self.ax_trusted = platform::accessibility_trusted(false);
            platform::activate_window();
        }
        #[cfg(target_os = "macos")]
        if let Some(t) = &self.tray {
            t.sync_paused(self.shared.paused.load(Ordering::Relaxed));
        }
        let v = self.shared.version.load(Ordering::Relaxed);
        if v != self.seen_version {
            self.seen_version = v;
            self.reload = true;
        }
        if self.reload {
            self.reload_list();
        }
        if self.shared.visible.load(Ordering::Relaxed) {
            self.handle_keys(ctx);
            if self.reload {
                self.reload_list();
            }
        }
        if let Some((toast, t)) = &self.toast {
            let ttl = match toast {
                Toast::Done(_) => 2.2,
                Toast::Warn(_) => 6.0,
            };
            if t.elapsed().as_secs_f32() > ttl {
                self.toast = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }
        if let Some((path, frames)) = &mut self.screenshot {
            *frames += 1;
            if *frames == 10 {
                let path = path.clone();
                ctx.request_screenshot(move |img| {
                    let [w, h] = img.size;
                    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                    if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                        let _ = buf.save(&path);
                    }
                    std::process::exit(0);
                });
            }
            ctx.request_repaint();
        }
    }

    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let pal = self.palette(&ctx);
        let full = root.max_rect();
        root.painter().rect_filled(full, 0.0, pal.panel);

        let w = full.width();
        let header = Rect::from_min_size(full.min, Vec2::new(w, HEADER_H));
        let filters = Rect::from_min_size(header.left_bottom(), Vec2::new(w, FILTER_H));
        let footer = Rect::from_min_max(Pos2::new(full.left(), full.bottom() - FOOTER_H), full.max);
        let body = Rect::from_min_max(filters.left_bottom(), footer.right_top());
        let list_w = LIST_W.min(body.width() * 0.48);
        let list = Rect::from_min_max(body.min, Pos2::new(body.left() + list_w, body.bottom()));
        let preview = Rect::from_min_max(list.right_top(), body.max);

        self.header_ui(root, &ctx, &pal, header);
        self.filters_ui(root, &pal, filters);
        self.list_ui(root, &ctx, &pal, list);
        self.preview_ui(root, &ctx, &pal, preview);
        self.footer_ui(root, &ctx, &pal, footer);

        let p = root.painter();
        let line = Stroke::new(1.0, pal.line);
        p.hline(full.x_range(), header.bottom() - 0.5, line);
        p.hline(full.x_range(), filters.bottom() - 0.5, line);
        p.vline(list.right() - 0.5, body.y_range(), line);
        p.hline(full.x_range(), footer.top() + 0.5, line);

        self.toast_ui(root, &pal, Rect::from_min_max(full.min, footer.right_top()));
    }
}

// ---------- sections ----------

impl ClipApp {
    fn header_ui(&mut self, root: &mut egui::Ui, ctx: &egui::Context, pal: &Palette, rect: Rect) {
        // The header doubles as the window's drag handle (there is no title bar).
        let drag = root.interact(
            rect,
            egui::Id::unique("window-drag"),
            Sense::click_and_drag(),
        );
        if drag.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }

        let cy = rect.center().y;
        let icon_rect =
            Rect::from_center_size(Pos2::new(rect.left() + 30.0, cy), Vec2::splat(20.0));
        theme::icon(root.painter(), icon_rect, Icon::Search, pal.muted, 2.0);

        let key_w = theme::keycap_width(root, &self.hotkey_label, 22.0);
        let key_x = rect.right() - 20.0 - key_w;
        theme::keycap_at(
            root.painter(),
            pal,
            Pos2::new(key_x, cy),
            &self.hotkey_label,
            22.0,
            false,
        );

        let field = Rect::from_min_max(
            Pos2::new(icon_rect.right() + 14.0, cy - 14.0),
            Pos2::new(key_x - 14.0, cy + 14.0),
        );
        let resp = root.put(
            field,
            egui::TextEdit::singleline(&mut self.query)
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .font(theme::sans(18.0))
                .text_color(pal.text)
                .hint_text(
                    RichText::new("Search everything you copied…")
                        .font(theme::sans(18.0))
                        .color(pal.faint),
                )
                .desired_width(field.width()),
        );
        if self.focus_search {
            resp.request_focus();
            self.focus_search = false;
        }
        if resp.changed() {
            self.selected = 0;
            self.reload = true;
        }
    }

    fn filters_ui(&mut self, root: &mut egui::Ui, pal: &Palette, rect: Rect) {
        let p = root.painter().clone();
        let cy = rect.center().y;
        let chips: Vec<_> = FILTERS
            .iter()
            .enumerate()
            .map(|(i, (f, label))| {
                let on = self.filter == *f;
                let font = if on {
                    theme::semibold(12.5)
                } else {
                    theme::medium(12.5)
                };
                let lg = p.layout_no_wrap(label.to_string(), font, pal.text);
                let count_color = if on { pal.accent_ink } else { pal.faint };
                let cg =
                    p.layout_no_wrap(self.counts[i].to_string(), theme::mono(10.5), count_color);
                let w = 10.0 + lg.size().x + 7.0 + cg.size().x + 10.0;
                (i, *f, on, lg, cg, count_color, w)
            })
            .collect();
        let total = chips.iter().map(|c| c.6).sum::<f32>() + 2.0 * (chips.len() - 1) as f32 + 6.0;
        let group = Rect::from_min_size(
            Pos2::new(rect.left() + 20.0, cy - 17.0),
            Vec2::new(total, 34.0),
        );
        p.rect_filled(group, 10.0, pal.elev);

        let mut x = group.left() + 3.0;
        for (i, f, on, lg, cg, count_color, w) in chips {
            let chip = Rect::from_min_size(Pos2::new(x, cy - 14.0), Vec2::new(w, 28.0));
            let resp = root
                .interact(chip, egui::Id::unique(("filter-chip", i)), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if on {
                let shadow_alpha = if pal.dark { 64 } else { 20 };
                p.rect_filled(
                    chip.translate(Vec2::new(0.0, 1.0)),
                    7.0,
                    Color32::from_black_alpha(shadow_alpha),
                );
                p.rect(
                    chip,
                    7.0,
                    pal.panel_solid,
                    Stroke::new(1.0, pal.line),
                    StrokeKind::Inside,
                );
            }
            let label_color = if on || resp.hovered() {
                pal.text
            } else {
                pal.muted
            };
            let lw = lg.size().x;
            p.galley_with_override_text_color(
                Pos2::new(chip.left() + 10.0, cy - lg.size().y / 2.0),
                lg,
                label_color,
            );
            p.galley(
                Pos2::new(chip.left() + 10.0 + lw + 7.0, cy - cg.size().y / 2.0),
                cg,
                count_color,
            );
            if resp.clicked() {
                self.filter = f;
                self.selected = 0;
                self.reload = true;
                self.focus_search = true;
            }
            x += w + 2.0;
        }

        // "[tab] to switch"
        let tg = p.layout_no_wrap("to switch".into(), theme::sans(12.0), pal.faint);
        let tx = rect.right() - 20.0 - tg.size().x;
        p.galley(Pos2::new(tx, cy - tg.size().y / 2.0), tg, pal.faint);
        let kw = theme::keycap_width(root, "tab", 18.0);
        theme::keycap_at(&p, pal, Pos2::new(tx - 6.0 - kw, cy), "tab", 18.0, false);
    }

    fn list_ui(&mut self, root: &mut egui::Ui, ctx: &egui::Context, pal: &Palette, rect: Rect) {
        let inner = Rect::from_min_max(
            rect.min + Vec2::new(10.0, 6.0),
            rect.max - Vec2::new(10.0, 0.0),
        );
        let mut ui = root.new_child(UiBuilder::new().max_rect(inner));

        if self.rows.is_empty() {
            let c = rect.center();
            theme::icon(
                ui.painter(),
                Rect::from_center_size(c - Vec2::new(0.0, 18.0), Vec2::splat(28.0)),
                Icon::Clipboard,
                pal.faint,
                1.7,
            );
            let msg = if self.query.trim().is_empty() {
                "Nothing here yet. Copy some text, an image or a file.".to_string()
            } else {
                format!("Nothing matches “{}”.", self.query.trim())
            };
            let mut job = egui::text::LayoutJob::simple(
                msg,
                theme::sans(13.0),
                pal.muted,
                rect.width() - 48.0,
            );
            job.halign = egui::Align::Center;
            let g = ui.painter().layout_job(job);
            ui.painter().galley(Pos2::new(c.x, c.y + 6.0), g, pal.muted);
            return;
        }

        let mut clicked = None;
        let mut activated = None;
        let rows = std::mem::take(&mut self.rows);
        egui::ScrollArea::vertical()
            .id_salt("list")
            .auto_shrink([false, false])
            .show(&mut ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let mut last_group = None;
                for (i, row) in rows.iter().enumerate() {
                    if last_group != Some(row.group) {
                        last_group = Some(row.group);
                        group_header(ui, pal, row.group.label());
                    }
                    let resp = self.row_ui(ui, ctx, pal, i, row);
                    if i == self.selected && self.scroll_to_selected {
                        resp.scroll_to_me(None);
                    }
                    if resp.clicked() {
                        clicked = Some(i);
                    }
                    if resp.double_clicked() {
                        activated = Some(i);
                    }
                }
                ui.add_space(12.0);
            });
        self.rows = rows;
        self.scroll_to_selected = false;
        if let Some(i) = clicked {
            self.selected = i;
            self.focus_search = true;
        }
        if let Some(i) = activated {
            self.paste_selected(ctx, i, true);
        }
    }

    fn row_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        pal: &Palette,
        i: usize,
        row: &Row,
    ) -> egui::Response {
        let (rect, resp) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
        if !ui.is_rect_visible(rect.expand(4.0)) {
            return resp;
        }
        let selected = i == self.selected;
        let p = ui.painter().clone();
        if selected {
            p.rect_filled(rect, 11.0, pal.sel);
            p.rect_stroke(
                rect,
                11.0,
                Stroke::new(1.0, alpha(pal.accent, 0.55)),
                StrokeKind::Inside,
            );
            p.rect_stroke(
                rect,
                11.0,
                Stroke::new(4.0, alpha(pal.accent, 0.12)),
                StrokeKind::Outside,
            );
        } else if resp.hovered() {
            p.rect_filled(rect, 11.0, pal.elev);
        }

        // kind tile
        let tile = Rect::from_min_size(
            Pos2::new(rect.left() + 10.0, rect.center().y - 18.0),
            Vec2::splat(36.0),
        );
        match row.kind {
            RowKind::Color => {
                let c = classify::parse_hex_color(row.clip.content.trim())
                    .map(|[r, g, b]| Color32::from_rgb(r, g, b))
                    .unwrap_or(pal.elev2);
                p.rect_filled(tile, 10.0, c);
                p.rect_stroke(
                    tile,
                    10.0,
                    Stroke::new(1.0, Color32::from_black_alpha(38)),
                    StrokeKind::Inside,
                );
            }
            RowKind::Image => match self.thumb_for(ctx, &row.clip) {
                Some(tex) => {
                    p.add(
                        egui::epaint::RectShape::filled(tile, 10.0, Color32::WHITE)
                            .with_texture(tex.id(), cover_uv(tex.size_vec2())),
                    );
                    p.rect_stroke(tile, 10.0, Stroke::new(1.0, pal.line), StrokeKind::Inside);
                }
                None => tile_icon(&p, pal, tile, row.kind),
            },
            _ => tile_icon(&p, pal, tile, row.kind),
        }

        // key cap on the right: ↵ on the selection, ⌘1–9 on the first nine
        let key = if selected {
            Some("↵".to_string())
        } else if i < 9 {
            Some(format!("⌘{}", i + 1))
        } else {
            None
        };
        let key_w = key
            .as_ref()
            .map(|k| theme::keycap_width(ui, k, 18.0))
            .unwrap_or(0.0);
        if let Some(k) = &key {
            let at = Pos2::new(rect.right() - 10.0 - key_w, rect.center().y);
            if selected {
                theme::keycap_at(&p, pal, at, k, 18.0, true);
            } else {
                let faded = Palette {
                    muted: alpha(pal.muted, 0.85),
                    ..*pal
                };
                theme::keycap_at(&p, &faded, at, k, 18.0, false);
            }
        }

        // title + subtitle
        let tx = tile.right() + 12.0;
        let tw = rect.right() - 10.0 - key_w - 12.0 - tx;
        let title_font = if row.kind.mono() {
            theme::mono(12.5)
        } else {
            theme::medium(13.5)
        };
        let title = theme::elided(ui, &row.title, title_font, pal.text, tw);
        let mut sub = String::new();
        if !row.clip.source_app.is_empty() {
            sub.push_str(&row.clip.source_app);
            sub.push_str(" · ");
        }
        if row.clip.pinned {
            sub.push_str("pinned");
        } else {
            sub.push_str(&classify::ago(row.clip.created_at, now_ms()));
        }
        let pin_w = if row.clip.pinned { 17.0 } else { 0.0 };
        let subg = theme::elided(ui, &sub, theme::sans(11.5), pal.faint, tw - pin_w);
        let top = rect.center().y - (title.size().y + 3.0 + subg.size().y) / 2.0;
        let sy = top + title.size().y + 3.0;
        p.galley(Pos2::new(tx, top), title, pal.text);
        if row.clip.pinned {
            let pr = Rect::from_min_size(
                Pos2::new(tx, sy + (subg.size().y - 11.0) / 2.0),
                Vec2::splat(11.0),
            );
            theme::icon(&p, pr, Icon::Pin, pal.accent_ink, 2.0);
        }
        p.galley(Pos2::new(tx + pin_w, sy), subg, pal.faint);
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    }

    fn preview_ui(&mut self, root: &mut egui::Ui, ctx: &egui::Context, pal: &Palette, rect: Rect) {
        let Some(row) = self.rows.get(self.selected) else {
            let g = root.painter().layout_no_wrap(
                "Select an item to preview it".into(),
                theme::sans(13.0),
                pal.muted,
            );
            root.painter()
                .galley(rect.center() - g.size() / 2.0, g, pal.muted);
            return;
        };
        let clip = row.clip.clone();
        let kind = row.kind;

        let action = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - ACTION_H), rect.max);
        root.painter().hline(
            rect.x_range(),
            action.top() + 0.5,
            Stroke::new(1.0, pal.line),
        );
        let content = Rect::from_min_max(rect.min, action.right_top());

        let mut ui = root.new_child(
            UiBuilder::new()
                .max_rect(content.shrink2(Vec2::new(20.0, 0.0)))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        ui.set_clip_rect(content);
        ui.add_space(16.0);

        // title row: kind + stats
        let stats = self.stats_for(&clip, kind);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
            ui.label(
                RichText::new(kind.label())
                    .font(theme::semibold(15.0))
                    .color(pal.text),
            );
            for s in &stats {
                theme::pill(ui, s, theme::mono(11.5), pal.muted, pal.elev, 22.0);
            }
        });
        ui.add_space(12.0);

        let scroll = if kind == RowKind::Code {
            egui::ScrollArea::both()
        } else {
            egui::ScrollArea::vertical()
        };
        scroll
            .id_salt(("preview", clip.id))
            .auto_shrink([false, false])
            .max_height(ui.available_height())
            .show(&mut ui, |ui| {
                match kind {
                    RowKind::Code => self.code_preview(ui, pal, &clip),
                    RowKind::Text => self.text_preview(ui, pal, &clip),
                    RowKind::Link => self.link_preview(ui, pal, &clip),
                    RowKind::Color => self.color_preview(ui, pal, &clip),
                    RowKind::Image => self.image_preview(ui, ctx, pal, &clip),
                    RowKind::Files => files_preview(ui, pal, &clip),
                }
                ui.add_space(16.0);
            });

        self.action_bar(root, ctx, pal, action, &clip);
    }

    fn stats_for(&mut self, clip: &Clip, kind: RowKind) -> Vec<String> {
        let plural =
            |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        match kind {
            RowKind::Code => {
                let full = self.full_text(clip);
                vec![
                    plural(full.lines().count().max(1), "line", "lines"),
                    plural(full.chars().count(), "char", "chars"),
                ]
            }
            RowKind::Text => {
                let full = self.full_text(clip);
                vec![
                    plural(full.split_whitespace().count(), "word", "words"),
                    plural(full.chars().count(), "char", "chars"),
                ]
            }
            RowKind::Link => {
                let full = self.full_text(clip);
                vec![
                    classify::parse_link(&full).scheme.to_uppercase(),
                    plural(full.trim().chars().count(), "char", "chars"),
                ]
            }
            RowKind::Color => vec!["sRGB".into()],
            RowKind::Image => vec![
                format!("{} × {}", clip.width, clip.height),
                "PNG".into(),
                human_size(clip.size),
            ],
            RowKind::Files => vec![
                plural(clip.files.len(), "item", "items"),
                human_size(clip.size),
            ],
        }
    }

    /// Full text of the selected text clip (cached; truncated for display).
    fn full_text(&mut self, clip: &Clip) -> String {
        if self.detail_text.as_ref().map(|(id, _)| *id) != Some(clip.id) {
            let full = self.store.full_text(clip.id).unwrap_or_default();
            let shown: String = if full.chars().count() > DETAIL_TEXT_CHARS {
                let mut s: String = full.chars().take(DETAIL_TEXT_CHARS).collect();
                s.push_str("\n… (preview truncated; Copy puts the full text on the clipboard)");
                s
            } else {
                full
            };
            self.detail_text = Some((clip.id, shown));
        }
        self.detail_text
            .as_ref()
            .map(|(_, t)| t.clone())
            .unwrap_or_default()
    }

    fn code_preview(&mut self, ui: &mut egui::Ui, pal: &Palette, clip: &Clip) {
        let full = self.full_text(clip);
        let fmt = |color: Color32| egui::TextFormat {
            font_id: theme::mono(12.5),
            color,
            line_height: Some(12.5 * 1.75),
            ..Default::default()
        };
        let lines: Vec<&str> = full.lines().take(CODE_PREVIEW_LINES).collect();
        let mut nums = egui::text::LayoutJob::default();
        let mut code = egui::text::LayoutJob::default();
        for (n, line) in lines.iter().enumerate() {
            let nl = if n + 1 < lines.len() { "\n" } else { "" };
            nums.append(&format!("{:>4}{nl}", n + 1), 0.0, fmt(pal.faint));
            for (text, tok) in classify::tokenize(line) {
                let color = match tok {
                    Tok::Plain => pal.text,
                    Tok::Keyword => pal.accent_ink,
                    Tok::Str | Tok::Path => pal.amber,
                    Tok::Num => pal.blue,
                    Tok::Flag => pal.violet,
                    Tok::Comment => pal.faint,
                };
                code.append(&text, 0.0, fmt(color));
            }
            code.append(nl, 0.0, fmt(pal.text));
        }
        let nums = ui.painter().layout_job(nums);
        let code = ui.painter().layout_job(code);

        let gutter = 52.0;
        let w = (gutter + code.size().x + 16.0).max(ui.available_width());
        let h = code.size().y.max(nums.size().y) + 28.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
        let p = ui.painter();
        p.rect(
            rect,
            12.0,
            pal.code,
            Stroke::new(1.0, pal.line),
            StrokeKind::Inside,
        );
        let top = rect.top() + 14.0;
        p.galley(
            Pos2::new(rect.left() + gutter - 14.0 - nums.size().x, top),
            nums,
            pal.faint,
        );
        p.galley(Pos2::new(rect.left() + gutter, top), code, pal.text);
    }

    fn text_preview(&mut self, ui: &mut egui::Ui, pal: &Palette, clip: &Clip) {
        let full = self.full_text(clip);
        let mut job = egui::text::LayoutJob::single_section(
            full,
            egui::TextFormat {
                font_id: theme::sans(15.0),
                color: pal.text,
                line_height: Some(15.0 * 1.65),
                ..Default::default()
            },
        );
        // ~46 characters per line, as in the design, for comfortable reading
        job.wrap.max_width = ui.available_width().min(46.0 * 8.2);
        ui.add(egui::Label::new(job).selectable(true));
    }

    fn link_preview(&mut self, ui: &mut egui::Ui, pal: &Palette, clip: &Clip) {
        let full = self.full_text(clip).trim().to_string();
        let link = classify::parse_link(&full);
        let initial: String = link
            .domain
            .trim_start_matches("www.")
            .chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .collect();
        egui::Frame::new()
            .fill(pal.elev)
            .stroke(Stroke::new(1.0, pal.line))
            .corner_radius(14)
            .inner_margin(16)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    let (tile, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
                    ui.painter().rect_filled(tile, 11.0, pal.elev2);
                    let g = ui
                        .painter()
                        .layout_no_wrap(initial, theme::semibold(17.0), pal.violet);
                    ui.painter()
                        .galley(tile.center() - g.size() / 2.0, g, pal.violet);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.label(
                            RichText::new(link.domain)
                                .font(theme::semibold(16.0))
                                .color(pal.text),
                        );
                        wrapped_mono(ui, link.path, 12.0, pal.muted);
                    });
                });
            });
        ui.add_space(14.0);
        wrapped_mono(ui, &full, 12.0, pal.faint);
        ui.add_space(14.0);
        let open = theme::button(
            ui,
            pal,
            ButtonStyle::Outline,
            Some(Icon::External),
            "Open in browser",
            None,
        );
        if open.clicked() {
            platform::open_url(&full);
        }
    }

    fn color_preview(&mut self, ui: &mut egui::Ui, pal: &Palette, clip: &Clip) {
        let hex = self.full_text(clip).trim().to_string();
        let Some(rgb) = classify::parse_hex_color(&hex) else {
            return;
        };
        let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
        let (swatch, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 150.0), Sense::hover());
        ui.painter().rect_filled(swatch, 14.0, color);
        ui.painter().rect_stroke(
            swatch,
            14.0,
            Stroke::new(1.0, Color32::from_black_alpha(31)),
            StrokeKind::Inside,
        );
        ui.add_space(14.0);

        let values = [
            ("HEX", hex.to_uppercase()),
            ("RGB", format!("{}, {}, {}", rgb[0], rgb[1], rgb[2])),
            ("HSL", classify::hsl_string(rgb)),
        ];
        let gap = 8.0;
        let cell_w = (ui.available_width() - gap * 2.0) / 3.0;
        let mut copy = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (label, value) in &values {
                let (r, resp) = ui.allocate_exact_size(Vec2::new(cell_w, 54.0), Sense::click());
                let fill = if resp.hovered() { pal.elev2 } else { pal.elev };
                ui.painter().rect(
                    r,
                    10.0,
                    fill,
                    Stroke::new(1.0, pal.line),
                    StrokeKind::Inside,
                );
                let job = egui::text::LayoutJob::single_section(
                    label.to_string(),
                    egui::TextFormat {
                        font_id: theme::semibold(10.5),
                        color: pal.faint,
                        extra_letter_spacing: 0.84,
                        ..Default::default()
                    },
                );
                let lg = ui.painter().layout_job(job);
                ui.painter()
                    .galley(r.min + Vec2::new(12.0, 10.0), lg, pal.faint);
                let vg = theme::elided(ui, value, theme::mono(12.5), pal.text, cell_w - 24.0);
                ui.painter()
                    .galley(r.min + Vec2::new(12.0, 28.0), vg, pal.text);
                let resp = resp
                    .on_hover_text(format!("Copy {label}"))
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if resp.clicked() {
                    copy = Some((*label, value.clone()));
                }
            }
        });
        if let Some((label, value)) = copy {
            self.copy_value(label, value);
        }
    }

    fn image_preview(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        pal: &Palette,
        clip: &Clip,
    ) {
        let w = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, w * 9.0 / 16.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, 12.0, pal.code);
        // checkerboard, so transparency is visible
        let cell = 8.0;
        let inner = rect.shrink(6.0);
        let (cols, rows) = (
            (inner.width() / cell).ceil() as i32,
            (inner.height() / cell).ceil() as i32,
        );
        let mut mesh = egui::Mesh::default();
        for y in 0..rows {
            for x in 0..cols {
                if (x + y) % 2 == 0 {
                    let min = inner.min + Vec2::new(x as f32 * cell, y as f32 * cell);
                    let r = Rect::from_min_size(min, Vec2::splat(cell)).intersect(inner);
                    mesh.add_colored_rect(r, pal.elev2);
                }
            }
        }
        p.add(mesh);
        p.rect_stroke(rect, 12.0, Stroke::new(1.0, pal.line), StrokeKind::Inside);

        match self.preview_for(ctx, clip) {
            Some(tex) => {
                let area = rect.shrink(12.0);
                let size = tex.size_vec2();
                let scale = (area.width() / size.x).min(area.height() / size.y).min(2.0);
                let r = Rect::from_center_size(area.center(), size * scale);
                let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                p.image(tex.id(), r, uv, Color32::WHITE);
            }
            None => {
                let g = p.layout_no_wrap("Image unavailable".into(), theme::sans(12.0), pal.muted);
                let r = Rect::from_center_size(rect.center(), g.size() + Vec2::new(20.0, 12.0));
                p.rect_filled(r, 7.0, pal.panel_solid);
                p.galley(r.center() - g.size() / 2.0, g, pal.muted);
            }
        }
    }

    fn action_bar(
        &mut self,
        root: &mut egui::Ui,
        ctx: &egui::Context,
        pal: &Palette,
        rect: Rect,
        clip: &Clip,
    ) {
        let mut ui = root.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    rect.min + Vec2::new(20.0, 0.0),
                    rect.max - Vec2::new(16.0, 0.0),
                ))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        ui.spacing_mut().item_spacing.x = 8.0;
        let (primary, paste) = if self.auto_paste {
            ("Paste", true)
        } else {
            ("Copy", false)
        };
        if theme::button(&mut ui, pal, ButtonStyle::Primary, None, primary, Some("↵")).clicked() {
            self.paste_selected(ctx, self.selected, paste);
        }
        let pin_label = if clip.pinned { "Unpin" } else { "Pin" };
        if theme::button(
            &mut ui,
            pal,
            ButtonStyle::Outline,
            None,
            pin_label,
            Some("⌘P"),
        )
        .clicked()
        {
            self.toggle_pin(clip.id);
        }
        if theme::button(&mut ui, pal, ButtonStyle::Ghost, None, "Delete", Some("⌘⌫")).clicked()
        {
            self.delete(clip.id);
        }

        // where it came from, in whatever space is left on the left
        let ago = classify::ago(clip.created_at, now_ms());
        let mut origin = if ago == "Yesterday" || ago.contains("ago") || ago == "just now" {
            format!("Copied {}", ago.to_lowercase())
        } else {
            format!("Copied {ago}")
        };
        if !clip.source_app.is_empty() {
            origin.push_str(" from ");
            origin.push_str(&clip.source_app);
        }
        let left = rect.left() + 20.0;
        let avail = ui.min_rect().left() - 8.0 - left;
        if avail > 40.0 {
            let g = theme::elided(&ui, &origin, theme::sans(12.0), pal.faint, avail);
            ui.painter().galley(
                Pos2::new(left, rect.center().y - g.size().y / 2.0),
                g,
                pal.faint,
            );
        }
    }

    fn footer_ui(&mut self, root: &mut egui::Ui, ctx: &egui::Context, pal: &Palette, rect: Rect) {
        root.painter().rect_filled(rect, 0.0, pal.elev);
        let paused = self.shared.paused.load(Ordering::Relaxed);
        let enter = if self.auto_paste { "paste" } else { "copy" };

        let mut ui = root.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    rect.min + Vec2::new(20.0, 0.0),
                    rect.max - Vec2::new(14.0, 0.0),
                ))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        ui.spacing_mut().item_spacing.x = 16.0;
        theme::hint(&mut ui, pal, &["↑", "↓"], "Navigate");
        theme::hint(&mut ui, pal, &["⌘1–9"], &format!("Quick {enter}"));
        if self.auto_paste {
            theme::hint(&mut ui, pal, &["⇧↵"], "Copy only");
        }
        theme::hint(&mut ui, pal, &["esc"], "Close");

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let gear = theme::icon_button(ui, pal, Icon::Gear, 26.0, false).on_hover_text("Settings");
            egui::Popup::menu(&gear).width(220.0).show(|ui| {
                ui.style_mut().override_font_id = Some(theme::sans(13.0));
                let pause_label = if paused {
                    "Resume recording"
                } else {
                    "Pause recording"
                };
                if ui.button(pause_label).clicked() {
                    self.set_paused(!paused);
                }
                if self.auto_paste && !self.ax_trusted && ui.button("Enable auto-paste…").clicked()
                {
                    self.ax_trusted = platform::accessibility_trusted(true);
                }
                if ui.button("Clear unpinned history").clicked() {
                    let _ = self.store.clear_unpinned();
                    self.reload = true;
                    self.done("Cleared history. Pinned items kept.");
                }
                if ui.button("Open settings file").clicked() {
                    platform::reveal(&self.config_path);
                }
                ui.separator();
                if ui.button("Quit ClipVault").clicked() {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.label(
                RichText::new(format!("{} items", self.counts[0]))
                    .font(theme::sans(12.0))
                    .color(pal.faint),
            );
            if paused {
                theme::pill(
                    ui,
                    "Recording paused",
                    theme::medium(11.0),
                    pal.amber,
                    alpha(pal.amber, 0.16),
                    20.0,
                );
            } else if self.auto_paste && !self.ax_trusted {
                let label = RichText::new("Enable auto-paste")
                    .font(theme::medium(12.0))
                    .color(pal.accent_ink);
                let r = ui
                    .add(egui::Label::new(label).sense(Sense::click()))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(
                        "Opens the Accessibility permission prompt. Until it's allowed, Enter only copies.",
                    );
                if r.clicked() {
                    self.ax_trusted = platform::accessibility_trusted(true);
                }
            }
        });
    }

    fn toast_ui(&mut self, root: &mut egui::Ui, pal: &Palette, area: Rect) {
        let Some((toast, _)) = &self.toast else {
            return;
        };
        let (msg, ok) = match toast {
            Toast::Done(m) => (m.as_str(), true),
            Toast::Warn(m) => (m.as_str(), false),
        };
        let g = theme::elided(root, msg, theme::sans(13.0), pal.text, area.width() - 120.0);
        let size = Vec2::new(10.0 + 22.0 + 10.0 + g.size().x + 14.0, 38.0);
        let rect = Rect::from_center_size(
            Pos2::new(area.center().x, area.bottom() - 20.0 - 19.0),
            size,
        );
        let p = root.painter();
        let shadow = egui::Shadow {
            offset: [0, 12],
            blur: 30,
            spread: 0,
            color: Color32::from_black_alpha(if pal.dark { 90 } else { 40 }),
        };
        p.add(shadow.as_shape(rect, 19.0));
        p.rect(
            rect,
            19.0,
            pal.panel_solid,
            Stroke::new(1.0, pal.line),
            StrokeKind::Inside,
        );
        let dot = Rect::from_center_size(
            Pos2::new(rect.left() + 21.0, rect.center().y),
            Vec2::splat(22.0),
        );
        let (bg, which) = if ok {
            (pal.accent, Icon::Check)
        } else {
            (pal.amber, Icon::Alert)
        };
        p.circle_filled(dot.center(), 11.0, bg);
        theme::icon(p, dot.shrink(5.0), which, pal.accent_text, 3.0);
        p.galley(
            Pos2::new(dot.right() + 10.0, rect.center().y - g.size().y / 2.0),
            g,
            pal.text,
        );
    }
}

// ---------- free helpers ----------

fn group_header(ui: &mut egui::Ui, pal: &Palette, label: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::hover());
    let job = egui::text::LayoutJob::single_section(
        label.to_uppercase(),
        egui::TextFormat {
            font_id: theme::semibold(11.0),
            color: pal.faint,
            extra_letter_spacing: 0.88,
            ..Default::default()
        },
    );
    let g = ui.painter().layout_job(job);
    let (gw, gh) = (g.size().x, g.size().y);
    let y = rect.bottom() - 6.0 - gh;
    let x = rect.left() + 10.0;
    ui.painter().galley(Pos2::new(x, y), g, pal.faint);
    let lx = x + gw + 8.0;
    if lx < rect.right() - 10.0 {
        ui.painter().hline(
            lx..=rect.right() - 10.0,
            y + gh / 2.0,
            Stroke::new(1.0, pal.line),
        );
    }
}

fn tile_icon(p: &egui::Painter, pal: &Palette, tile: Rect, kind: RowKind) {
    p.rect_filled(tile, 10.0, pal.elev2);
    let ir = Rect::from_center_size(tile.center(), Vec2::splat(17.0));
    theme::icon(p, ir, kind.icon(), kind.tint(pal), 2.0);
}

fn wrapped_mono(ui: &mut egui::Ui, text: &str, size: f32, color: Color32) {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_string(),
        egui::TextFormat::simple(theme::mono(size), color),
    );
    job.wrap.max_width = ui.available_width();
    job.wrap.break_anywhere = true;
    ui.add(egui::Label::new(job).selectable(true));
}

fn files_preview(ui: &mut egui::Ui, pal: &Palette, clip: &Clip) {
    ui.spacing_mut().item_spacing.y = 8.0;
    for f in &clip.files {
        file_card(ui, pal, f);
    }
}

fn file_card(ui: &mut egui::Ui, pal: &Palette, f: &FileEntry) {
    egui::Frame::new()
        .fill(pal.elev)
        .stroke(Stroke::new(1.0, pal.line))
        .corner_radius(12)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                // extension badge
                let (badge, _) = ui.allocate_exact_size(Vec2::new(40.0, 48.0), Sense::hover());
                ui.painter().rect_filled(badge, 7.0, pal.elev2);
                let ext: String = if f.is_dir {
                    "DIR".into()
                } else {
                    std::path::Path::new(&f.name)
                        .extension()
                        .map(|e| e.to_string_lossy().to_uppercase().chars().take(4).collect())
                        .unwrap_or_default()
                };
                let eg = ui
                    .painter()
                    .layout_no_wrap(ext, theme::mono_medium(9.5), pal.amber);
                let at = Pos2::new(
                    badge.center().x - eg.size().x / 2.0,
                    badge.bottom() - 7.0 - eg.size().y,
                );
                ui.painter().galley(at, eg, pal.amber);

                let usable = f.usable_path();
                let text_w = ui.available_width() - 30.0 - 12.0;
                ui.allocate_ui_with_layout(
                    Vec2::new(text_w, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(text_w);
                        ui.spacing_mut().item_spacing.y = 4.0;
                        let mut job = egui::text::LayoutJob::single_section(
                            f.name.clone(),
                            egui::TextFormat::simple(theme::semibold(13.0), pal.text),
                        );
                        job.wrap.max_width = text_w;
                        job.wrap.break_anywhere = true;
                        ui.label(job);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            ui.label(
                                RichText::new(human_size(f.size))
                                    .font(theme::sans(12.0))
                                    .color(pal.muted),
                            );
                            let (text, color) = match (&f.stored, &usable) {
                                (Some(_), Some(_)) => ("Saved in vault", pal.green),
                                (None, Some(_)) => ("Linked · too large to copy", pal.amber),
                                (_, None) => ("No longer available", pal.red),
                            };
                            theme::pill(
                                ui,
                                text,
                                theme::medium(11.0),
                                color,
                                alpha(color, 0.16),
                                20.0,
                            );
                        });
                        wrapped_mono(ui, &tilde(&f.original), 11.0, pal.faint);
                    },
                );
                if let Some(path) = usable {
                    let finder = theme::icon_button(ui, pal, Icon::External, 30.0, true)
                        .on_hover_text("Show in Finder");
                    if finder.clicked() {
                        platform::reveal(&path);
                    }
                }
            });
        });
}

fn load_texture(ctx: &egui::Context, name: &str, img: image::DynamicImage) -> TextureHandle {
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let ci = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    ctx.load_texture(name, ci, egui::TextureOptions::LINEAR)
}

/// UVs that crop a texture to fill a square ("object-fit: cover").
fn cover_uv(size: Vec2) -> Rect {
    if size.x > size.y {
        let f = size.y / size.x;
        Rect::from_min_max(
            Pos2::new((1.0 - f) / 2.0, 0.0),
            Pos2::new((1.0 + f) / 2.0, 1.0),
        )
    } else {
        let f = size.x / size.y.max(1.0);
        Rect::from_min_max(
            Pos2::new(0.0, (1.0 - f) / 2.0),
            Pos2::new(1.0, (1.0 + f) / 2.0),
        )
    }
}

/// `/Users/me/Downloads/x` → `~/Downloads/x`.
fn tilde(path: &str) -> String {
    let Some(home) = dirs::home_dir() else {
        return path.to_string();
    };
    let home = home.to_string_lossy();
    match path.strip_prefix(home.as_ref()) {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_string(),
    }
}

fn human_size(b: u64) -> String {
    const U: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else {
        format!("{v:.1} {}", U[i])
    }
}

fn pretty_hotkey(spec: &str) -> String {
    spec.split('+')
        .map(|t| match t.trim().to_lowercase().as_str() {
            "cmd" | "command" | "super" | "meta" => "⌘".to_string(),
            "shift" => "⇧".to_string(),
            "alt" | "option" => "⌥".to_string(),
            "ctrl" | "control" => "⌃".to_string(),
            other => other
                .strip_prefix("key")
                .or_else(|| other.strip_prefix("digit"))
                .unwrap_or(other)
                .to_uppercase(),
        })
        .collect()
}
