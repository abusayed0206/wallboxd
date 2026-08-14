#![windows_subsystem = "windows"]

mod applog;
mod autostart;
mod config;
mod settings_dialog;
mod tray;
mod wallpaper;

use config::Config;
use std::cell::RefCell;
use windows::core::w;
use windows::Win32::Foundation::{
    GetLastError, SetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT, POINT,
    WIN32_ERROR, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos, GetMessageW, KillTimer,
    PostQuitMessage, RegisterClassW, SetTimer, TranslateMessage, CW_USEDEFAULT, MSG,
    WM_COMMAND, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP, WM_TIMER, WNDCLASSW, WS_OVERLAPPED,
};

const TIMER_REFRESH: usize = 1;
const SINGLE_INSTANCE_MUTEX: windows::core::PCWSTR = w!("Local\\WallbSingleInstanceMutex");

thread_local! {
    static STATE: RefCell<Option<Config>> = const { RefCell::new(None) };
}

fn main() {
    let Some(_instance_guard) = acquire_single_instance() else {
        return;
    };

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
        "wallb - not configured".to_string()
    } else {
        format!("wallb - {}", cfg.username)
    }
}

fn spawn_refresh(cfg: Config) {
    std::thread::spawn(move || match wallpaper::refresh(&cfg) {
        Ok(()) => applog::info("wallpaper refreshed"),
        Err(e) => applog::error(&format!("refresh failed: {e}")),
    });
}

fn start_timer(hwnd: HWND, interval_minutes: u32) {
    let millis = interval_minutes.max(1) as u32 * 60_000;
    unsafe {
        SetTimer(Some(hwnd), TIMER_REFRESH, millis, None);
    }
}

fn class_name() -> windows::core::PCWSTR {
    w!("WallbMainWindow")
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
            w!("wallb"),
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
                let _ = GetCursorPos(&mut pt);
                tray::show_menu(hwnd, pt.x, pt.y);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = wparam.0 & 0xFFFF;
            match id as u32 {
                tray::ID_REFRESH => {
                    let cfg = STATE.with(|s| s.borrow().clone());
                    if let Some(cfg) = cfg {
                        if !cfg.username.is_empty() {
                            spawn_refresh(cfg);
                        }
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

                        let _ = KillTimer(Some(hwnd), TIMER_REFRESH);
                        start_timer(hwnd, new_cfg.interval_minutes);

                        tray::set_tooltip(hwnd, &tooltip_for(&new_cfg));

                        if let Err(e) = wallpaper::apply_cached(&new_cfg) {
                            applog::info(&format!("apply_cached after settings change: {e}"));
                        }
                        if !new_cfg.username.is_empty() {
                            spawn_refresh(new_cfg.clone());
                        }

                        STATE.with(|s| *s.borrow_mut() = Some(new_cfg));
                    }
                    LRESULT(0)
                }
                tray::ID_EXIT => {
                    tray::remove(hwnd);
                    PostQuitMessage(0);
                    LRESULT(0)
                }
                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
        WM_TIMER => {
            if wparam.0 == TIMER_REFRESH {
                let cfg = STATE.with(|s| s.borrow().clone());
                if let Some(cfg) = cfg {
                    if !cfg.username.is_empty() {
                        spawn_refresh(cfg);
                    }
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            tray::remove(hwnd);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
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
