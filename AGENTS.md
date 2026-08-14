# AGENTS.md

Windows-only Rust system-tray app that sets the desktop wallpaper to a Letterboxd user's poster wall, fetched from the posterboi API (`https://lb.sayed.app`). User-facing docs live in `README.md`; this file is agent guidance.

## Editing Win32 code

- Load the repo-local skill `.opencode/skills/rust-windows/SKILL.md` before touching any Win32 code (window procedures, tray, dialogs, registry, wallpaper). Key gotchas it covers:
  - `windows` crate **features are opt-in per API** — using a new API means adding its feature in `Cargo.toml`; a missing feature fails at compile time.
  - Runtime strings need null-terminated UTF-16 buffers (`s.encode_utf16().chain(std::iter::once(0))`); compile-time literals use `w!("...")`.
  - Two error styles coexist: newer bindings return `Result<_, Error>` (`?`/`.map_err`), older ones return a raw `WIN32_ERROR` converted with `.ok()` (sometimes `.ok().ok()`). See `autostart.rs:11-19`.
- `Cargo.lock` is committed — CI and the release workflow build with `--locked`.

## Build / verify

- **Windows-only**: Win32 via the `windows` crate; CI runs on `windows-latest`. Won't build on other platforms.
- **Edition 2024** with let-chains (`if let Some(cfg) = cfg && !cfg.username.is_empty()` in `main.rs`) — requires a recent stable Rust toolchain.
- CI gate order (all must pass): `cargo fmt --all -- --check` → `cargo clippy --all-targets --all-features --locked -- -D warnings` → `cargo build --release --locked` → `cargo test --release --locked`.
- **No tests exist** — `cargo test` is a no-op gate. Verification is fmt + clippy + release build.
- `#![windows_subsystem = "windows"]` — no console. Diagnostics go to `%LOCALAPPDATA%\wallb\wallb.log` via `applog`; never `println!`.

## Release

- Pushing a tag `v*.*.*` triggers `release.yml`, which **fails if the tag version ≠ Cargo.toml `version`**. Bump Cargo.toml before tagging.
- Packages `target/release/wallb.exe` into `dist/wallb-$version-windows-x64.zip`.

## Runtime data

- Everything lives in `%LOCALAPPDATA%\wallb` (via `config::data_dir()`): `config.json`, `wallpaper.jpg` (cache), `wallb.log`. `LOCALAPPDATA` is required — the code `.expect()`s it.

## Architecture

- `main.rs` — hidden, never-shown message-only window that owns the tray icon, refresh timer, and settings dialog. Single-instance via named mutex `Local\WallbSingleInstanceMutex` (second launch exits silently). Refresh runs on a spawned thread; current `Config` lives in a `thread_local!`.
- `tray.rs` — tray icon + popup menu (Refresh now / Settings... / Exit). Uses stock `IDI_APPLICATION` icon — no `.ico` resource is embedded.
- `settings_dialog.rs` — modal Win32 dialog; state via `thread_local!` slots (safe because it's always modal). Interval parses with min 5, invalid input → 60. `poster_count` is hardcoded `None` from the dialog.
- `wallpaper.rs` — fetches the poster wall, writes `wallpaper.jpg`, applies via `SystemParametersInfoW(SPI_SETDESKWALLPAPER)`. `reqwest` blocking + rustls (no native-tls), 20s timeout.
- `config.rs` — serde JSON config; `WallpaperStyle` maps to Windows registry values.
- `autostart.rs` — `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, exe path written quoted.
- `applog.rs` — append-only timestamped log.

## Gotchas

- **Wallpaper style registry values must be written BEFORE `SPI_SETDESKWALLPAPER`** — Windows reads them at apply time, not after.
- Offline story is intentional: `apply_cached()` re-applies the cached `wallpaper.jpg` at startup; any refresh failure leaves the cache and current wallpaper untouched. No separate fallback path.
- `WallpaperStyle` registry encoding: Fill=10/0, Fit=6/0, Stretch=2/0, Center=0/0, Tile=0/1, Span=22/0.

## posterboi API — `https://lb.sayed.app`

Public, no auth, no sessions. Backs the wallpaper fetch. Source: posterboi repo `API.md`.

### `GET /api/wallpaper.jpg` — combined wall (the endpoint this app uses)

- Params: `username` (required), `w` (default 1920, clamped `[64, 4096]`), `h` (default 1080, clamped `[64, 4096]`), `count` (default: as many as fit, capped at 60).
- Returns one pre-composited JPEG, `Cache-Control: public, max-age=86400` (24h), cached by `username`+`w`+`h`+`count`.
- Generation is slow (~3–10s); designed to be hit ~once/day per combination, not per refresh.
- Errors: `400` missing username/invalid size, `404` no such user or no diary entries, `500` fetch/parse failure.

### `GET /api/diary.json` — recent diary as JSON

- Params: `username` (required), `h` (poster source height, default 345), `test` (any value → bundled offline sample data).
- Returns up to 200 entries: `title`, `year`, `poster` (direct URL on `a.ltrbxd.com`, no auth), `uri`, `watched` (ms epoch), `rating` (`{text, score}`).
- Cached 1h by `username`+`h`. Errors: `400`/`404`/`500`.

### `GET /api/proxy` — single poster re-serve

- Params: `src` (required, must be `https://` on `ltrbxd.com` or subdomain — else `403`), `h` (optional resize).
- Returns image bytes with `Access-Control-Allow-Origin: *`, 24h cache. Errors: `400`/`403`/passthrough of CDN status.

### Poster sizing (`h`) — critical gotcha

- Letterboxd's CDN only honors an **exact 2:3 ratio** AND a **hard allowlist of heights**: `345, 450, 690, 750, 900, 1500, 1800, 3000`. Anything else is rejected.
- Both endpoints snap `h` up to the smallest allowlisted height ≥ your value (`h=800` → `900`); below 345 / above 3000 clamps to the nearest end.
- Request `h` = the actual pixel height you'll draw the poster at — more wastes bandwidth, less upscales and looks soft.