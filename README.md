# Skylight

A small native Windows weather app. Rust + [Slint](https://slint.dev) (software renderer, no web engine), data from [Open-Meteo](https://open-meteo.com) (free, no key). About 30 MB of memory and near-zero CPU when idle.

## Install

1. Download the latest `skylight-<version>-windows-x64.zip` from the repository's Releases page.
2. Unzip it anywhere and run `skylight.exe`.

To uninstall, delete the folder and `%APPDATA%\Skylight`.

### Windows SmartScreen

The executable is not code-signed, so Windows may show "Windows protected your PC". Click "More info", then "Run anyway". To check the download, compare its SHA-256 with the one on the release page:

```
Get-FileHash .\skylight-<version>-windows-x64.zip -Algorithm SHA256
```

## Features

- Current conditions, hourly chart (hover for details), 6-day forecast, and tiles for wind, pressure, air quality, UV, visibility, sun and moon phase.
- Saved places with a city search.
- Light, dark or system theme, accent colour, and units for temperature, wind, pressure and distance.
- Responses are cached on disk (30 minutes by default) so reopening the app does not hit the network.

Settings and the cache live in `%APPDATA%\Skylight`.

## Build

```
cargo build --release
```

Needs Rust 1.92 or newer and the Windows SDK (for the icon resource). Optional renderers: `--features skia` or `--features femtovg` give GPU rendering at roughly 150 MB or more of memory.

## Credits

Lato font by Łukasz Dziedzic, SIL Open Font License 1.1 (`assets/fonts`). Weather data by Open-Meteo.
