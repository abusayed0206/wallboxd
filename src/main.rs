#![windows_subsystem = "windows"]

mod applog;
mod autostart;
mod config;
mod letterboxd;
mod poster_layout;
mod progress;
mod settings_dialog;
mod tray;
mod wallerror;
mod wallgen;
mod wallpaper;

use config::Config;
use std::cell::RefCell;
use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, POINT, SetLastError,
    WIN32_ERROR, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos, GetMessageW,
    KillTimer, MSG, PostMessageW, PostQuitMessage, RegisterClassW, SetTimer, TranslateMessage,
    WM_APP, WM_COMMAND, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP, WM_TIMER, WNDCLASSW, WS_OVERLAPPED,
};
use windows::core::w;

const TIMER_REFRESH: usize = 1;
const WM_REVERT_CONFIG: u32 = WM_APP + 3;
const SINGLE_INSTANCE_MUTEX: windows::core::PCWSTR = w!("Local\\WallboxdSingleInstanceMutex");

thread_local! {
    static STATE: RefCell<Option<Config>> = const { RefCell::new(None) };
}

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let Some(_instance_guard) = acquire_single_instance() else {
        return;
    };

    config::migrate_from_legacy();
    let _ = std::fs::create_dir_all(config::data_dir());
    let cfg = Config::load();

    let hwnd = create_main_window();

    if let Err(e) = wallpaper::apply_cached(&cfg) {
        applog::info(&format!("no cached wallpaper on startup: {e}"));
    }

    tray::add(hwnd, &tooltip_for(&cfg));
    start_timer(hwnd, cfg.interval_minutes);

    if !cfg.username.is_empty() {
        spawn_refresh(cfg.clone());
    }

    STATE.with(|s| *s.borrow_mut() = Some(cfg));

    run_message_loop();
}

/// Grabs a named mutex so a second launch (e.g. a stray Run-key start while
/// the tray instance is already up) exits immediately instead of registering
/// a duplicate tray icon.
fn acquire_single_instance() -> Option<HANDLE> {
    unsafe {
        SetLastError(WIN32_ERROR(0));
        let mutex = CreateMutexW(None, true, SINGLE_INSTANCE_MUTEX).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            None
        } else {
            Some(mutex)
        }
    }
}

fn tooltip_for(cfg: &Config) -> String {
    if cfg.username.is_empty() {
        "Wallboxd - not configured".to_string()
    } else {
        format!("Wallboxd - {}", cfg.username)
    }
}

fn spawn_refresh(cfg: Config) {
    std::thread::spawn(move || match wallpaper::refresh(&cfg, &|_, _| {}) {
        Ok(()) => applog::info("wallpaper refreshed"),
        Err(e) => applog::error(&format!("refresh failed: {e}")),
    });
}

/// Like `spawn_refresh`, but reports the outcome to the progress dialog via
/// WM_REFRESH_DONE so the user sees the download/apply happen. If the dialog
/// was closed before the refresh finished, the PostMessage fails and the
/// result is logged directly instead.
fn spawn_refresh_with_progress(
    cfg: Config,
    dialog: HWND,
    hwnd_main: HWND,
    old_cfg: Option<Config>,
) {
    // HWND wraps a raw pointer and isn't Send; pass the pointer value and
    // rebuild the handle inside the thread.
    let dialog_raw = dialog.0 as isize;
    let main_raw = hwnd_main.0 as isize;
    std::thread::spawn(move || {
        let dialog = HWND(dialog_raw as *mut _);
        let main = HWND(main_raw as *mut _);
        let on_progress = move |done: u32, total: u32| {
            let dialog = HWND(dialog_raw as *mut _);
            let _ = unsafe {
                PostMessageW(
                    Some(dialog),
                    progress::WM_PROGRESS_UPDATE,
                    WPARAM(done as usize),
                    LPARAM(total as isize),
                )
            };
        };
        let result = wallpaper::refresh(&cfg, &on_progress);
        let (wparam, lparam) = match result {
            Ok(()) => (WPARAM(0), LPARAM(0)),
            Err(e) => {
                if let Some(old) = old_cfg {
                    let old_ptr = Box::into_raw(Box::new(old)) as isize;
                    let _ = unsafe {
                        PostMessageW(Some(main), WM_REVERT_CONFIG, WPARAM(0), LPARAM(old_ptr))
                    };
                }
                (WPARAM(1), LPARAM(Box::into_raw(Box::new(e)) as isize))
            }
        };
        let posted =
            unsafe { PostMessageW(Some(dialog), progress::WM_REFRESH_DONE, wparam, lparam) };
        if posted.is_err() {
            if wparam.0 != 0 {
                let err = unsafe { Box::from_raw(lparam.0 as *mut String) };
                applog::error(&format!("refresh failed: {err}"));
            } else {
                applog::info("wallpaper refreshed");
            }
        }
    });
}

