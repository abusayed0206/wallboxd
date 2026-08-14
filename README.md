# wallb

Sets your Windows desktop wallpaper to a Letterboxd user's poster wall.

wallb runs quietly in the system tray. On a schedule, it fetches a pre-composited
poster-wall image for your Letterboxd account from the
[posterboi API](https://lb.sayed.app), saves it to disk, and applies it as your
desktop wallpaper — sized to your primary monitor.

## Features

- System tray app — hidden window, no console, nothing to manage
- Poster wall sized to your primary monitor resolution
- Configurable refresh interval (minimum 5 minutes)
- Wallpaper style: Fill, Fit, Stretch, Center, Tile, Span
- Optional start-with-Windows autostart
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

The binary lands at `target/release/wallb.exe`. No external install steps.

## Usage

1. Run `wallb.exe`. It starts in the system tray.
2. Right-click the tray icon → **Settings...**
3. Enter your Letterboxd username, pick a wallpaper style and refresh interval,
   and toggle autostart as you like. Press **OK**.
4. The first refresh fetches and applies your poster wall.

The tray menu offers **Refresh now** / **Settings...** / **Exit**. If no username
has been configured yet, the tray tooltip reads "wallb - not configured".

## Data

Everything lives in `%LOCALAPPDATA%\wallb`:

| File           | Purpose                                                        |
| -------------- | -------------------------------------------------------------- |
| `config.json`  | Username, refresh interval, autostart, wallpaper style          |
| `wallpaper.jpg`| Cached poster wall (re-applied at startup)                      |
| `wallb.log`    | Append-only diagnostic log — there is no console output         |

To fully remove wallb: exit via the tray menu, delete `%LOCALAPPDATA%\wallb`, and
remove the `wallb` value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
if autostart was enabled.

## How it works

wallb calls the posterboi API's combined-wall endpoint with the primary monitor's
size:

```
GET https://lb.sayed.app/api/wallpaper.jpg?username=<user>&w=<width>&h=<height>[&count=N]
```

The response is cached to `wallpaper.jpg` and applied via `SystemParametersInfoW`.
The wallpaper style is written to the registry (`WallpaperStyle` / `TileWallpaper`)
*before* the wallpaper is applied — Windows reads those values at apply time.

## Development

See `AGENTS.md` for architecture, verification commands, and gotchas. CI runs
`cargo fmt --check` → `cargo clippy -D warnings` → `cargo build --release` →
`cargo test` on `windows-latest`. Pushing a `v*.*.*` tag triggers a release build
that fails unless the tag matches the `version` in `Cargo.toml`.

Not affiliated with Letterboxd.
