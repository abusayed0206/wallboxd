use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
pub enum WallpaperStyle {
    #[default]
    Fill,
    Fit,
    Stretch,
    Center,
    Tile,
    Span,
}

impl WallpaperStyle {
    pub const ALL: [WallpaperStyle; 6] = [
        WallpaperStyle::Fill,
        WallpaperStyle::Fit,
        WallpaperStyle::Stretch,
        WallpaperStyle::Center,
        WallpaperStyle::Tile,
        WallpaperStyle::Span,
    ];

    pub fn label(self) -> &'static str {
        match self {
            WallpaperStyle::Fill => "Fill",
            WallpaperStyle::Fit => "Fit",
            WallpaperStyle::Stretch => "Stretch",
            WallpaperStyle::Center => "Center",
            WallpaperStyle::Tile => "Tile",
            WallpaperStyle::Span => "Span",
        }
    }

    /// (WallpaperStyle, TileWallpaper) values under
    /// HKCU\Control Panel\Desktop, per the documented Windows registry
    /// encoding for desktop background display modes.
    pub fn registry_values(self) -> (&'static str, &'static str) {
        match self {
            WallpaperStyle::Fill => ("10", "0"),
            WallpaperStyle::Fit => ("6", "0"),
            WallpaperStyle::Stretch => ("2", "0"),
            WallpaperStyle::Center => ("0", "0"),
            WallpaperStyle::Tile => ("0", "1"),
            WallpaperStyle::Span => ("22", "0"),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    /// Master switch: when false, nothing runs automatically (startup,
    /// timer, or post-settings-save refresh all skip) even if `username` is
    /// set. Defaults to false so a fresh install does nothing until the user
    /// explicitly opts in from Settings, with no wallpaper changes without
    /// asking first. Manual actions (tray "Refresh now", the Settings
    /// dialog's own "Refresh Now" button) intentionally bypass this, since
    /// clicking them is itself the explicit consent.
    #[serde(default)]
    pub enabled: bool,
    pub username: String,
    pub interval_minutes: u32,
    pub autostart: bool,
    #[serde(default)]
    pub poster_count: Option<u32>,
    #[serde(default)]
    pub style: WallpaperStyle,
    #[serde(default)]
    pub set_lockscreen: bool,
    #[serde(default)]
    pub lockscreen_username: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            username: "abusayed".to_string(),
            interval_minutes: 60,
            autostart: true,
            poster_count: None,
            style: WallpaperStyle::default(),
            set_lockscreen: false,
            lockscreen_username: String::new(),
        }
    }
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").expect("LOCALAPPDATA not set");
    PathBuf::from(base).join("Wallboxd")
}

fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

impl Config {
    pub fn load() -> Self {
        std::fs::read_to_string(config_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(data_dir())?;
        std::fs::write(config_path(), serde_json::to_string_pretty(self)?)
    }
}
