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
            username: String::new(),
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

/// Pre-rename data directory (the app was called "wallb" before adopting
/// the "Wallboxd" Store identity). Only ever read from, never written to.
fn legacy_data_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").expect("LOCALAPPDATA not set");
    PathBuf::from(base).join("wallb")
}

fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

/// One-time carry-over from the pre-rename `%LOCALAPPDATA%\wallb` directory,
/// so upgrading from "wallb" to "Wallboxd" doesn't drop the user's username/
/// settings or force a re-download of the current wallpaper before the first
/// refresh. Only runs when the new location has no config yet; the old
/// directory is left untouched (never deleted) since it may still belong to
/// an old install the user hasn't uninstalled.
pub fn migrate_from_legacy() {
    if config_path().exists() {
        return;
    }
    let legacy = legacy_data_dir();
    if !legacy.join("config.json").exists() {
        return;
    }
    let _ = std::fs::create_dir_all(data_dir());
    for file in ["config.json", "wallpaper.jpg", "lockscreen.jpg"] {
        let _ = std::fs::copy(legacy.join(file), data_dir().join(file));
    }
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
