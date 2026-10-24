use crate::applog;
use std::cell::{Cell, RefCell};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    ICC_PROGRESS_CLASS, INITCOMMONCONTROLSEX, InitCommonControlsEx, PBM_GETPOS, PBM_SETPOS,
    PBM_SETRANGE32, PROGRESS_CLASS,
};
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem};
use windows::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, GetSystemMetrics, HMENU,
    KillTimer, RegisterClassW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW, SWP_NOSIZE, SWP_NOZORDER,
    SendMessageW, SetTimer, SetWindowPos, SetWindowTextW, ShowWindow, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_SETFONT, WM_TIMER, WNDCLASSW,
    WS_CAPTION, WS_CHILD, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

/// Posted by the refresh thread to the progress dialog when the refresh
/// finishes. wParam != 0 means failure, in which case lParam points to a
/// boxed error String that the dialog owns.
pub const WM_REFRESH_DONE: u32 = WM_APP + 2;

/// Posted (possibly many times, from any of the poster-fetch worker threads —
/// see `wallgen::fetch_all_posters`) as poster tiles finish downloading.
/// wParam = tiles done so far, lParam = total tiles. Switches the bar from
/// its indeterminate startup animation to a real determinate progress bar.
pub const WM_PROGRESS_UPDATE: u32 = WM_APP + 6;

const TIMER_ANIMATE: usize = 1;
const ID_BTN_OK: usize = 3001;
const PROGRESS_STEP: i32 = 5;

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    // (progress bar, status label, OK button)
    static CTRLS: RefCell<Option<(HWND, HWND, HWND)>> = const { RefCell::new(None) };
    // Set once the first real WM_PROGRESS_UPDATE arrives, so the indeterminate
    // animation timer stops fighting over the bar's position with real data.
    static REAL_PROGRESS: Cell<bool> = const { Cell::new(false) };
    // Highest (done, total) seen so far — guards against the bar visibly
    // jumping backward if updates from different worker threads arrive out
    // of order.
    static LAST_PROGRESS: Cell<(u32, u32)> = const { Cell::new((0, 0)) };
    /// DPI captured when the dialog opens; control rectangles below are in
    /// 96-DPI design units and go through `s()`.
    static DPI: Cell<u32> = const { Cell::new(96) };
}

/// Scales a 96-DPI design-unit coordinate to physical pixels.
fn s(value: i32) -> i32 {
    crate::uifont::scale(DPI.with(|d| d.get()), value)
}

pub fn is_active() -> bool {
    ACTIVE.with(|a| a.get())
}

fn get_font() -> WPARAM {
    crate::uifont::message_font(DPI.with(|d| d.get()))
}

/// Shows the progress dialog and returns its HWND so the caller can hand it
/// to the refresh thread. Non-modal: the main message loop keeps running and
/// the dialog closes itself when the refresh thread posts WM_REFRESH_DONE.
pub fn show(owner: HWND) -> HWND {
    register_class_once();
    ACTIVE.set(true);
    REAL_PROGRESS.with(|r| r.set(false));
    LAST_PROGRESS.with(|p| p.set((0, 0)));
    unsafe {
        let instance = GetModuleHandleW(None).unwrap();
        // Wider than the original 300: refresh errors ("Refresh failed: ...")
        // are one long sentence, and a 276-unit single-line label clipped them
        // after a few words, hiding the actual cause from the user.
        const DLG_W: i32 = 360;
        const DLG_H: i32 = 120;
        let dpi = GetDpiForSystem();
        DPI.with(|d| d.set(dpi));

        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
        let mut frame = RECT {
            left: 0,
            top: 0,
            right: s(DLG_W),
            bottom: s(DLG_H),
        };
        let _ = AdjustWindowRectExForDpi(&mut frame, style, false, WINDOW_EX_STYLE(0), dpi);
        let win_w = frame.right - frame.left;
        let win_h = frame.bottom - frame.top;

        let dlg = CreateWindowExW(
            Default::default(),
            class_name(),
            w!("Wallboxd"),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            win_w,
            win_h,
            Some(owner),
            None,
            Some(instance.into()),
            None,
        )
        .unwrap();

        let scr_w = GetSystemMetrics(SM_CXSCREEN);
        let scr_h = GetSystemMetrics(SM_CYSCREEN);
        let _ = SetWindowPos(
            dlg,
            Some(HWND::default()),
            (scr_w - win_w) / 2,
            (scr_h - win_h) / 2,
            0,
            0,
            SWP_NOZORDER | SWP_NOSIZE,
        );

        let label = CreateWindowExW(
            Default::default(),
            w!("STATIC"),
            w!("Refreshing wallpaper..."),
            WS_CHILD | WS_VISIBLE,
            s(12),
            s(8),
            s(336),
            s(36),
            Some(dlg),
            None,
            Some(instance.into()),
            None,
        )
        .unwrap();

        let bar = CreateWindowExW(
            Default::default(),
            PROGRESS_CLASS,
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE,
            s(12),
            s(50),
            s(336),
            s(18),
            Some(dlg),
            None,
            Some(instance.into()),
            None,
        )
        .unwrap();

        // Hidden until a refresh fails; shown alongside the error text.
        let btn_ok = CreateWindowExW(
            Default::default(),
            w!("BUTTON"),
            w!("OK"),
            WINDOW_STYLE(0) | WS_CHILD | WS_TABSTOP,
            s(140),
            s(76),
            s(80),
            s(24),
            Some(dlg),
            Some(HMENU(ID_BTN_OK as *mut _)),
            Some(instance.into()),
            None,
        )
        .unwrap();

        let _ = SendMessageW(label, WM_SETFONT, Some(get_font()), Some(LPARAM(0)));
        let _ = SendMessageW(btn_ok, WM_SETFONT, Some(get_font()), Some(LPARAM(0)));

        CTRLS.with(|c| *c.borrow_mut() = Some((bar, label, btn_ok)));

        let _ = SetTimer(Some(dlg), TIMER_ANIMATE, 100, None);
        let _ = ShowWindow(dlg, SW_SHOW);
        dlg
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // See main.rs's wndproc for why this needs catch_unwind: unwinding across
    // an extern "system" boundary aborts the whole process regardless of
    // panic=unwind, taking the tray icon down with it for what would
    // otherwise be a single bad message.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wndproc_inner(hwnd, msg, wparam, lparam)
    }))
    .unwrap_or_else(|_| {
        applog::error("panic in progress window procedure - message was not handled");
        LRESULT(0)
    })
}

