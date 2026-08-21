use crate::config::{Config, WallpaperStyle};
use std::cell::RefCell;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    BST_CHECKED, BST_UNCHECKED, PBM_GETPOS, PBM_SETPOS, PBM_SETRANGE32, PROGRESS_CLASS,
};
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::Shell::ShellExecuteW;
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
const ID_BTN_GITHUB: usize = 2006;
const ID_CHK_ENABLE: usize = 2007;
const ID_CHK_LOCKSCREEN: usize = 2008;
const ID_CHK_AUTOSTART: usize = 2009;

const GITHUB_URL: &str = "https://github.com/abusayed0206/wallboxd.git";

const WM_REFRESH_DONE: u32 = WM_APP + 2;
/// Posted (possibly many times, from any of the poster-fetch worker threads —
/// see `wallgen::fetch_all_posters`) as poster tiles finish downloading.
/// wParam = tiles done so far, lParam = total tiles.
const WM_PROGRESS_UPDATE: u32 = WM_APP + 4;
const TIMER_ANIMATE: usize = 1;
const PROGRESS_STEP: i32 = 5;

thread_local! {
    static RESULT: RefCell<Option<Config>> = const { RefCell::new(None) };
    #[allow(clippy::type_complexity)]
    static CTRLS: RefCell<Option<(HWND, HWND, HWND, HWND, HWND, HWND, HWND, HWND, HWND, HWND, HWND, HWND)>> = const { RefCell::new(None) };
    static REFRESHING: RefCell<bool> = const { RefCell::new(false) };
    // The config the dialog was opened with, so a "Refresh Now" preview
    // (which applies the *unsaved, edited* config to the real desktop/lock
    // screen/registry immediately - see PREVIEW_BACKUP below) can be undone
    // by re-applying this if the user cancels instead of saving.
    static OPENING_CFG: RefCell<Option<Config>> = const { RefCell::new(None) };
    // Set the first time "Refresh Now" runs in this dialog session, from
    // whatever wallpaper.jpg/lockscreen.jpg existed *before* that preview
    // touched them - so Cancel can restore the pre-preview desktop instead
    // of leaving previewed-but-never-saved settings applied for real. `None`
    // until the first preview; deliberately not re-captured on subsequent
    // previews within the same dialog session, so it always reflects the
    // state from before *any* preview, not the most recent one.
    static PREVIEW_BACKUP: RefCell<Option<PreviewBackup>> = const { RefCell::new(None) };
    // Set once the first real WM_PROGRESS_UPDATE arrives, so the indeterminate
    // animation timer stops fighting over the bar's position with real data.
    static REAL_PROGRESS: RefCell<bool> = const { RefCell::new(false) };
    // Highest (done, total) seen so far — guards against the bar visibly
    // jumping backward if updates from different worker threads arrive out
    // of order.
    static LAST_PROGRESS: RefCell<(u32, u32)> = const { RefCell::new((0, 0)) };
}

/// Whether `wallpaper.jpg`/`lockscreen.jpg` existed before the first
/// "Refresh Now" preview in this dialog session touched them - lets restore
/// tell "put the backup back" apart from "there was nothing here, delete
/// what the preview created".
struct PreviewBackup {
    main_existed: bool,
    lock_existed: bool,
}

fn preview_backup_path(p: &std::path::Path) -> std::path::PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(".previewbak");
    std::path::PathBuf::from(s)
}

/// Snapshots the pre-preview `wallpaper.jpg`/`lockscreen.jpg` so a later
/// Cancel can restore them. Only called once per dialog session (see
/// `PREVIEW_BACKUP`'s doc comment) - callers must check it's still `None`
/// first.
fn backup_before_preview() {
    let main_path = crate::wallpaper::cache_path();
    let lock_path = crate::wallpaper::lockscreen_cache_path();
    let main_existed = main_path.exists();
    let lock_existed = lock_path.exists();
    if main_existed {
        let _ = std::fs::copy(&main_path, preview_backup_path(&main_path));
    }
    if lock_existed {
        let _ = std::fs::copy(&lock_path, preview_backup_path(&lock_path));
    }
    PREVIEW_BACKUP.with(|b| {
        *b.borrow_mut() = Some(PreviewBackup {
            main_existed,
            lock_existed,
        })
    });
}

