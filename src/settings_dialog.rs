use crate::config::{Config, WallpaperStyle};
use std::cell::RefCell;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{BST_CHECKED, BST_UNCHECKED};
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, CBS_DROPDOWNLIST,
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowTextLengthW, GetWindowTextW, IsWindow, MSG, RegisterClassW, SW_SHOW, SendMessageW,
    ShowWindow, TranslateMessage, WM_CLOSE, WM_COMMAND, WM_DESTROY, WNDCLASSW, WS_CAPTION,
    WS_CHILD, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

const ID_BTN_OK: usize = 2004;
const ID_BTN_CANCEL: usize = 2005;

thread_local! {
    // A settings dialog is always modal (the caller blocks until it closes),
    // so one slot of state shared via thread-locals is simpler than threading
    // a raw pointer through GWLP_USERDATA for something that only ever has
    // one live instance.
    static RESULT: RefCell<Option<Config>> = const { RefCell::new(None) };
    // username, style combo, interval, autostart
    static CTRLS: RefCell<Option<(HWND, HWND, HWND, HWND)>> = const { RefCell::new(None) };
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = wparam.0 & 0xFFFF;
            if id == ID_BTN_OK {
                let cfg = CTRLS.with(|c| unsafe {
                    let (u, style, i, a) = c.borrow().unwrap();
                    let username = get_text(u);
                    let interval: u32 = get_text(i).trim().parse().unwrap_or(60).max(5);
                    let autostart =
                        SendMessageW(a, BM_GETCHECK, None, None).0 as u32 == BST_CHECKED.0;
                    let sel = SendMessageW(style, CB_GETCURSEL, None, None).0 as i32;
                    let style = usize::try_from(sel)
                        .ok()
                        .and_then(|i| WallpaperStyle::ALL.get(i).copied())
                        .unwrap_or_default();
                    Config {
                        username,
                        interval_minutes: interval,
                        autostart,
                        poster_count: None,
                        style,
                    }
                });
                RESULT.with(|r| *r.borrow_mut() = Some(cfg));
                let _ = unsafe { DestroyWindow(hwnd) };
                LRESULT(0)
            } else if id == ID_BTN_CANCEL {
                let _ = unsafe { DestroyWindow(hwnd) };
                LRESULT(0)
            } else {
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
        }
        WM_CLOSE => {
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
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
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let instance = GetModuleHandleW(None).unwrap();
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class_name(),
            ..Default::default()
        };
        RegisterClassW(&wc);
    });
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
        let _ = CreateWindowExW(
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
        );
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
        CreateWindowExW(
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
        .unwrap()
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
        CreateWindowExW(
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
        .unwrap()
    }
}

fn combo(
    parent: HWND,
    instance: windows::Win32::Foundation::HMODULE,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> HWND {
    unsafe {
        CreateWindowExW(
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
        .unwrap()
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
    let hwnd = combo(parent, instance, x, y, w, h);
    unsafe {
        for style in WallpaperStyle::ALL {
            let wide: Vec<u16> = style
                .label()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            SendMessageW(
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
        SendMessageW(hwnd, CB_SETCURSEL, Some(WPARAM(idx)), None);
    }
    hwnd
}

/// Blocks the calling thread until the user closes the dialog. Returns
/// `Some(config)` on OK, `None` on Cancel/close.
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
            340,
            320,
            Some(owner),
            None,
            Some(instance.into()),
            None,
        )
        .unwrap();

        label(dlg, instance, w!("Letterboxd username:"), 12, 12, 300, 18);
        let hwnd_user = edit(dlg, instance, &current.username, 12, 32, 300, 24);

        label(dlg, instance, w!("Wallpaper style:"), 12, 66, 300, 18);
        let hwnd_style = style_combo(dlg, instance, current.style, 12, 86, 300, 24);

        label(
            dlg,
            instance,
            w!("Refresh interval (minutes, min 5):"),
            12,
            120,
            300,
            18,
        );
        let hwnd_interval = edit(
            dlg,
            instance,
            &current.interval_minutes.to_string(),
            12,
            140,
            300,
            24,
        );

        let hwnd_autostart = button(
            dlg,
            instance,
            w!("Start with Windows"),
            windows::Win32::UI::WindowsAndMessaging::BS_AUTOCHECKBOX as u32,
            0,
            (12, 174, 300, 22),
        );
        if current.autostart {
            let _ = SendMessageW(
                hwnd_autostart,
                BM_SETCHECK,
                Some(WPARAM(BST_CHECKED.0 as usize)),
                None,
            );
        } else {
            let _ = SendMessageW(
                hwnd_autostart,
                BM_SETCHECK,
                Some(WPARAM(BST_UNCHECKED.0 as usize)),
                None,
            );
        }

        button(dlg, instance, w!("OK"), 0, ID_BTN_OK, (90, 210, 80, 28));
        button(
            dlg,
            instance,
            w!("Cancel"),
            0,
            ID_BTN_CANCEL,
            (180, 210, 80, 28),
        );

        CTRLS.with(|c| {
            *c.borrow_mut() = Some((hwnd_user, hwnd_style, hwnd_interval, hwnd_autostart))
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