fn wndproc_inner(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            if wparam.0 == TIMER_ANIMATE && !REAL_PROGRESS.with(|r| r.get()) {
                CTRLS.with(|c| unsafe {
                    if let Some((bar, _, _)) = c.borrow().as_ref() {
                        let pos = SendMessageW(*bar, PBM_GETPOS, None, None).0 as i32;
                        let next = if pos + PROGRESS_STEP > 100 {
                            0
                        } else {
                            pos + PROGRESS_STEP
                        };
                        let _ = SendMessageW(*bar, PBM_SETPOS, Some(WPARAM(next as usize)), None);
                    }
                });
            }
            LRESULT(0)
        }
        WM_PROGRESS_UPDATE => {
            let done = wparam.0 as u32;
            let total = (lparam.0 as u32).max(1);
            let is_newer = LAST_PROGRESS.with(|p| {
                let (last_done, last_total) = p.get();
                if total != last_total || done > last_done {
                    p.set((done, total));
                    true
                } else {
                    false
                }
            });
            if is_newer {
                REAL_PROGRESS.with(|r| r.set(true));
                CTRLS.with(|c| unsafe {
                    if let Some((bar, label, _)) = c.borrow().as_ref() {
                        let _ = SendMessageW(
                            *bar,
                            PBM_SETRANGE32,
                            Some(WPARAM(0)),
                            Some(LPARAM(total as isize)),
                        );
                        let _ = SendMessageW(*bar, PBM_SETPOS, Some(WPARAM(done as usize)), None);

                        let text = format!("Fetching posters ({done}/{total})...");
                        let wide: Vec<u16> =
                            text.encode_utf16().chain(std::iter::once(0)).collect();
                        let _ = SetWindowTextW(*label, PCWSTR::from_raw(wide.as_ptr()));
                    }
                });
            }
            LRESULT(0)
        }
        WM_REFRESH_DONE => {
            let _ = unsafe { KillTimer(Some(hwnd), TIMER_ANIMATE) };
            if wparam.0 != 0 {
                let err = unsafe { Box::from_raw(lparam.0 as *mut String) };
                applog::error(&format!("refresh failed: {err}"));
                CTRLS.with(|c| unsafe {
                    if let Some((_, label, btn)) = c.borrow().as_ref() {
                        let text = format!("Refresh failed: {err}");
                        let wide: Vec<u16> =
                            text.encode_utf16().chain(std::iter::once(0)).collect();
                        let _ = SetWindowTextW(*label, PCWSTR::from_raw(wide.as_ptr()));
                        let _ = ShowWindow(*btn, SW_SHOW);
                    }
                });
            } else {
                applog::info("wallpaper refreshed");
                ACTIVE.set(false);
                let _ = unsafe { DestroyWindow(hwnd) };
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = wparam.0 & 0xFFFF;
            if id == ID_BTN_OK {
                ACTIVE.set(false);
                let _ = unsafe { DestroyWindow(hwnd) };
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            // Closing the dialog doesn't cancel the refresh; the thread's
            // PostMessage to the destroyed window just fails and it logs.
            ACTIVE.set(false);
            let _ = unsafe { KillTimer(Some(hwnd), TIMER_ANIMATE) };
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_DESTROY => {
            ACTIVE.set(false);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn class_name() -> PCWSTR {
    w!("WallboxdProgressWindow")
}

fn register_class_once() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let icc = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_PROGRESS_CLASS,
        };
        let _ = InitCommonControlsEx(&icc);
        let instance = GetModuleHandleW(None).unwrap();
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class_name(),
            hIcon: crate::appicon::load(),
            // Same reason as the settings dialog: a missing background brush
            // leaves the client area unpainted and the static label draws on
            // a mismatched COLOR_BTNFACE band.
            hbrBackground: windows::Win32::Graphics::Gdi::GetSysColorBrush(
                windows::Win32::Graphics::Gdi::COLOR_BTNFACE,
            ),
            ..Default::default()
        };
        RegisterClassW(&wc);
    });
}