/// Undoes a "Refresh Now" preview: restores the pre-preview cache files (or
/// removes them if there was nothing before) and re-applies the dialog's
/// *opening* config so the actual desktop wallpaper/lock screen/registry
/// style match what's being restored, not whatever the preview last set. A
/// no-op if no preview ran this session.
fn restore_preview_if_needed() {
    let Some(backup) = PREVIEW_BACKUP.with(|b| b.borrow_mut().take()) else {
        return;
    };
    let main_path = crate::wallpaper::cache_path();
    let lock_path = crate::wallpaper::lockscreen_cache_path();
    let main_bak = preview_backup_path(&main_path);
    let lock_bak = preview_backup_path(&lock_path);

    if backup.main_existed {
        let _ = std::fs::copy(&main_bak, &main_path);
        let _ = std::fs::remove_file(&main_bak);
    } else {
        let _ = std::fs::remove_file(&main_path);
    }
    if backup.lock_existed {
        let _ = std::fs::copy(&lock_bak, &lock_path);
        let _ = std::fs::remove_file(&lock_bak);
    } else {
        let _ = std::fs::remove_file(&lock_path);
    }

    if let Some(opening) = OPENING_CFG.with(|c| c.borrow().clone()) {
        let _ = crate::wallpaper::apply_cached(&opening);
    }
}

/// Discards a preview backup without restoring anything - called on OK,
/// where the just-previewed state is exactly what's being kept (saved as
/// the new config), so there's nothing to undo, just backup files to clean
/// up.
fn discard_preview_backup() {
    let Some(_backup) = PREVIEW_BACKUP.with(|b| b.borrow_mut().take()) else {
        return;
    };
    let _ = std::fs::remove_file(preview_backup_path(&crate::wallpaper::cache_path()));
    let _ = std::fs::remove_file(preview_backup_path(
        &crate::wallpaper::lockscreen_cache_path(),
    ));
}

fn get_font() -> WPARAM {
    use std::sync::Once;
    use windows::Win32::Graphics::Gdi::CreateFontIndirectW;
    use windows::Win32::UI::WindowsAndMessaging::{
        NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
        SystemParametersInfoW,
    };
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
        let (enable_check, u, style, i, count_hwnd, a, l_check, l_user, _, _, _, _) =
            c.borrow().unwrap();
        let enabled = SendMessageW(enable_check, BM_GETCHECK, None, None).0 as u32 == BST_CHECKED.0;
        let username = get_text(u);
        let sel_interval = SendMessageW(i, CB_GETCURSEL, None, None).0 as i32;
        let interval = INTERVALS
            .get(sel_interval as usize)
            .map(|(m, _)| *m)
            .unwrap_or(60);
        let autostart = SendMessageW(a, BM_GETCHECK, None, None).0 as u32 == BST_CHECKED.0;
        let set_lockscreen =
            SendMessageW(l_check, BM_GETCHECK, None, None).0 as u32 == BST_CHECKED.0;
        let lockscreen_username = get_text(l_user).trim().to_string();
        let sel = SendMessageW(style, CB_GETCURSEL, None, None).0 as i32;
        let style = usize::try_from(sel)
            .ok()
            .and_then(|idx| WallpaperStyle::ALL.get(idx).copied())
            .unwrap_or_default();
        let count_str = get_text(count_hwnd).trim().to_string();
        // Clamped to [1, 60]: 0 would make `poster_layout::rows_for_count`
        // return `None` (no valid layout fits zero posters), which used to
        // surface as an opaque "invalid wallpaper size" error with no
        // mention of the poster count field being the actual cause.
        let poster_count = if count_str.is_empty() {
            None
        } else {
            count_str.parse::<u32>().ok().map(|v| v.clamp(1, 60))
        };
        Config {
            enabled,
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
    // See main.rs's wndproc for why this needs catch_unwind: unwinding across
    // an extern "system" boundary aborts the whole process regardless of
    // panic=unwind, taking the tray icon and this dialog down with it for
    // what would otherwise be a single bad message.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        wndproc_inner(hwnd, msg, wparam, lparam)
    }))
    .unwrap_or_else(|_| {
        crate::applog::error("panic in settings window procedure - message was not handled");
        LRESULT(0)
    })
}

