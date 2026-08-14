use crate::config::{Config, WallpaperStyle};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use windows::core::w;
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_SZ,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SystemParametersInfoW, SM_CXSCREEN, SM_CYSCREEN, SPIF_SENDCHANGE,
    SPIF_UPDATEINIFILE, SPI_SETDESKWALLPAPER,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

pub fn screen_size() -> (i32, i32) {
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

pub fn cache_path() -> PathBuf {
    crate::config::data_dir().join("wallpaper.jpg")
}

fn http_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}

/// Fetches the current wallpaper.jpg for `cfg.username` sized to the primary
/// monitor and applies it. On any failure the existing cached file (and
/// whatever Windows currently has set) is left untouched — that's the whole
/// offline story, no separate fallback path needed.
pub fn refresh(cfg: &Config) -> Result<(), String> {
    let (w, h) = screen_size();
    let mut url = format!(
        "https://lb.sayed.app/api/wallpaper.jpg?username={}&w={}&h={}",
        urlencode(&cfg.username),
        w,
        h
    );
    if let Some(count) = cfg.poster_count {
        url.push_str(&format!("&count={count}"));
    }

    let bytes = http_client()
        .get(&url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .bytes()
        .map_err(|e| e.to_string())?;

    let path = cache_path();
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;

    set_wallpaper_file(&path, cfg.style)
}

/// Re-applies whatever is already cached, without hitting the network.
/// Used at startup so a stale-but-present wallpaper is guaranteed set even
/// before the first refresh completes.
pub fn apply_cached(cfg: &Config) -> Result<(), String> {
    let path = cache_path();
    if path.exists() {
        set_wallpaper_file(&path, cfg.style)
    } else {
        Err("no cached wallpaper yet".into())
    }
}

fn set_wallpaper_file(path: &Path, style: WallpaperStyle) -> Result<(), String> {
    set_style_registry(style)?;

    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    unsafe {
        SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            Some(wide.as_mut_ptr().cast()),
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        )
    }
    .map_err(|e| e.to_string())
}

/// Sets the desktop background display mode (Fill/Fit/Stretch/Center/Tile/
/// Span) that Windows applies whenever a wallpaper is set. This has to be
/// written before `SPI_SETDESKWALLPAPER` — Windows reads it at apply time,
/// not the other way around.
fn set_style_registry(style: WallpaperStyle) -> Result<(), String> {
    let (style_val, tile_val) = style.registry_values();
    let mut hkey = HKEY::default();
    unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Control Panel\\Desktop"),
            None,
            KEY_WRITE,
            &mut hkey,
        )
        .map_err(|e| e.to_string())?;

        let mut style_wide: Vec<u16> = style_val.encode_utf16().chain(std::iter::once(0)).collect();
        let style_bytes =
            std::slice::from_raw_parts(style_wide.as_mut_ptr().cast::<u8>(), style_wide.len() * 2);
        let r1 = RegSetValueExW(hkey, w!("WallpaperStyle"), None, REG_SZ, Some(style_bytes));

        let mut tile_wide: Vec<u16> = tile_val.encode_utf16().chain(std::iter::once(0)).collect();
        let tile_bytes =
            std::slice::from_raw_parts(tile_wide.as_mut_ptr().cast::<u8>(), tile_wide.len() * 2);
        let r2 = RegSetValueExW(hkey, w!("TileWallpaper"), None, REG_SZ, Some(tile_bytes));

        let _ = RegCloseKey(hkey);
        r1.map_err(|e| e.to_string())?;
        r2.map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Letterboxd usernames are alphanumeric/underscore/hyphen, but percent-encode
/// defensively anyway since this goes straight into a query string.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else {
            for b in c.to_string().as_bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}
