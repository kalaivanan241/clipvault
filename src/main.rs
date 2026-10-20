mod app;
mod classify;
mod config;
mod hotkey;
mod platform;
mod store;
mod theme;
#[cfg(target_os = "macos")]
mod tray;
mod watcher;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let dir = config::data_dir();
    let cfg = config::load(&dir);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ClipVault")
            .with_inner_size([860.0, 560.0])
            .with_min_inner_size([680.0, 440.0])
            .with_always_on_top()
            // Palette style: no visible title bar; the search header is the drag handle.
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
            .with_titlebar_buttons_shown(false),
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        "ClipVault",
        options,
        Box::new(move |cc| Ok(Box::new(app::ClipApp::new(cc, dir, cfg)?))),
    )
}
