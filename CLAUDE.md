# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Skylight is a small native Windows weather app: Rust + Slint (software renderer, no web engine), data from Open-Meteo. Goals are low memory (~30 MB) and near-zero idle CPU, so keep the default feature set and avoid per-frame work.

## Commands

```
cargo build --release          # release exe at target/release/skylight.exe (size-optimised, LTO, panic=abort, stripped)
cargo run                      # debug build; keeps the console, so panics are visible
cargo test                     # unit tests live in src/view.rs (cargo test <name> for one)
cargo fmt --check              # CI enforces rustfmt (max_width 140, see rustfmt.toml)
cargo clippy --all-targets -- -D warnings   # CI enforces zero clippy warnings
```

- Release builds use `windows_subsystem = "windows"` and `panic = "abort"`: a panic shows up only as exit code `0xc0000409`. Reproduce with a debug build to see the message.
- A running Skylight locks `target\release\skylight.exe`; quit it from the tray before rebuilding release.
- `.cargo/config.toml` is local-only (gitignored); it just remaps build paths. CI sets the same via `RUSTFLAGS`.
- Optional renderers (`--features skia` / `femtovg`) exist but are off by default (much more memory).
- Dev aids: env vars `SKYLIGHT_FONT=<family>` and `SKYLIGHT_VIEW=air|sky|settings0..settings4` (handled in `main.rs`).
- Slint prints several known warnings on every build (binding loop in `AppWindow`, `padding` on `Text`, unused `Chip`). They are pre-existing, not new breakage.

## Architecture

Data flows one way: **fetch → `State` → `view::apply` → Slint properties**. The UI only draws; every user action is a Slint callback handled in `main.rs`.

- `src/main.rs`: owns the global `STATE` (`Mutex<State>`: settings, parsed `Weather`, saved-places temps, status). `fetch()` runs on a worker thread (cache → network → stale-cache fallback via `load()`), writes into `STATE`, then `upgrade_in_event_loop(redraw)`. `redraw()` calls `view::apply`. The `change!` macro is the standard "mutate settings, save, redraw" path for settings callbacks. Also builds timers (refresh, icon animation, 30 s clock redraw), tray, single-instance wiring, and `apply_zoom`.
- `src/api.rs`: Open-Meteo requests and parsing into `Weather` (hourly `hours`, `days`, current `cur`). `Weather.time`/`idx`/`today` identify "now" in the hour array; `Weather::roll()` advances them from `base: Instant` so a long-lived window stays on the current hour between fetches (called from `redraw`). Cached data is re-based by its age in `fetch`.
- `src/view.rs`: all presentation logic: unit formatting, WMO weather-code → icon/name, tile construction (`build_tiles`), hourly chart geometry (`hour_cols`: 12 columns, Now then a constant 2 h step, Catmull-Rom path, sunrise/sunset markers placed by absolute minutes), and `apply()` which sets every Slint property.
- `src/settings.rs`: `Settings` persisted as JSON in `%APPDATA%\Skylight\settings.json` (`#[serde(default)]`, so adding a field is backward compatible). `src/cache.rs` caches raw API responses on disk by key. `src/system.rs`: Windows-only autostart registry and single-instance/show-event plumbing.
- `ui/app.slint`: the whole UI (single file). `Theme` global holds the palette and follows system/light/dark; `Anim` global is driven by a Rust timer (Slint's own 60 fps animations cost too much CPU, so icon animation is a 4 fps tick, opt-in per `WIcon` instance). Icons are hand-drawn `Path`s.

### Gotchas that span files

- **Chart path viewbox**: Slint `Path` keeps aspect ratio, so the hourly chart's SVG viewbox must match its on-screen box. `HourlyCard` reports its aspect to Rust (`chart-aspect` callback → `State.aspect`), and `hour_cols` regenerates the path with a matching viewbox height. Width is always 1200 units. The card height is also tied to the left rail's "Saved places" card via `target-h`.
- **Hour data contract**: `HourData.op` is the rain chance as 0..1 (drives bar height), `y` is the curve height fraction, and column 0 is always "Now". `api::parse_weather` and `Weather::roll` require at least 24 spare hours after `idx`.
- **UI zoom** (Ctrl + / Ctrl − / Ctrl 0, also in Settings → Appearance): implemented by dispatching `WindowEvent::ScaleFactorChanged` on the Slint window (`apply_zoom` in `main.rs`), stored as `zoom` percent (60–300) in settings. After changing the scale it must also dispatch `Resized` with the unchanged physical size in new logical units; skipping that makes the software renderer panic ("buffer ... too small"). `apply_zoom` is idempotent and re-run on show/timer because the OS can reset the scale factor. Key handling is a root `FocusScope` in `AppWindow` so keys bubble from anywhere.
- **Colour semantics** (design direction): neutrals do the layout; amber = temperature, blue = rain/interactive accent, status colours (`Theme.tone()`) only for status. Don't add other accent colours.
- **Fonts**: Lato is embedded from `assets/fonts` and imported at the top of `app.slint`.
