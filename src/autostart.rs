use std::os::windows::ffi::OsStrExt;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_SZ, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
    RegSetValueExW,
};
use windows::core::{PCWSTR, w};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: PCWSTR = w!("Wallboxd");

// Must match packaging/AppxManifest.xml's <desktop:StartupTask TaskId="...">
// exactly, since that's how StartupTask::GetAsync finds the right task.
const TASK_ID: &str = "WallboxdStartupTask";

/// Registry Run keys and the classic Startup folder are both documented as
/// unsupported for MSIX/Desktop Bridge packages: Windows doesn't reliably
/// launch a packaged app from either, regardless of what's written there.
/// Packaged apps must declare a `windows.startupTask` extension in the
/// manifest and toggle it via the `Windows.ApplicationModel.StartupTask` API
/// instead, which is what shows up (and can be independently disabled) in
/// Task Manager's Startup tab. The plain `cargo build` exe has no package
/// identity and no such extension, so it keeps using the registry Run key;
/// this is the same identity check used for the lock screen API in
/// wallpaper.rs.
///
/// Always spawns a thread rather than running inline: confirmed by testing
/// that `StartupTask::RequestEnableAsync` (hit whenever a task moves from
/// Disabled back to Enabled, e.g. the app's own prior `Disable()` call, not
/// just a first-ever run) can block for well over 30s when called from the
/// main thread before the message loop is pumping, with the whole process
/// never reaching `create_main_window`. Every caller here already treats
/// this as fire-and-forget (none read a return value or rely on it finishing
/// before the next line), so moving it off-thread costs nothing.
pub fn set(enabled: bool) {
    std::thread::spawn(move || {
        if has_package_identity() {
            set_packaged(enabled);
        } else {
            set_unpackaged(enabled);
        }
    });
}

fn has_package_identity() -> bool {
    use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
    const APPMODEL_ERROR_NO_PACKAGE: u32 = 15700;
    let mut len: u32 = 0;
    unsafe { GetCurrentPackageFullName(&mut len, None) }.0 != APPMODEL_ERROR_NO_PACKAGE
}

fn set_packaged(enabled: bool) {
    use std::future::IntoFuture;
    use windows::ApplicationModel::{StartupTask, StartupTaskState};
    use windows::core::HSTRING;

    let result = || -> windows::core::Result<()> {
        let task = futures::executor::block_on(
            StartupTask::GetAsync(&HSTRING::from(TASK_ID))?.into_future(),
        )?;
        let state = task.State()?;

        if enabled {
            match state {
                StartupTaskState::Enabled => {}
                // The user (or an admin policy) turned this off directly in
                // Task Manager's Startup tab; only they can turn it back on
                // from there. RequestEnableAsync would just no-op back to
                // this same state, so there's nothing useful to do here.
                StartupTaskState::DisabledByUser | StartupTaskState::DisabledByPolicy => {
                    crate::applog::info(
                        "autostart: startup task was disabled by the user or policy in Task \
                        Manager > Startup, re-enable it there, Wallboxd can't override that",
                    );
                }
                _ => {
                    futures::executor::block_on(task.RequestEnableAsync()?.into_future())?;
                }
            }
        } else if state == StartupTaskState::Enabled {
            task.Disable()?;
        }

        Ok(())
    }();

    if let Err(e) = result {
        crate::applog::error(&format!("autostart: failed to update startup task: {e}"));
    }
}

fn set_unpackaged(enabled: bool) {
    let Some(hkey) = open_run_key() else { return };
    unsafe {
        if enabled {
            if let Ok(exe) = std::env::current_exe() {
                // Quoted so explorer.exe doesn't misparse an install path
                // containing spaces (e.g. "C:\Program Files\...") as
                // separate command-line arguments.
                let quoted = format!("\"{}\"", exe.display());
                let mut wide: Vec<u16> = std::ffi::OsStr::new(&quoted).encode_wide().collect();
                wide.push(0);
                let bytes = std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2);
                let _ = RegSetValueExW(hkey, VALUE_NAME, None, REG_SZ, Some(bytes));
            }
        } else {
            let _ = RegDeleteValueW(hkey, VALUE_NAME);
        }
        let _ = RegCloseKey(hkey);
    }
}

fn open_run_key() -> Option<HKEY> {
    let mut hkey = HKEY::default();
    // RegOpenKeyExW returns a raw WIN32_ERROR, not a Result: .ok() turns it
    // into Result<(), Error>, and the second .ok() turns that into Option<()>.
    unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_WRITE, &mut hkey) }
        .ok()
        .ok()
        .map(|_| hkey)
}
