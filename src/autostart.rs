use std::os::windows::ffi::OsStrExt;
use windows::core::{w, PCWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_WRITE, REG_SZ,
};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: PCWSTR = w!("wallb");

fn open_run_key() -> Option<HKEY> {
    let mut hkey = HKEY::default();
    unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_WRITE, &mut hkey) }
        .ok()
        .map(|_| hkey)
}

pub fn set(enabled: bool) {
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