unsafe fn wndproc_inner(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND => {
                let id = wparam.0 & 0xFFFF;
                if id == ID_BTN_OK {
                    // Belt-and-suspenders: OK/Cancel are disabled (EnableWindow)
                    // for the duration of a Refresh Now, same as WM_CLOSE below,
                    // so this shouldn't normally fire — but don't rely solely on
                    // a disabled control's click being unreachable. Closing here
                    // would orphan the still-running refresh thread and let
                    // main.rs immediately spawn another one on top of it.
                    if REFRESHING.with(|r| *r.borrow()) {
                        return LRESULT(0);
                    }
                    let cfg = get_current_config();
                    // Whatever "Refresh Now" previewed is exactly what's
                    // being kept now - nothing to revert, just clean up the
                    // backup files.
                    discard_preview_backup();
                    RESULT.with(|r| *r.borrow_mut() = Some(cfg));
                    let _ = DestroyWindow(hwnd);
                    LRESULT(0)
                } else if id == ID_BTN_CANCEL {
                    if REFRESHING.with(|r| *r.borrow()) {
                        return LRESULT(0);
                    }
                    // Undo any "Refresh Now" preview so the real desktop/lock
                    // screen isn't left showing settings the user just
                    // discarded (see `restore_preview_if_needed`'s doc
                    // comment - a no-op if no preview ran).
                    restore_preview_if_needed();
                    let _ = DestroyWindow(hwnd);
                    LRESULT(0)
                } else if id == ID_BTN_REFRESH {
                    let cfg = get_current_config();
                    if cfg.username.is_empty() || crate::wallpaper::is_in_progress() {
                        return LRESULT(0);
                    }
                    if PREVIEW_BACKUP.with(|b| b.borrow().is_none()) {
                        backup_before_preview();
                    }
                    REFRESHING.with(|r| *r.borrow_mut() = true);

                    CTRLS.with(|c| {
                        let (
                            enable_check,
                            u,
                            style,
                            i,
                            count,
                            a,
                            l_check,
                            l_user,
                            ref_btn,
                            ok_btn,
                            can_btn,
                            prog,
                        ) = c.borrow().unwrap();
                        let _ = EnableWindow(enable_check, false);
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

                    REAL_PROGRESS.with(|r| *r.borrow_mut() = false);
                    LAST_PROGRESS.with(|p| *p.borrow_mut() = (0, 0));
                    let _ = SetTimer(Some(hwnd), TIMER_ANIMATE, 100, None);

                    let hwnd_raw = hwnd.0 as isize;
                    std::thread::spawn(move || {
                        let hwnd_thread = HWND(hwnd_raw as *mut _);
                        let on_progress = move |done: u32, total: u32| {
                            let dlg = HWND(hwnd_raw as *mut _);
                            let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                                Some(dlg),
                                WM_PROGRESS_UPDATE,
                                WPARAM(done as usize),
                                LPARAM(total as isize),
                            );
                        };
                        let result = crate::wallpaper::refresh(&cfg, &on_progress);
                        let (w, l) = match result {
                            Ok(()) => (WPARAM(0), LPARAM(0)),
                            Err(e) => (WPARAM(1), LPARAM(Box::into_raw(Box::new(e)) as isize)),
                        };
                        let posted = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                            Some(hwnd_thread),
                            WM_REFRESH_DONE,
                            w,
                            l,
                        );
                        if posted.is_err() && w.0 != 0 {
                            // Dialog window gone before delivery - nobody
                            // will call Box::from_raw on this pointer to
                            // reclaim it, so reclaim it here instead of
                            // leaking.
                            let _ = Box::from_raw(l.0 as *mut String);
                        }
                    });

                    LRESULT(0)
                } else if id == ID_BTN_GITHUB {
                    let url: Vec<u16> = GITHUB_URL
                        .encode_utf16()
                        .chain(std::iter::once(0))
                        .collect();
                    // "open" a URL string just hands it to the OS's default
                    // browser, same as double-clicking a link, no shell
                    // execute flags needed. Fire-and-forget: nothing useful
                    // to do with the HINSTANCE-shaped result code here.
                    let _ = ShellExecuteW(
                        Some(hwnd),
                        w!("open"),
                        PCWSTR::from_raw(url.as_ptr()),
                        PCWSTR::null(),
                        PCWSTR::null(),
                        windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
                    );
                    LRESULT(0)
                } else if id == ID_CHK_ENABLE {
                    // BS_AUTOCHECKBOX already flipped its own check state
                    // before sending this notification, so reading it back
                    // here reflects the click that just happened.
                    CTRLS.with(|c| {
                        if let Some((enable_check, _, _, _, _, autostart, lock_check, ..)) =
                            c.borrow().as_ref()
                        {
                            let checked = SendMessageW(*enable_check, BM_GETCHECK, None, None).0
                                as u32
                                == BST_CHECKED.0;
                            let _ = EnableWindow(*lock_check, checked);
                            let _ = EnableWindow(*autostart, checked);
                        }
                    });
                    LRESULT(0)
                } else {
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }
            }
            WM_TIMER => {
                if wparam.0 == TIMER_ANIMATE
                    && REFRESHING.with(|r| *r.borrow())
                    && !REAL_PROGRESS.with(|r| *r.borrow())
                {
                    CTRLS.with(|c| {
                        if let Some((_, _, _, _, _, _, _, _, _, _, _, bar)) = c.borrow().as_ref() {
                            let pos = SendMessageW(*bar, PBM_GETPOS, None, None).0 as i32;
                            let next = if pos + PROGRESS_STEP > 100 {
                                0
                            } else {
                                pos + PROGRESS_STEP
                            };
                            let _ =
                                SendMessageW(*bar, PBM_SETPOS, Some(WPARAM(next as usize)), None);
                        }
                    });
                }
                LRESULT(0)
            }
            WM_PROGRESS_UPDATE => {
                let done = wparam.0 as u32;
                let total = (lparam.0 as u32).max(1);
                let is_newer = LAST_PROGRESS.with(|p| {
                    let (last_done, last_total) = *p.borrow();
                    if total != last_total || done > last_done {
                        *p.borrow_mut() = (done, total);
                        true
                    } else {
                        false
                    }
                });
                if is_newer {
                    REAL_PROGRESS.with(|r| *r.borrow_mut() = true);
                    CTRLS.with(|c| {
                        if let Some((_, _, _, _, _, _, _, _, _, _, _, bar)) = c.borrow().as_ref() {
                            let _ = SendMessageW(
                                *bar,
                                PBM_SETRANGE32,
                                Some(WPARAM(0)),
                                Some(LPARAM(total as isize)),
                            );
                            let _ =
                                SendMessageW(*bar, PBM_SETPOS, Some(WPARAM(done as usize)), None);
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
                    let msg = format!("Refresh failed:\n\n{err}")
                        .encode_utf16()
                        .chain(std::iter::once(0))
                        .collect::<Vec<_>>();
                    let _ = windows::Win32::UI::WindowsAndMessaging::MessageBoxW(
                        Some(hwnd),
                        windows::core::PCWSTR(msg.as_ptr()),
                        w!("Error"),
                        windows::Win32::UI::WindowsAndMessaging::MB_OK
                            | windows::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
                    );
                } else {
                    crate::applog::info("wallpaper refreshed");
                }

                REFRESHING.with(|r| *r.borrow_mut() = false);

                CTRLS.with(|c| {
                    let (
                        enable_check,
                        u,
                        style,
                        i,
                        count,
                        a,
                        l_check,
                        l_user,
                        ref_btn,
                        ok_btn,
                        can_btn,
                        prog,
                    ) = c.borrow().unwrap();
                    let _ = EnableWindow(enable_check, true);
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
                    // Closing via the title bar's X is Cancel in every other
                    // respect (RESULT stays None, nothing gets saved), so it
                    // needs the same preview undo Cancel does.
                    restore_preview_if_needed();
                    let _ = DestroyWindow(hwnd);
                }
                LRESULT(0)
            }
            WM_DESTROY => LRESULT(0),
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
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
    w!("WallboxdSettingsWindow")
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
            hIcon: crate::appicon::load(),
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

/// Same as `label()`, but centered text from an owned `&str` instead of a
/// compile-time `PCWSTR` literal, for the version/build label whose text is
/// only known at runtime.
fn centered_label(
    parent: HWND,
    instance: windows::Win32::Foundation::HMODULE,
    text: &str,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) {
    // SS_CENTER's value (1) is stable ABI, defined in
    // `Win32_System_SystemServices` — pulling in that whole feature for one
    // constant isn't worth it, so it's inlined here instead.
    const SS_CENTER: u32 = 1;
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let hwnd = CreateWindowExW(
            Default::default(),
            w!("STATIC"),
            PCWSTR::from_raw(wide.as_ptr()),
            windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(SS_CENTER)
                | WS_CHILD
                | WS_VISIBLE,
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
    text: &str,
    style: u32,
    id: usize,
    rect: (i32, i32, i32, i32),
) -> HWND {
    let (x, y, w, h) = rect;
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let hwnd = CreateWindowExW(
            Default::default(),
            w!("BUTTON"),
            PCWSTR::from_raw(wide.as_ptr()),
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

/// Visually groups related controls behind a labeled frame — plain "BUTTON"
/// class with `BS_GROUPBOX`, same as `button()` but non-interactive (no
/// `WS_TABSTOP`, no command ID) so it's just a frame, never focusable.
fn group_box(
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
            w!("BUTTON"),
            text,
            windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(
                windows::Win32::UI::WindowsAndMessaging::BS_GROUPBOX as u32,
            ) | WS_CHILD
                | WS_VISIBLE,
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
    OPENING_CFG.with(|c| *c.borrow_mut() = Some(current.clone()));
    // Defensive: should already be None (both Cancel/Close and OK clear it),
    // but a fresh dialog session must never inherit a stale backup pointer
    // from a previous one.
    PREVIEW_BACKUP.with(|b| *b.borrow_mut() = None);

    unsafe {
        let instance = GetModuleHandleW(None).unwrap();

        // Two logical sections (Letterboxd account/wallpaper settings, lock
        // screen settings) behind labeled group-box frames, rather than one
        // flat stack of controls — plus the manifest-enabled themed common
        // controls (see assets/app.manifest), this is most of what makes the
        // dialog look like a modern Windows app instead of a Windows 2000
        // property sheet.
        const DLG_W: i32 = 380;
        const DLG_H: i32 = 634;
        const MARGIN: i32 = 12;
        const INDENT: i32 = 24;

        let dlg = CreateWindowExW(
            Default::default(),
            class_name(),
            w!("Wallboxd Settings"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            DLG_W,
            DLG_H,
            Some(owner),
            None,
            Some(instance.into()),
            None,
        )
        .unwrap();

        // Derived from the *actual* client rect rather than DLG_W: the
        // window's non-client frame (title bar + border) eats a few pixels
        // that CreateWindowExW's width includes but child controls don't get
        // to use, so a content width computed straight from DLG_W silently
        // overflows the real client area by that amount and throws off
        // right-edge alignment of anything spanning the full width.
        let mut client_rect = windows::Win32::Foundation::RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(dlg, &mut client_rect);
        let client_w = client_rect.right;
        let content_w = client_w - 2 * MARGIN;
        let field_w = client_w - 2 * INDENT; // indented inside a group box

        let scr_w = GetSystemMetrics(SM_CXSCREEN);
        let scr_h = GetSystemMetrics(SM_CYSCREEN);
        let _ = SetWindowPos(
            dlg,
            Some(HWND::default()),
            (scr_w - DLG_W) / 2,
            (scr_h - DLG_H) / 2,
            0,
            0,
            SWP_NOZORDER | SWP_NOSIZE,
        );

        // --- Master switch: nothing runs automatically until this is on ---
        let hwnd_enable = button(
            dlg,
            instance,
            "Enable Wallboxd",
            windows::Win32::UI::WindowsAndMessaging::BS_AUTOCHECKBOX as u32,
            ID_CHK_ENABLE,
            (MARGIN, 10, content_w, 22),
        );
        let enable_state = if current.enabled {
            BST_CHECKED.0
        } else {
            BST_UNCHECKED.0
        };
        let _ = SendMessageW(
            hwnd_enable,
            BM_SETCHECK,
            Some(WPARAM(enable_state as usize)),
            None,
        );

        // --- "Letterboxd" group: account + wallpaper generation settings ---
        group_box(dlg, instance, w!("Letterboxd"), MARGIN, 44, content_w, 232);

        label(
            dlg,
            instance,
            w!("Letterboxd username:"),
            INDENT,
            66,
            field_w,
            16,
        );
        let hwnd_user = edit(dlg, instance, &current.username, INDENT, 84, field_w, 24);

        label(
            dlg,
            instance,
            w!("Wallpaper style:"),
            INDENT,
            118,
            field_w,
            16,
        );
        let hwnd_style = style_combo(dlg, instance, current.style, INDENT, 136, field_w, 120);

        label(
            dlg,
            instance,
            w!("Refresh interval:"),
            INDENT,
            170,
            field_w,
            16,
        );
        let hwnd_interval = interval_combo(
            dlg,
            instance,
            current.interval_minutes,
            INDENT,
            188,
            field_w,
            120,
        );

        label(
            dlg,
            instance,
            w!("Poster count (empty = auto, max 60):"),
            INDENT,
            222,
            field_w,
            16,
        );
        let count_str = current
            .poster_count
            .map(|c| c.to_string())
            .unwrap_or_default();
        let hwnd_count = edit(dlg, instance, &count_str, INDENT, 240, field_w, 24);

        // --- "Lock Screen" group ---
        group_box(
            dlg,
            instance,
            w!("Lock Screen"),
            MARGIN,
            288,
            content_w,
            104,
        );

        let hwnd_lock_check = button(
            dlg,
            instance,
            "Set lock screen wallpaper",
            windows::Win32::UI::WindowsAndMessaging::BS_AUTOCHECKBOX as u32,
            ID_CHK_LOCKSCREEN,
            (INDENT, 310, field_w, 22),
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
            INDENT,
            338,
            field_w,
            16,
        );
        let hwnd_lock_user = edit(
            dlg,
            instance,
            &current.lockscreen_username,
            INDENT,
            356,
            field_w,
            24,
        );

        // --- Outside any group: startup behavior, progress, actions ---
        let hwnd_autostart = button(
            dlg,
            instance,
            "Start with Windows",
            windows::Win32::UI::WindowsAndMessaging::BS_AUTOCHECKBOX as u32,
            ID_CHK_AUTOSTART,
            (MARGIN, 404, content_w, 22),
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

        // Both depend on Wallboxd actually running to mean anything, so they
        // can't be turned on while the master switch is off. Reflects the
        // dialog's *opening* state here; the WM_COMMAND handler for
        // ID_CHK_ENABLE keeps this in sync as the user toggles it live.
        let _ = EnableWindow(hwnd_lock_check, current.enabled);
        let _ = EnableWindow(hwnd_autostart, current.enabled);

        let hwnd_progress = CreateWindowExW(
            Default::default(),
            PROGRESS_CLASS,
            PCWSTR::null(),
            WS_CHILD, // Not visible initially
            MARGIN,
            438,
            content_w,
            18,
            Some(dlg),
            None,
            Some(instance.into()),
            None,
        )
        .unwrap();

        let btn_w = (content_w - 2 * 13) / 3; // 3 equal buttons, 13px gaps between
        let hwnd_refresh = button(
            dlg,
            instance,
            "Refresh Now",
            0,
            ID_BTN_REFRESH,
            (MARGIN, 468, btn_w, 30),
        );
        let hwnd_ok = button(
            dlg,
            instance,
            "OK",
            0,
            ID_BTN_OK,
            (MARGIN + btn_w + 13, 468, btn_w, 30),
        );
        let hwnd_cancel = button(
            dlg,
            instance,
            "Cancel",
            0,
            ID_BTN_CANCEL,
            (MARGIN + 2 * (btn_w + 13), 468, btn_w, 30),
        );

        // Narrow and centered, rather than spanning the full width like the
        // row above: it's a secondary link, not an equal-weight action, so
        // matching the primary row's width would overstate its importance.
        const GITHUB_BTN_W: i32 = 100;
        let _hwnd_github = button(
            dlg,
            instance,
            "GitHub",
            0,
            ID_BTN_GITHUB,
            (
                MARGIN + (content_w - GITHUB_BTN_W) / 2,
                512,
                GITHUB_BTN_W,
                26,
            ),
        );

        let version_label = format!(
            "v{} ({})",
            env!("CARGO_PKG_VERSION"),
            env!("WALLBOXD_GIT_HASH")
        );
        centered_label(dlg, instance, &version_label, MARGIN, 544, content_w, 16);

        CTRLS.with(|c| {
            *c.borrow_mut() = Some((
                hwnd_enable,
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
