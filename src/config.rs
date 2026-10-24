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
    /// Parses a config file's contents, tolerating a UTF-8 BOM (Notepad and
    /// PowerShell's `Set-Content -Encoding utf8` both write one, and serde
    /// rejects the whole file over it) and treating a pre-master-switch config
    /// as already opted in (see `load`).
    fn parse(raw: &str) -> Option<Self> {
        let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
        let mut cfg = serde_json::from_str::<Self>(raw).ok()?;
        // Back-compat: a config.json saved before the `enabled` master
        // switch was introduced has no "enabled" key, so `#[serde(default)]`
        // silently resolves it to `false` - identical to a fresh install
        // even though this user already had Wallboxd actively configured
        // (possibly with set_lockscreen/username/etc. already set). Without
        // this, every existing install goes silently inert on upgrade: no
        // startup apply, no timer refresh, no post-settings-save refresh,
        // and no error anywhere - it just looks like the wallpaper/lock
        // screen stopped updating. Detect "key genuinely absent" (not just
        // `false`) via a raw JSON parse and treat that case as already
        // opted in, since an existing config file is itself evidence of
        // prior active use.
        if let Ok(serde_json::Value::Object(map)) = serde_json::from_str(raw)
            && !map.contains_key("enabled")
        {
            cfg.enabled = true;
        }
        Some(cfg)
    }

    pub fn load() -> Self {
        let Some(raw) = std::fs::read_to_string(config_path()).ok() else {
            // No config file at all: a genuine fresh install, so `enabled`
            // stays at its intended false default.
            return Self::default();
        };
        Self::parse(&raw).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(data_dir())?;
        std::fs::write(config_path(), serde_json::to_string_pretty(self)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tolerates_bom_and_treats_legacy_config_as_enabled() {
        // A BOM is not valid JSON; before this was tolerated, a config file
        // re-saved by Notepad/PowerShell silently reset every setting.
        let raw = "\u{feff}{\"username\":\"abusayed\",\"interval_minutes\":60,\"autostart\":false}";
        let cfg = Config::parse(raw).expect("BOM-prefixed config should parse");
        assert_eq!(cfg.username, "abusayed");
        assert!(
            cfg.enabled,
            "a config without the 'enabled' key is legacy and must stay active"
        );
    }

    #[test]
    fn parse_respects_explicit_disabled() {
        let raw =
            "{\"enabled\":false,\"username\":\"x\",\"interval_minutes\":60,\"autostart\":false}";
        let cfg = Config::parse(raw).expect("config should parse");
        assert!(!cfg.enabled, "an explicit false must not be overridden");
    }
}
