use crate::config::{Config, WallpaperStyle};
use std::cell::RefCell;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    BST_CHECKED, BST_UNCHECKED, PBM_GETPOS, PBM_SETPOS, PROGRESS_CLASS,
};
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, CBS_DROPDOWNLIST,
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetSystemMetrics, GetWindowTextLengthW, GetWindowTextW, IsWindow, KillTimer, MSG,
    RegisterClassW, SM_CXSCREEN, SM_CYSCREEN, SW_HIDE, SW_SHOW, SWP_NOSIZE, SWP_NOZORDER,
    SendMessageW, SetTimer, SetWindowPos, ShowWindow, TranslateMessage, WM_APP, WM_CLOSE,
    WM_COMMAND, WM_DESTROY, WM_SETFONT, WM_TIMER, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_OVERLAPPED,
    WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

const ID_BTN_REFRESH: usize = 2003;
const ID_BTN_OK: usize = 2004;
const ID_BTN_CANCEL: usize = 2005;

const WM_REFRESH_DONE: u32 = WM_APP + 2;
const TIMER_ANIMATE: usize = 1;
const PROGRESS_STEP: i32 = 5;

thread_local! {
    static RESULT: RefCell<Option<Config>> = const { RefCell::new(None) };
    #[allow(clippy::type_complexity)]
    static CTRLS: RefCell<Option<(HWND, HWND, HWND, HWND, HWND, HWND, HWND, HWND, HWND, HWND, HWND)>> = const { RefCell::new(None) };
    static REFRESHING: RefCell<bool> = const { RefCell::new(false) };
}

fn get_font() -> WPARAM {
    use std::sync::Once;
    use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETNONCLIENTMETRICS, NONCLIENTMETRICSW, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};
    use windows::Win32::Graphics::Gdi::CreateFontIndirectW;
    static mut FONT_WPARAM: usize = 0;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let mut ncm = NONCLIENTMETRICSW {
            cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
            ..Default::default()
        };
        let _ = SystemParametersInfoW(
            SPI_GETNONCLIENTMETRICS,
            std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
            Some(&mut ncm as *mut _ as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let f = CreateFontIndirectW(&ncm.lfMessageFont);
        FONT_WPARAM = std::mem::transmute::<windows::Win32::Graphics::Gdi::HFONT, usize>(f);
    });
    WPARAM(unsafe { FONT_WPARAM })
}

fn apply_font(hwnd: HWND) {
    unsafe {
        let _ = SendMessageW(hwnd, WM_SETFONT, Some(get_font()), Some(LPARAM(0)));
    }
}

