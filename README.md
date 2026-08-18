# Wallboxd

Sets your Windows desktop wallpaper (and lock screen) to a Letterboxd user's
poster wall.

Wallboxd runs quietly in the system tray. On a schedule, it builds a
poster-wall image for your Letterboxd account **locally** — no server
round-trip — saves it to disk, and applies it as your desktop wallpaper,
sized to your primary monitor.

> Formerly "wallb." If you're upgrading from that version, Wallboxd
> automatically carries over your `config.json` and cached wallpaper the
> first time it runs — see "Data" below.

## Features

- System tray app — hidden window, no console, nothing to manage
- Poster wall sized to your primary monitor resolution
- Configurable refresh interval (minimum 5 minutes)
- Wallpaper style: Fill, Fit, Stretch, Center, Tile, Span
- Optional lock screen: same poster wall (or a different Letterboxd user's) —
  **works from the MSIX build** (see "Packaging" below); the plain `.exe`
  build can't do this at all (Windows requires package identity), see "Lock
  screen" under "How it works" for details
- Optional start-with-Windows autostart
- Progress dialog for manual refreshes: an indeterminate animation while your
  diary loads, then a real progress bar ("Fetching posters (12/24)...") once
  poster downloads start, so a refresh in progress never looks stuck
  (failures show the error in the dialog); timer auto-refreshes stay silent
- Poster downloads run concurrently (up to 8 at once) — a 40-poster 4K wall
  composites in a couple of seconds rather than the over a minute sequential
  fetching would take
- Single instance — a second launch exits silently
- Offline-friendly: the cached wallpaper is re-applied at startup, and a failed
  refresh leaves your current wallpaper untouched

## Requirements

- Windows (Win32 API; does not build on other platforms)
- Rust toolchain (recent stable — the crate uses edition 2024) to build from source

## Build

```
cargo build --release --locked
```

The binary lands at `target/release/Wallboxd.exe`. No external install steps
for the plain exe — for the MSIX package (needed for the lock screen feature
to work, and for Store distribution), see `packaging/README.md`.

## Usage

1. Run `Wallboxd.exe` (or launch "Wallboxd" from the Start menu if installed
   via MSIX). It starts in the system tray.
2. Right-click the tray icon → **Settings...**
3. Enter your Letterboxd username, pick a wallpaper style and refresh interval,
   and toggle autostart as you like. Press **OK**.
4. The first refresh fetches and applies your poster wall.

The tray menu offers **Refresh now** / **Settings...** / **Exit**. If no username
has been configured yet, the tray tooltip reads "Wallboxd - not configured".

## Data

Everything lives in `%LOCALAPPDATA%\Wallboxd`:

| File           | Purpose                                                        |
| -------------- | -------------------------------------------------------------- |
| `config.json`  | Username, refresh interval, autostart, wallpaper style          |
| `wallpaper.jpg`| Cached poster wall (re-applied at startup)                      |
| `lockscreen.jpg`| Cached lock screen wall — only written when the lock screen username differs from the desktop one |
| `Wallboxd.log` | Append-only diagnostic log — there is no console output         |

If `%LOCALAPPDATA%\wallb\config.json` exists (a pre-rename install) and
`%LOCALAPPDATA%\Wallboxd\config.json` doesn't yet, Wallboxd copies the old
config and cached images over on first launch. The old `wallb` directory and
autostart entry are never deleted automatically — remove them yourself once
you've confirmed Wallboxd is working.

To fully remove Wallboxd: exit via the tray menu, delete
`%LOCALAPPDATA%\Wallboxd`, and remove the `Wallboxd` value under
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run` if autostart was enabled
(plus the old `wallb` folder/value too, if you're upgrading from that
version and haven't cleaned it up).

## How it works

Wallboxd fetches your public Letterboxd diary RSS feed directly, downloads
each entry's poster from Letterboxd's own CDN (up to 8 posters at once —
network latency per poster varies a lot in practice, anywhere from tens of
milliseconds to several seconds, so fetching serially would make a full wall
take well over a minute), and composites them into an edge-to-edge poster
wall in-process (no cropping or stretching — each poster keeps its true
aspect ratio), sized to the primary monitor. The result is written to
`wallpaper.jpg` and applied via `SystemParametersInfoW`. The wallpaper style
is written to the registry (`WallpaperStyle` / `TileWallpaper`) *before* the
wallpaper is applied — Windows reads those values at apply time.

A manual refresh (tray "Refresh now", or Settings → Refresh Now/OK) shows a
progress bar that starts indeterminate (while your diary loads) and switches
to real, determinate progress as poster downloads complete — e.g. "Fetching
posters (12/24)...". Timer-driven auto-refreshes stay silent as before.

### Lock screen

If enabled, Wallboxd applies the same wall (or a different Letterboxd user's,
if you set one) to the lock screen. **This works, but only from the MSIX
build** — the plain `.exe` build can't do it at all, because Windows' lock
screen APIs require the calling process to have package (MSIX/APPX) identity,
confirmed directly via `GetCurrentPackageFullName` returning
`APPMODEL_ERROR_NO_PACKAGE` for an unpackaged process.

Getting this working took a few rounds of testing to land on the right API:
Windows' newer `UserProfilePersonalizationSettings.TrySetLockScreenImageAsync`
resolves to a `bool` instead of throwing on failure, and — confirmed directly,
not assumed — it kept returning `false` even with package identity present,
lock screen mode confirmed as genuine "Picture," and no competing lock-screen
app installed. The API that actually works is the older
`LockScreen.SetImageFileAsync`, combined with copying the image to a
uniquely-named file before each call. Confirmed working by reading
`LockScreen.OriginalImageFile()` back immediately after setting it, which
reported exactly the freshly-applied file — not just "no error was thrown."
See `AGENTS.md`'s "Lock screen" section for the full investigation if you're
curious how many wrong turns that took. Desktop wallpaper is unaffected by
any of this and works identically in both the `.exe` and MSIX builds.

This used to be done by fetching a pre-composited image from a Cloudflare
Worker (the [posterboi API](https://lb.sayed.app), which still exists and
still uses this same layout), but that endpoint occasionally hit its Workers
CPU-time limit and failed. Generating the wall locally removes that
dependency entirely.

## Packaging

Wallboxd ships (or will ship) on the Microsoft Store as a real product with
Store-assigned identity — see `packaging/README.md` for the MSIX manifest,
build script, product identity values, and local sideload-testing
instructions.

## Development

See `AGENTS.md` for architecture, verification commands, and gotchas. CI runs
`cargo fmt --check` → `cargo clippy -D warnings` → `cargo build --release` →
`cargo test` on `windows-latest`. Pushing a `v*.*.*` tag triggers a release build
that fails unless the tag matches the `version` in `Cargo.toml`.

Not affiliated with Letterboxd.
