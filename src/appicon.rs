use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{HICON, IDI_APPLICATION, LoadIconW};
use windows::core::PCWSTR;

/// Windows resource ID the app icon is embedded under (`build.rs`,
/// `res.set_icon_with_id("assets/icon.ico", "1")`) — the lowest-numbered
/// icon resource in the exe is also what Explorer/Task Manager/Alt-Tab show
/// for the process, so this one `.ico` covers every window/UI surface that
/// loads it via this function.
const APP_ICON_RESOURCE_ID: usize = 1;

/// Loads the app's own embedded icon (see `APP_ICON_RESOURCE_ID`) for use as
/// a window class icon, dialog icon, or tray icon. Falls back to the stock
/// `IDI_APPLICATION` icon rather than showing nothing if the embedded
/// resource can't be loaded for some reason.
pub fn load() -> HICON {
    unsafe {
        if let Ok(instance) = GetModuleHandleW(None)
            && let Ok(icon) = LoadIconW(
                Some(instance.into()),
                PCWSTR(APP_ICON_RESOURCE_ID as *const u16),
            )
        {
            return icon;
        }
        LoadIconW(None, IDI_APPLICATION).unwrap_or_default()
    }
}
