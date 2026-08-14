use crate::config::{Config, WallpaperStyle};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_SZ, RegCloseKey, RegOpenKeyExW, RegSetValueExW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN, SPI_SETDESKWALLPAPER, SPIF_SENDCHANGE,
    SPIF_UPDATEINIFILE, SystemParametersInfoW,
};
use windows::core::w;

/// The posterboi API has a cold start — it pulls from Letterboxd via a
/// subprocess on the server — so the first request after idle can take far
/// longer than the normal 3-10s generation. Give it room, and retry transient
/// failures below.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_ATTEMPTS: u32 = 3;
const RETRY_BACKOFF: Duration = Duration::from_secs(5);

pub fn screen_size() -> (i32, i32) {
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

pub fn cache_path() -> PathBuf {
    crate::config::data_dir().join("wallpaper.jpg")
}

pub fn lockscreen_cache_path() -> PathBuf {
    crate::config::data_dir().join("lockscreen.jpg")
}

fn http_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}

pub fn refresh(cfg: &Config) -> Result<(), String> {
    let (w, h) = screen_size();
    let client = http_client();
    let main_path = cache_path();
    
    fetch_wallpaper(&cfg.username, w, h, cfg.poster_count, &main_path, &client)?;
    set_wallpaper_file(&main_path, cfg.style)?;

    if cfg.set_lockscreen {
        let lock_user = if cfg.lockscreen_username.is_empty() {
            &cfg.username
        } else {
            &cfg.lockscreen_username
        };
        
        let lock_path = if lock_user == &cfg.username {
            main_path
        } else {
            let path = lockscreen_cache_path();
            fetch_wallpaper(lock_user, w, h, cfg.poster_count, &path, &client)?;
            path
        };
        
        if let Err(e) = set_lockscreen_file(&lock_path) {
            crate::applog::error(&format!("failed to set lockscreen: {e}"));
        }
    }
    
    Ok(())
}

fn fetch_wallpaper(
    username: &str,
    w: i32,
    h: i32,
    poster_count: Option<u32>,
    path: &Path,
    client: &reqwest::blocking::Client,
) -> Result<(), String> {
    let mut url = format!(
        "https://lb.sayed.app/api/wallpaper.jpg?username={}&w={}&h={}",
        urlencode(username),
        w,
        h
    );
    if let Some(count) = poster_count {
        url.push_str(&format!("&count={count}"));
    }

    let mut last_err = String::new();
    for attempt in 0..MAX_ATTEMPTS {
        let result = client
            .get(&url)
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.bytes());
        match result {
            Ok(bytes) => {
                std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
                std::fs::write(path, &bytes).map_err(|e| e.to_string())?;
                return Ok(());
            }
            Err(e) => {
                last_err = e.to_string();
                if !is_retryable(&e) || attempt + 1 == MAX_ATTEMPTS {
                    return Err(last_err);
                }
                std::thread::sleep(RETRY_BACKOFF);
            }
        }
    }
    Err(last_err)
}

/// Transient failures — server cold start (5xx), timeouts, connection drops —
/// are worth retrying; 4xx client errors are permanent and won't fix
/// themselves.
fn is_retryable(e: &reqwest::Error) -> bool {
    if e.is_timeout() || e.is_connect() || e.is_request() {
        return true;
    }
    e.status().is_some_and(|s| s.is_server_error())
}

pub fn apply_cached(cfg: &Config) -> Result<(), String> {
    let main_path = cache_path();
    if main_path.exists() {
        set_wallpaper_file(&main_path, cfg.style)?;
    } else {
        return Err("no cached wallpaper yet".into());
    }

    if cfg.set_lockscreen {
        let lock_user = if cfg.lockscreen_username.is_empty() {
            &cfg.username
        } else {
            &cfg.lockscreen_username
        };
        
        let lock_path = if lock_user == &cfg.username {
            main_path
        } else {
            lockscreen_cache_path()
        };
        
        if lock_path.exists() {
            if let Err(e) = set_lockscreen_file(&lock_path) {
                crate::applog::error(&format!("failed to set lockscreen from cache: {e}"));
            }
        }
    }
    Ok(())
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

fn set_lockscreen_file(path: &Path) -> Result<(), String> {
    use windows::System::UserProfile::UserProfilePersonalizationSettings;
    use windows::Storage::StorageFile;
    use windows::core::HSTRING;
    use std::future::IntoFuture;
    
    let path_str = path.to_str().ok_or("invalid path")?;
    let hstring_path = HSTRING::from(path_str);
    
    let result = || -> windows::core::Result<()> {
        if !UserProfilePersonalizationSettings::IsSupported()? {
            return Ok(());
        }
        let settings = UserProfilePersonalizationSettings::Current()?;
        let file = futures::executor::block_on(StorageFile::GetFileFromPathAsync(&hstring_path)?.into_future())?;
        futures::executor::block_on(settings.TrySetLockScreenImageAsync(&file)?.into_future())?;
        Ok(())
    }();
    
    result.map_err(|e| e.to_string())
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
        // RegOpenKeyExW/RegSetValueExW return a raw WIN32_ERROR; .ok()
        // converts it to Result<(), Error>.
        .ok()
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
        r1.ok().map_err(|e| e.to_string())?;
        r2.ok().map_err(|e| e.to_string())?;
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
