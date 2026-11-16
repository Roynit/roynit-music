# Roynit Music

Native Rust music player for Windows, developed by **dang**.

[Downloads](https://roynit.dangz.org/?app=roynit-music) · [Development](docs/DEVELOPMENT.md) · [Extensions](docs/EXTENSIONS.md) · [Signing](docs/SIGNING.md)

## Run

Download the Windows installer from the Roynit library and run it with `--install --launch`, or use the Windows ZIP on GitHub: open `riff-desktop.exe` or `Install Roynit.cmd`. Installation uses your Windows user profile and creates a Start menu shortcut. The executable keeps its previous filename for compatibility with installed Rust versions. The interface is a working preview; Audio Studio is intentionally absent.

FFmpeg/FFprobe must be on PATH, or configured through RIFF_FFMPEG/RIFF_FFPROBE. yt-dlp can be installed from Settings with checksum verification. External media helpers are not bundled. No Electron or Node application runtime is required.

## Features

YouTube Music home/search/charts/browse/radio; YouTube/SoundCloud and public Spotify catalog search; playlists/favorites/history/local library/offline downloads; native playback/output devices/seek/repeat/shuffle/crossfade/sleep; lyrics/translation/karaoke; Windows media controls; native themes/visualizer/plugins; HTTP/SOCKS proxy; backup/migration; verified update staging and per-user installation.

Spotify supplies metadata. Audio is matched to YouTube Music with arrangement and extracted-duration checks; unavailable matching recordings are refused. Private account libraries require supported sign-in setup. Radio order comes from YouTube Music; no proprietary recommendation model is recreated or guaranteed account personalization.

## Updates and signing

Official updates and downloads use [roynit.dangz.org](https://roynit.dangz.org/?app=roynit-music), backed by Cloudflare D1/R2. The app checks at startup unless disabled in Settings, verifies newer downloads and offers Apply/restart. GitHub releases also retain the compatible `riff-rust-windows-x64.exe` update asset for older versions. Existing library locations are preserved. See [Cloudflare publishing](docs/CLOUDFLARE.md).

Read `signing-status.json` for actual Windows signature status. Early unsigned builds are labeled as such. SHA-256 verification is not a publisher certificate. Signing requires a real code-signing certificate; the project does not create or trust a fake certificate.

## Build

```powershell
cargo build --release --locked --features desktop --bins
pwsh -File scripts/package.ps1 -SkipBuild
```

Rust 1.100.0 is used in CI. See the development guide for tests and the headless JSON API. This repository contains the Rust application and supporting build/test files; old Electron source is not included. Required upstream and dependency notices remain in LICENSE and THIRD_PARTY_NOTICES.md.
