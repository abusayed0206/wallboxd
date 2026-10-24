//! Shared DPI-correct UI font and geometry helpers.
//!
//! Wallboxd is per-monitor DPI aware (see `main.rs`), so window coordinates
//! are always *physical* pixels. The dialogs are authored in 96-DPI "design
//! units", which means both the font and every control rectangle must be
//! scaled for the monitor the dialog actually opens on.
//!
//! The font is the subtle half: `SystemParametersInfoW(SPI_GETNONCLIENTMETRICS)`
//! returns the message font for the *system* DPI, which on a laptop whose
//! display scale differs from the design layout produces text taller than the
//! label rectangles it sits in, visibly overlapping rows. `SystemParametersInfoForDpi`
//! asks for the metrics at one specific DPI instead.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use windows::Win32::Foundation::WPARAM;
use windows::Win32::Graphics::Gdi::{CreateFontIndirectW, HFONT};
use windows::Win32::UI::HiDpi::SystemParametersInfoForDpi;
use windows::Win32::UI::WindowsAndMessaging::{NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS};

/// Cached `HFONT`-as-`WPARAM` for `dpi`, created on first use. One font per
/// distinct DPI is all a session will ever need.
pub fn message_font(dpi: u32) -> WPARAM {
    static FONTS: OnceLock<Mutex<HashMap<u32, usize>>> = OnceLock::new();
    let fonts = FONTS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut fonts = fonts
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if let Some(&raw) = fonts.get(&dpi) {
        return WPARAM(raw);
    }

    let mut ncm = NONCLIENTMETRICSW {
        cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
        ..Default::default()
    };
    let raw = unsafe {
        // Requires Windows 10 1607+; the MSIX manifest already targets
        // 10.0.17763, so no fallback dance is needed.
        let _ = SystemParametersInfoForDpi(
            SPI_GETNONCLIENTMETRICS.0,
            std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
            Some(&mut ncm as *mut _ as *mut core::ffi::c_void),
            0,
            dpi,
        );
        let font: HFONT = CreateFontIndirectW(&ncm.lfMessageFont);
        // A null HFONT makes WM_SETFONT fall back to the system default font
        // rather than fail, which is an acceptable degraded result.
        std::mem::transmute::<HFONT, usize>(font)
    };
    fonts.insert(dpi, raw);
    WPARAM(raw)
}

/// Converts a 96-DPI design unit to physical pixels at `dpi`.
pub fn scale(dpi: u32, value: i32) -> i32 {
    (value as f64 * dpi as f64 / 96.0).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Gdi::{GetObjectW, HGDIOBJ, LOGFONTW};

    /// Reads back the actual `lfHeight` of the font cached for `dpi`, rather
    /// than trusting that `SystemParametersInfoForDpi` did something useful.
    fn font_height(dpi: u32) -> i32 {
        let font = message_font(dpi);
        let mut lf = LOGFONTW::default();
        let copied = unsafe {
            GetObjectW(
                HGDIOBJ(font.0 as *mut core::ffi::c_void),
                std::mem::size_of::<LOGFONTW>() as i32,
                Some(&mut lf as *mut _ as *mut core::ffi::c_void),
            )
        };
        assert!(copied > 0, "GetObjectW failed for the font at {dpi} DPI");
        lf.lfHeight
    }

    /// The Settings dialog's labels overlapped at 125%/150% scaling because it
    /// used the system-DPI message font inside a 96-DPI layout. The fix only
    /// holds if asking for the metrics at 144 DPI actually yields a ~1.5x
    /// taller font.
    #[test]
    fn message_font_scales_with_dpi() {
        let at_96 = font_height(96);
        let at_144 = font_height(144);
        assert!(at_96 < 0, "expected a negative font height, got {at_96}");
        assert!(at_144 < 0, "expected a negative font height, got {at_144}");
        let ratio = at_144 as f64 / at_96 as f64;
        assert!(
            (ratio - 1.5).abs() < 0.15,
            "font height ratio should be ~1.5x (96 DPI: {at_96}, 144 DPI: {at_144})"
        );
    }

    #[test]
    fn design_units_scale_to_physical_pixels() {
        assert_eq!(scale(96, 380), 380);
        assert_eq!(scale(120, 12), 15);
        assert_eq!(scale(144, 380), 570);
        assert_eq!(scale(144, 16), 24);
    }
}