fn get_current_config() -> Config {
    CTRLS.with(|c| unsafe {
        let (u, style, i, count_hwnd, a, l_check, l_user, _, _, _, _) = c.borrow().unwrap();
        let username = get_text(u);
        let sel_interval = SendMessageW(i, CB_GETCURSEL, None, None).0 as i32;
        let interval = INTERVALS.get(sel_interval as usize).map(|(m, _)| *m).unwrap_or(60);
        let autostart = SendMessageW(a, BM_GETCHECK, None, None).0 as u32 == BST_CHECKED.0;
        let set_lockscreen = SendMessageW(l_check, BM_GETCHECK, None, None).0 as u32 == BST_CHECKED.0;
        let lockscreen_username = get_text(l_user).trim().to_string();
        let sel = SendMessageW(style, CB_GETCURSEL, None, None).0 as i32;
        let style = usize::try_from(sel)
            .ok()
            .and_then(|idx| WallpaperStyle::ALL.get(idx).copied())
            .unwrap_or_default();
        let count_str = get_text(count_hwnd).trim().to_string();
        let poster_count = if count_str.is_empty() {
            None
        } else {
            count_str.parse::<u32>().ok().map(|v| v.min(60))
        };
        Config {
            username,
            interval_minutes: interval,
            autostart,
            poster_count,
            style,
            set_lockscreen,
            lockscreen_username,
        }
    })
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { match msg {
        WM_COMMAND => {
            let id = wparam.0 & 0xFFFF;
            if id == ID_BTN_OK {
                let cfg = get_current_config();
                RESULT.with(|r| *r.borrow_mut() = Some(cfg));
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            } else if id == ID_BTN_CANCEL {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            } else if id == ID_BTN_REFRESH {
                let cfg = get_current_config();
                if cfg.username.is_empty() {
                    return LRESULT(0);
                }
                REFRESHING.with(|r| *r.borrow_mut() = true);

                CTRLS.with(|c| {
                    let (u, style, i, count, a, l_check, l_user, ref_btn, ok_btn, can_btn, prog) =
                        c.borrow().unwrap();
                    let _ = EnableWindow(u, false);
                    let _ = EnableWindow(style, false);
                    let _ = EnableWindow(i, false);
                    let _ = EnableWindow(count, false);
                    let _ = EnableWindow(a, false);
                    let _ = EnableWindow(l_check, false);
                    let _ = EnableWindow(l_user, false);
                    let _ = EnableWindow(ref_btn, false);
                    let _ = EnableWindow(ok_btn, false);
                    let _ = EnableWindow(can_btn, false);
                    let _ = ShowWindow(prog, SW_SHOW);
                });

                let _ = SetTimer(Some(hwnd), TIMER_ANIMATE, 100, None);

                let hwnd_raw = hwnd.0 as isize;
                std::thread::spawn(move || {
                    let hwnd_thread = HWND(hwnd_raw as *mut _);
                    let result = crate::wallpaper::refresh(&cfg);
                    let (w, l) = match result {
                        Ok(()) => (WPARAM(0), LPARAM(0)),
                        Err(e) => (WPARAM(1), LPARAM(Box::into_raw(Box::new(e)) as isize)),
                    };
                    let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                        Some(hwnd_thread),
                        WM_REFRESH_DONE,
                        w,
                        l,
                    );
                });

                LRESULT(0)
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        WM_TIMER => {
            if wparam.0 == TIMER_ANIMATE && REFRESHING.with(|r| *r.borrow()) {
                CTRLS.with(|c| {
                    if let Some((_, _, _, _, _, _, _, _, _, _, bar)) = c.borrow().as_ref() {
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
        WM_REFRESH_DONE => {
            let _ = KillTimer(Some(hwnd), TIMER_ANIMATE);
            if wparam.0 != 0 {
                let err = Box::from_raw(lparam.0 as *mut String);
                crate::applog::error(&format!("refresh failed: {err}"));
                let mut msg = format!("Refresh failed:\n\n{err}").encode_utf16().chain(std::iter::once(0)).collect::<Vec<_>>();
                let _ = windows::Win32::UI::WindowsAndMessaging::MessageBoxW(
                    Some(hwnd),
                    windows::core::PCWSTR(msg.as_ptr()),
                    w!("Error"),
                    windows::Win32::UI::WindowsAndMessaging::MB_OK | windows::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
                );
            } else {
                crate::applog::info("wallpaper refreshed");
            }

            REFRESHING.with(|r| *r.borrow_mut() = false);

            CTRLS.with(|c| {
                let (u, style, i, count, a, l_check, l_user, ref_btn, ok_btn, can_btn, prog) = c.borrow().unwrap();
                let _ = EnableWindow(u, true);
                let _ = EnableWindow(style, true);
                let _ = EnableWindow(i, true);
                let _ = EnableWindow(count, true);
                let _ = EnableWindow(a, true);
                let _ = EnableWindow(l_check, true);
                let _ = EnableWindow(l_user, true);
                let _ = EnableWindow(ref_btn, true);
                let _ = EnableWindow(ok_btn, true);
                let _ = EnableWindow(can_btn, true);
                let _ = SendMessageW(prog, PBM_SETPOS, Some(WPARAM(0)), None);
                let _ = ShowWindow(prog, SW_HIDE);
            });
            let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), None, true);
            LRESULT(0)
        }
        WM_CLOSE => {
            if !REFRESHING.with(|r| *r.borrow()) {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    } }
}

fn get_text(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

fn class_name() -> PCWSTR {
    w!("WallbSettingsWindow")
}

fn register_class_once() {
    use std::sync::Once;
    use windows::Win32::UI::Controls::{
        ICC_PROGRESS_CLASS, INITCOMMONCONTROLSEX, InitCommonControlsEx,
    };
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
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);
    });
}

const INTERVALS: [(u32, &str); 5] = [
    (5, "5 min"),
    (10, "10 min"),
    (60, "1 hour"),
    (360, "6 hours"),
    (1440, "24 hours"),
];

fn interval_combo(
    parent: HWND,
    instance: windows::Win32::Foundation::HMODULE,
    current: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> HWND {
    unsafe {
        let hwnd = CreateWindowExW(
            Default::default(),
            w!("COMBOBOX"),
            PCWSTR::null(),
            windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(CBS_DROPDOWNLIST as u32)
                | WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP,
            x,
            y,
            w,
            h,
            Some(parent),
            None,
            Some(instance.into()),
            None,
        )
        .unwrap();
        apply_font(hwnd);
        let mut selected_idx = 0;
        for (i, (val, label)) in INTERVALS.iter().enumerate() {
            let wide: Vec<u16> = label.encode_utf16().chain(std::iter::once(0)).collect();
            let _ = SendMessageW(
                hwnd,
                CB_ADDSTRING,
                None,
                Some(LPARAM(wide.as_ptr() as isize)),
            );
            if *val == current {
                selected_idx = i;
            }
        }
        let _ = SendMessageW(hwnd, CB_SETCURSEL, Some(WPARAM(selected_idx)), None);
        hwnd
    }
}

fn label(
    parent: HWND,
    instance: windows::Win32::Foundation::HMODULE,
    text: PCWSTR,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) {
    unsafe {
        let hwnd = CreateWindowExW(
            Default::default(),
            w!("STATIC"),
            text,
            WS_CHILD | WS_VISIBLE,
            x,
            y,
            w,
            h,
            Some(parent),
            None,
            Some(instance.into()),
            None,
        )
        .unwrap();
        apply_font(hwnd);
    }
}

fn edit(
    parent: HWND,
    instance: windows::Win32::Foundation::HMODULE,
    text: &str,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> HWND {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let hwnd = CreateWindowExW(
            windows::Win32::UI::WindowsAndMessaging::WS_EX_CLIENTEDGE,
            w!("EDIT"),
            PCWSTR::from_raw(wide.as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            x,
            y,
            w,
            h,
            Some(parent),
            None,
            Some(instance.into()),
            None,
        )
        .unwrap();
        apply_font(hwnd);
        hwnd
    }
}

fn button(
    parent: HWND,
    instance: windows::Win32::Foundation::HMODULE,
    text: PCWSTR,
    style: u32,
    id: usize,
    rect: (i32, i32, i32, i32),
) -> HWND {
    let (x, y, w, h) = rect;
    unsafe {
        let hwnd = CreateWindowExW(
            Default::default(),
            w!("BUTTON"),
            text,
            windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(style)
                | WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP,
            x,
            y,
            w,
            h,
            Some(parent),
            Some(windows::Win32::UI::WindowsAndMessaging::HMENU(id as *mut _)),
            Some(instance.into()),
            None,
        )
        .unwrap();
        apply_font(hwnd);
        hwnd
    }
}

fn style_combo(
    parent: HWND,
    instance: windows::Win32::Foundation::HMODULE,
    current: WallpaperStyle,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> HWND {
    unsafe {
        let hwnd = CreateWindowExW(
            Default::default(),
            w!("COMBOBOX"),
            PCWSTR::null(),
            windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(CBS_DROPDOWNLIST as u32)
                | WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP,
            x,
            y,
            w,
            h,
            Some(parent),
            None,
            Some(instance.into()),
            None,
        )
        .unwrap();
        apply_font(hwnd);
        for style in WallpaperStyle::ALL {
            let wide: Vec<u16> = style
                .label()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let _ = SendMessageW(
                hwnd,
                CB_ADDSTRING,
                None,
                Some(LPARAM(wide.as_ptr() as isize)),
            );
        }
        let idx = WallpaperStyle::ALL
            .iter()
            .position(|s| *s == current)
            .unwrap_or(0);
        let _ = SendMessageW(hwnd, CB_SETCURSEL, Some(WPARAM(idx)), None);
        hwnd
    }
}

pub fn show(owner: HWND, current: &Config) -> Option<Config> {
    register_class_once();
    RESULT.with(|r| *r.borrow_mut() = None);

    unsafe {
        let instance = GetModuleHandleW(None).unwrap();

        let dlg = CreateWindowExW(
            Default::default(),
            class_name(),
            w!("wallb settings"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            360,
            480,
            Some(owner),
            None,
            Some(instance.into()),
            None,
        )
        .unwrap();

        let scr_w = GetSystemMetrics(SM_CXSCREEN);
        let scr_h = GetSystemMetrics(SM_CYSCREEN);
        let _ = SetWindowPos(dlg, Some(HWND::default()), (scr_w - 360) / 2, (scr_h - 480) / 2, 0, 0, SWP_NOZORDER | SWP_NOSIZE);

        label(dlg, instance, w!("Letterboxd username:"), 12, 12, 320, 18);
        let hwnd_user = edit(dlg, instance, &current.username, 12, 32, 320, 24);

        label(dlg, instance, w!("Wallpaper style:"), 12, 66, 320, 18);
        let hwnd_style = style_combo(dlg, instance, current.style, 12, 86, 320, 120);

        label(
            dlg,
            instance,
            w!("Refresh interval:"),
            12,
            120,
            320,
            18,
        );
        let hwnd_interval = interval_combo(
            dlg,
            instance,
            current.interval_minutes,
            12,
            140,
            320,
            120,
        );

        label(
            dlg,
            instance,
            w!("Poster count (empty = auto, max 60):"),
            12,
            174,
            320,
            18,
        );
        let count_str = current
            .poster_count
            .map(|c| c.to_string())
            .unwrap_or_default();
        let hwnd_count = edit(dlg, instance, &count_str, 12, 194, 320, 24);

        let hwnd_autostart = button(
            dlg,
            instance,
            w!("Start with Windows"),
            windows::Win32::UI::WindowsAndMessaging::BS_AUTOCHECKBOX as u32,
            0,
            (12, 228, 320, 22),
        );
        let check_state = if current.autostart {
            BST_CHECKED.0
        } else {
            BST_UNCHECKED.0
        };
        let _ = SendMessageW(
            hwnd_autostart,
            BM_SETCHECK,
            Some(WPARAM(check_state as usize)),
            None,
        );

        let hwnd_lock_check = button(
            dlg,
            instance,
            w!("Set lock screen wallpaper"),
            windows::Win32::UI::WindowsAndMessaging::BS_AUTOCHECKBOX as u32,
            0,
            (12, 256, 320, 22),
        );
        let l_check_state = if current.set_lockscreen {
            BST_CHECKED.0
        } else {
            BST_UNCHECKED.0
        };
        let _ = SendMessageW(
            hwnd_lock_check,
            BM_SETCHECK,
            Some(WPARAM(l_check_state as usize)),
            None,
        );

        label(
            dlg,
            instance,
            w!("Lock screen username (leave empty to use same):"),
            12,
            284,
            320,
            18,
        );
        let hwnd_lock_user = edit(dlg, instance, &current.lockscreen_username, 12, 304, 320, 24);

        let hwnd_progress = CreateWindowExW(
            Default::default(),
            PROGRESS_CLASS,
            PCWSTR::null(),
            WS_CHILD, // Not visible initially
            12,
            340,
            320,
            18,
            Some(dlg),
            None,
            Some(instance.into()),
            None,
        )
        .unwrap();

        let hwnd_refresh = button(
            dlg,
            instance,
            w!("Refresh Now"),
            0,
            ID_BTN_REFRESH,
            (12, 368, 100, 28),
        );
        let hwnd_ok = button(dlg, instance, w!("OK"), 0, ID_BTN_OK, (140, 368, 80, 28));
        let hwnd_cancel = button(
            dlg,
            instance,
            w!("Cancel"),
            0,
            ID_BTN_CANCEL,
            (240, 368, 80, 28),
        );

        CTRLS.with(|c| {
            *c.borrow_mut() = Some((
                hwnd_user,
                hwnd_style,
                hwnd_interval,
                hwnd_count,
                hwnd_autostart,
                hwnd_lock_check,
                hwnd_lock_user,
                hwnd_refresh,
                hwnd_ok,
                hwnd_cancel,
                hwnd_progress,
            ))
        });

        let _ = EnableWindow(owner, false);
        let _ = ShowWindow(dlg, SW_SHOW);

        let mut msg = MSG::default();
        while IsWindow(Some(dlg)).as_bool() {
            let r = GetMessageW(&mut msg, None, 0, 0);
            if r.0 <= 0 {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = EnableWindow(owner, true);
    }

    RESULT.with(|r| r.borrow_mut().take())
}