fn start_timer(hwnd: HWND, interval_minutes: u32) {
    let millis = interval_minutes.max(1) * 60_000;
    unsafe {
        SetTimer(Some(hwnd), TIMER_REFRESH, millis, None);
    }
}

fn class_name() -> windows::core::PCWSTR {
    w!("WallboxdMainWindow")
}

fn create_main_window() -> HWND {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap();
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class_name(),
            ..Default::default()
        };
        RegisterClassW(&wc);

        // Never shown — it only exists to own the tray icon, receive its
        // messages, and act as the settings dialog's owner window.
        CreateWindowExW(
            Default::default(),
            class_name(),
            w!("Wallboxd"),
            WS_OVERLAPPED,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap()
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        tray::WM_TRAYICON => {
            let mouse_msg = lparam.0 as u32;
            if mouse_msg == WM_RBUTTONUP || mouse_msg == WM_LBUTTONUP {
                let mut pt = POINT::default();
                let _ = unsafe { GetCursorPos(&mut pt) };
                tray::show_menu(hwnd, pt.x, pt.y);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = wparam.0 & 0xFFFF;
            match id as u32 {
                tray::ID_REFRESH => {
                    let cfg = STATE.with(|s| s.borrow().clone());
                    if let Some(cfg) = cfg
                        && !cfg.username.is_empty()
                        && !progress::is_active()
                    {
                        let dialog = progress::show(hwnd);
                        spawn_refresh_with_progress(cfg, dialog, hwnd, None);
                    }
                    LRESULT(0)
                }
                tray::ID_SETTINGS => {
                    let current = STATE.with(|s| s.borrow().clone()).unwrap_or_default();
                    if let Some(new_cfg) = settings_dialog::show(hwnd, &current) {
                        if let Err(e) = new_cfg.save() {
                            applog::error(&format!("failed to save config: {e}"));
                        }
                        autostart::set(new_cfg.autostart);

                        let _ = unsafe { KillTimer(Some(hwnd), TIMER_REFRESH) };
                        start_timer(hwnd, new_cfg.interval_minutes);

                        tray::set_tooltip(hwnd, &tooltip_for(&new_cfg));

                        if let Err(e) = wallpaper::apply_cached(&new_cfg) {
                            applog::info(&format!("apply_cached after settings change: {e}"));
                        }
                        if !new_cfg.username.is_empty() && !progress::is_active() {
                            let dialog = progress::show(hwnd);
                            spawn_refresh_with_progress(
                                new_cfg.clone(),
                                dialog,
                                hwnd,
                                Some(current.clone()),
                            );
                        }

                        STATE.with(|s| *s.borrow_mut() = Some(new_cfg));
                    }
                    LRESULT(0)
                }
                tray::ID_EXIT => {
                    tray::remove(hwnd);
                    unsafe { PostQuitMessage(0) };
                    LRESULT(0)
                }
                _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
            }
        }
        WM_TIMER => {
            if wparam.0 == TIMER_REFRESH {
                let cfg = STATE.with(|s| s.borrow().clone());
                if let Some(cfg) = cfg
                    && !cfg.username.is_empty()
                    && !progress::is_active()
                {
                    spawn_refresh(cfg);
                }
            }
            LRESULT(0)
        }
        WM_REVERT_CONFIG => {
            let old_cfg = unsafe { Box::from_raw(lparam.0 as *mut Config) };
            if let Err(e) = old_cfg.save() {
                applog::error(&format!("failed to revert config: {e}"));
            }
            autostart::set(old_cfg.autostart);
            STATE.with(|s| *s.borrow_mut() = Some(*old_cfg.clone()));
            tray::set_tooltip(hwnd, &tooltip_for(&old_cfg));
            start_timer(hwnd, old_cfg.interval_minutes);
            if let Err(e) = wallpaper::apply_cached(&old_cfg) {
                applog::error(&format!("failed to re-apply old wallpaper on revert: {e}"));
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            tray::remove(hwnd);
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn run_message_loop() {
    let mut msg = MSG::default();
    unsafe {
        loop {
            let r = GetMessageW(&mut msg, None, 0, 0);
            if r.0 <= 0 {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
