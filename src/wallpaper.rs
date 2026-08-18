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

/// The wall is generated in-process now (see `wallgen`), fetching Letterboxd's
/// diary RSS plus one image per poster tile — many small requests rather than
/// one, so a slow/flaky network can still make a single attempt take a while.
/// Give it room, and retry transient failures below.
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
        // Letterboxd and its poster CDN are third-party sites now (previously
        // only our own Cloudflare Worker was called) — identify the client
        // rather than sending reqwest's default UA.
        .user_agent(concat!("Wallboxd/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}

/// `on_progress(done, total)` fires as poster tiles finish fetching (see
/// `wallgen::generate`) — pass `&|_, _| {}` for a silent refresh.
pub fn refresh(cfg: &Config, on_progress: &(dyn Fn(u32, u32) + Sync)) -> Result<(), String> {
    let (w, h) = screen_size();
    let client = http_client();
    let main_path = cache_path();

    fetch_wallpaper(
        &cfg.username,
        w,
        h,
        cfg.poster_count,
        &main_path,
        &client,
        on_progress,
    )?;
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
            fetch_wallpaper(
                lock_user,
                w,
                h,
                cfg.poster_count,
                &path,
                &client,
                on_progress,
            )?;
            path
        };

        if let Err(e) = set_lockscreen_file(&lock_path) {
            crate::applog::error(&format!("failed to set lockscreen: {e}"));
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn fetch_wallpaper(
    username: &str,
    w: i32,
    h: i32,
    poster_count: Option<u32>,
    path: &Path,
    client: &reqwest::blocking::Client,
    on_progress: &(dyn Fn(u32, u32) + Sync),
) -> Result<(), String> {
    let mut last_err = String::new();
    for attempt in 0..MAX_ATTEMPTS {
        match crate::wallgen::generate(
            username,
            w as u32,
            h as u32,
            poster_count,
            client,
            on_progress,
        ) {
            Ok(jpeg_bytes) => {
                std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
                std::fs::write(path, &jpeg_bytes).map_err(|e| e.to_string())?;
                return Ok(());
            }
            Err(e) => {
                let retryable = e.is_retryable();
                last_err = e.to_string();
                if !retryable || attempt + 1 == MAX_ATTEMPTS {
                    return Err(last_err);
                }
                std::thread::sleep(RETRY_BACKOFF);
            }
        }
    }
    Err(last_err)
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

        if lock_path.exists()
            && let Err(e) = set_lockscreen_file(&lock_path)
        {
            crate::applog::error(&format!("failed to set lockscreen from cache: {e}"));
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

/// Sets the lock screen image.
///
/// Confirmed by live testing: without package (MSIX/APPX) identity, UWP lock
/// screen APIs decline unconditionally. With package identity present, the
/// modern `UserProfilePersonalizationSettings.TrySetLockScreenImageAsync`
/// still silently returned `Ok(false)` for this Desktop Bridge app — tested
/// across package identity, confirmed genuine "Picture" mode, no competing
/// lock-screen provider app, and a uniquely-named file, none of which helped
/// (see AGENTS.md's "Lock screen" section for the full history).
///
/// The fix: the older `LockScreen::SetImageFileAsync` API, which throws on
/// real failure instead of silently returning false, and — confirmed by
/// reading `LockScreen::OriginalImageFile()` back immediately after calling
/// it — actually applies the image. Keeping the unique-filename workaround
/// alongside it (harmless, and was already in place) rather than re-testing
/// its necessity with this specific API.
fn set_lockscreen_file(path: &Path) -> Result<(), String> {
    use std::future::IntoFuture;
    use windows::Storage::StorageFile;
    use windows::System::UserProfile::LockScreen;
    use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
    use windows::core::HSTRING;

    const APPMODEL_ERROR_NO_PACKAGE: u32 = 15700;
    let mut len: u32 = 0;
    if unsafe { GetCurrentPackageFullName(&mut len, None) }.0 == APPMODEL_ERROR_NO_PACKAGE {
        return Err(
            "Windows won't let Wallboxd set the lock screen image from this build: it has no \
            package (MSIX/APPX) identity, which Windows' lock screen API silently requires — \
            it declines with no error of its own, regardless of lock screen mode. This can't be \
            fixed from Settings; it needs Wallboxd to ship with package identity (the packaged \
            MSIX build does; a plain `cargo build` exe does not)."
                .to_string(),
        );
    }

    // Workaround for Windows UWP lock screen API quirk:
    // We copy the image to a uniquely named file first to ensure Windows treats it as new.
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let unique_name = format!("lockscreen_{timestamp}.jpg");
    let unique_path = path.with_file_name(&unique_name);

    if let Err(e) = std::fs::copy(path, &unique_path) {
        return Err(format!(
            "failed to copy lockscreen file for unique naming: {e}"
        ));
    }

    let path_str = unique_path.to_str().ok_or("invalid path")?;
    let hstring_path = HSTRING::from(path_str);

    let result = || -> windows::core::Result<()> {
        let file = futures::executor::block_on(
            StorageFile::GetFileFromPathAsync(&hstring_path)?.into_future(),
        )?;

        // Use the older LockScreen API, which successfully applies the image
        // for Desktop Bridge apps where UserProfilePersonalizationSettings fails.
        futures::executor::block_on(LockScreen::SetImageFileAsync(&file)?.into_future())?;

        Ok(())
    }();

    // Clean up old unique files, keeping only the newly created one.
    if let Some(dir) = path.parent()
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        for entry in entries.flatten() {
            let fname = entry.file_name();
            let fname_str = fname.to_string_lossy();
            if fname_str.starts_with("lockscreen_")
                && fname_str.ends_with(".jpg")
                && fname_str != unique_name
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    match result {
        Ok(_) => Ok(()),
        Err(e) => Err(format!(
            "Windows declined to set the lock screen image: {}. Make sure the lock screen \
            background is set to \"Picture\" in Settings > Personalization > Lock screen.",
            e
        )),
    }
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
