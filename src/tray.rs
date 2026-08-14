use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, HICON, HMENU, IDI_APPLICATION, LoadIconW, MF_STRING,
    SetForegroundWindow, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TrackPopupMenu,
};
use windows::core::PCWSTR;

pub const WM_TRAYICON: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 1;
pub const TRAY_ID: u32 = 1;

pub const ID_REFRESH: u32 = 1001;
pub const ID_SETTINGS: u32 = 1002;
pub const ID_EXIT: u32 = 1003;

fn app_icon() -> HICON {
    // ponytail: stock system icon, no .ico resource to build/embed. Swap in a
    // branded icon via LoadImageW(GetModuleHandle, resource id) if it matters.
    unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() }
}

fn wide_tip(text: &str) -> [u16; 128] {
    let mut buf = [0u16; 128];
    for (dst, src) in buf.iter_mut().zip(text.encode_utf16().take(127)) {
        *dst = src;
    }
    buf
}

fn base_nid(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW::default();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_ID;
    nid
}

pub fn add(hwnd: HWND, tooltip: &str) {
    let mut nid = base_nid(hwnd);
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = WM_TRAYICON;
    nid.hIcon = app_icon();
    nid.szTip = wide_tip(tooltip);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
    }
}

pub fn set_tooltip(hwnd: HWND, tooltip: &str) {
    let mut nid = base_nid(hwnd);
    nid.uFlags = NIF_TIP;
    nid.szTip = wide_tip(tooltip);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

pub fn remove(hwnd: HWND) {
    let nid = base_nid(hwnd);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

pub fn show_menu(hwnd: HWND, x: i32, y: i32) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            ID_REFRESH as usize,
            PCWSTR::from_raw(w("Refresh now").as_ptr()),
        );
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            ID_SETTINGS as usize,
            PCWSTR::from_raw(w("Settings...").as_ptr()),
        );
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            ID_EXIT as usize,
            PCWSTR::from_raw(w("Exit").as_ptr()),
        );

        // Popup menus need the owner window in the foreground or they won't
        // dismiss on outside click; standard tray-icon dance.
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_BOTTOMALIGN,
            x,
            y,
            Some(0),
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
    }
}

/// Leaks a null-terminated UTF-16 buffer for the lifetime of the call —
/// menu labels are tiny and built once per click, not worth pooling.
fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
