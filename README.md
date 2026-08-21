<p align="center">
  <img src="assets/logo.png" alt="Wallboxd logo" width="96" height="96">
</p>

<h1 align="center">Wallboxd</h1>

<p align="center">
  Turns your Letterboxd diary into your Windows desktop wallpaper and lock screen.
</p>

---

Wallboxd runs quietly in the system tray. On a schedule, it builds a
poster-wall image from your Letterboxd account **entirely locally** — no
server round-trip — and applies it as your desktop wallpaper (and, if you
enable it, your lock screen), sized to your primary monitor.

## Screenshots

<p align="center">
  <img src="assets/screenshots/desktop.png" alt="Wallboxd desktop wallpaper" width="45%">
  &nbsp;&nbsp;
  <img src="assets/screenshots/lockscreen.png" alt="Wallboxd lock screen" width="45%">
</p>

## Features

- System tray app — hidden window, no console, nothing to manage
- Poster wall sized to your primary monitor resolution, edge to edge, with
  every poster kept at its true aspect ratio — no cropping or stretching
- Configurable refresh interval (minimum 5 minutes)
- Wallpaper style: Fill, Fit, Stretch, Center, Tile, Span
- Optional lock screen — same poster wall, or a different Letterboxd user's
- Optional start-with-Windows autostart
- Live progress while refreshing ("Fetching posters (12/24)...") instead of a
  frozen-looking dialog — poster downloads run concurrently, so a full 4K
  wall composites in a couple of seconds
- Single instance, and refreshes can't collide with each other
- Offline-friendly: the cached wallpaper is re-applied at startup, and a
  failed refresh leaves your current wallpaper untouched

## Requirements

- Windows 10/11
- To build from source: a recent stable Rust toolchain (edition 2024)

## Install

Grab the latest release from the
[Releases page](../../releases), or build from source:

```
cargo build --release --locked
```

The binary lands at `target/release/Wallboxd.exe` — no installer needed for
the plain exe. An MSIX package (required for the lock screen feature, and
for Microsoft Store distribution) can also be built — see
[`packaging/README.md`](packaging/README.md).

## Usage

1. Run `Wallboxd.exe` (or launch "Wallboxd" from the Start menu if installed
   via MSIX). It starts in the system tray.
2. Right-click the tray icon → **Settings...**
3. Enter your Letterboxd username, pick a wallpaper style and refresh
   interval, and toggle autostart as you like. Press **OK**.
4. The first refresh fetches and applies your poster wall.

The tray menu offers **Refresh now** / **Settings...** / **Exit**.

## How it works

Wallboxd fetches your public Letterboxd diary RSS feed directly, downloads
each entry's poster from Letterboxd's own CDN, and composites them into an
edge-to-edge poster wall in-process, sized to your primary monitor. The
result is applied as your desktop wallpaper via the Windows API, and — for
the MSIX build — your lock screen too.

Everything runs locally: there's no server involved in generating the wall,
so there's nothing external to be slow or unavailable.

## Data

Everything lives in `%LOCALAPPDATA%\Wallboxd`:

| File              | Purpose                                            |
| ----------------- | --------------------------------------------------- |
| `config.json`     | Username, refresh interval, autostart, wallpaper style |
| `wallpaper.jpg`   | Cached poster wall (re-applied at startup)         |
| `lockscreen.jpg`  | Cached lock screen wall (only if it differs from the desktop one) |
| `Wallboxd.log`    | Append-only diagnostic log — there is no console output |

To fully remove Wallboxd: exit via the tray menu, delete
`%LOCALAPPDATA%\Wallboxd`, and remove the `Wallboxd` value under
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run` if autostart was
enabled.

## Development

See [`AGENTS.md`](AGENTS.md) for architecture and internals, and
[`packaging/README.md`](packaging/README.md) for MSIX packaging and Store
submission. CI runs `cargo fmt` → `cargo clippy -D warnings` →
`cargo build --release` → `cargo test` on every push.

---

Not affiliated with Letterboxd.
