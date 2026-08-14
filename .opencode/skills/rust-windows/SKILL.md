---
name: rust-windows
description: Use when writing or editing Win32 code in this repo via the `windows` crate — window procedures, tray icons, dialogs, registry access, wallpaper APIs, UTF-16 string handling, or any unsafe Win32 call. Covers `windows` crate API patterns and repo conventions that differ from plain Rust.
---

# Rust + Windows (Win32) development

This repo is a Windows-only Win32 app using the `windows` crate (v0.62). Every Win32 call is `unsafe`. There is no console (`#![windows_subsystem = "windows"]`) — diagnostics go to `%LOCALAPPDATA%\wallb\wallb.log` via `applog`, never `println!`.

## `windows` crate essentials

- **Features are opt-in**: every API used must be enabled as a feature in `Cargo.toml` (`windows = { version = "0.62.2", features = [...] }`). Adding a new API means adding its feature (e.g. `Win32_UI_WindowsAndMessaging`); a missing feature fails at compile time.
- **Wide strings**: compile-time literals use `windows::core::w!("...")`. Runtime strings need a null-terminated UTF-16 buffer: `s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>()`, then pass `PCWSTR::from_raw(buf.as_ptr())` or `Some(buf.as_mut_ptr().cast())` depending on the callee.
- **Two error styles** (easy to mix up):
  - Newer bindings return `Result<_, windows::core::Error>` — use `?` / `.map_err(|e| e.to_string())`. Examples: `SystemParametersInfoW`, `CreateWindowExW`, `CreateMutexW`.
  - Older bindings return a raw `WIN32_ERROR` — the repo converts with `.ok()` (turns it into `Result<(), Error>`), sometimes `.ok().ok()` (into `Option`). Examples: `RegOpenKeyExW`, `RegSetValueExW`, `RegDeleteValueW`. See `autostart.rs:11-19`.
- **Message params**: construct with `WPARAM(usize)` / `LPARAM(isize)`, read back via `.0`. Command IDs come from `wparam.0 & 0xFFFF`. `SendMessageW` returns `LRESULT` — read `.0 as u32` / `.0 as i32`.
- **Handles**: parent-window params are `Option<HWND>` (`None` = desktop). `CreateMutexW` returns `Option<HANDLE>`.
- **Registry**: open with `RegOpenKeyExW(HKEY_CURRENT_USER, w!("..."), None, KEY_WRITE, &mut hkey)`, write `REG_SZ` values as raw byte slices of the UTF-16 buffer (`std::slice::from_raw_parts(buf.as_mut_ptr().cast::<u8>(), buf.len() * 2)`), always `RegCloseKey`.

## Repo conventions

- **Modal dialog state via `thread_local!`** (`settings_dialog.rs:19-27`): safe only because the dialog is always modal (the caller blocks). Don't "fix" this to `GWLP_USERDATA` without a reason.
- **Config lives in a `thread_local!`** in `main.rs` (`STATE`); refresh runs on a spawned thread with a cloned `Config`.
- **Single instance**: named mutex `Local\WallbSingleInstanceMutex`; a second launch exits silently (`main.rs:61-71`).
- **Wallpaper style registry values must be written BEFORE `SPI_SETDESKWALLPAPER`** — Windows reads them at apply time, not after (`wallpaper.rs:74-88`).
- **Offline story is intentional**: `apply_cached()` re-applies the cached `wallpaper.jpg` at startup; any refresh failure leaves cache and current wallpaper untouched. Don't add a fallback path.
- **Edition 2024 let-chains** are used (`if let Some(cfg) = cfg && !cfg.username.is_empty()`) — keep them; requires a recent stable Rust toolchain.

## Verify

- Windows-only: `cargo build --release` won't compile on non-Windows.
- Gate order: `cargo fmt --all -- --check` → `cargo clippy --all-targets --all-features --locked -- -D warnings` → `cargo build --release --locked`. No tests exist.