//! User configuration, stored as JSON next to the history database.
//! Edit `config.json` (see README) and restart the app to apply changes.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    /// Global shortcut that shows/hides the history window.
    /// Format: modifiers joined by '+', then a key, e.g. "cmd+shift+v",
    /// "ctrl+alt+KeyC", "cmd+shift+Digit9".
    pub hotkey: String,
    /// Unpinned items beyond this count are deleted, oldest first.
    pub max_items: usize,
    /// Copied files/folders up to this total size (MB) are duplicated into the
    /// vault; bigger ones are only remembered by their path.
    pub max_copy_mb: u64,
    /// How often the system clipboard is checked (milliseconds).
    pub poll_ms: u64,
    /// After you pick a clip, press ⌘V in the app you came from (needs
    /// Accessibility permission). When false, the clip is only copied.
    pub auto_paste: bool,
    /// Clips copied while one of these apps is in front are never recorded.
    /// Matches bundle ids ("com.1password.1password") or app names ("1Password").
    pub ignore_apps: Vec<String>,
    /// "system" (follow macOS), "dark" or "light".
    pub theme: String,
    /// Highlight colour as hex, e.g. "#c8f169" (lime), "#8b7cff", "#5ee6d0", "#ff9a62".
    pub accent: String,
    /// Menu-bar glyph from the design: "vault" (default), "stack" or "history".
    pub menu_bar_icon: String,
    /// Read the text in copied images on-device (Apple Vision) so screenshots
    /// are searchable. Nothing is sent anywhere.
    pub ocr: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            hotkey: "cmd+shift+v".to_string(),
            max_items: 1000,
            max_copy_mb: 100,
            poll_ms: 400,
            auto_paste: true,
            ignore_apps: [
                "com.1password.1password",
                "com.agilebits.onepassword7",
                "com.bitwarden.desktop",
                "com.lastpass.LastPass",
                "org.keepassxc.keepassxc",
                "com.apple.keychainaccess",
                "com.apple.Passwords",
            ]
            .map(String::from)
            .to_vec(),
            theme: "system".to_string(),
            accent: "#c8f169".to_string(),
            menu_bar_icon: "vault".to_string(),
            ocr: true,
        }
    }
}

impl Config {
    pub fn is_ignored(&self, app: &crate::platform::SourceApp) -> bool {
        self.ignore_apps
            .iter()
            .any(|a| a.eq_ignore_ascii_case(&app.bundle_id) || a.eq_ignore_ascii_case(&app.name))
    }
}

/// `~/Library/Application Support/ClipVault` on macOS.
pub fn data_dir() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("ClipVault")
}

pub fn load(dir: &Path) -> Config {
    let _ = fs::create_dir_all(dir);
    let path = dir.join("config.json");
    match fs::read_to_string(&path) {
        Ok(s) => match serde_json::from_str::<Config>(&s) {
            Ok(mut c) => {
                c.poll_ms = c.poll_ms.clamp(100, 5000);
                c.max_items = c.max_items.max(10);
                c
            }
            Err(e) => {
                eprintln!("config.json is invalid ({e}); using defaults");
                Config::default()
            }
        },
        Err(_) => {
            let c = Config::default();
            if let Ok(s) = serde_json::to_string_pretty(&c) {
                let _ = fs::write(&path, s);
            }
            c
        }
    }
}
