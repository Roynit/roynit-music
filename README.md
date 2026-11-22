# Roynit Music

Native Rust music player for Windows, developed by **dang**.

[Downloads](https://roynit.dangz.org/?app=roynit-music) · [Development](docs/DEVELOPMENT.md) · [Extensions](docs/EXTENSIONS.md) · [Signing](docs/SIGNING.md)

## Run

Download `Roy-Music-<version>-Setup.exe` from the Roynit library and run it. Setup checks your PC, shows what is missing and installs it (FFmpeg and yt-dlp, with SHA-256 verification); no administrator rights or Visual C++ runtime are needed. It adds a Start menu shortcut, an optional desktop shortcut and an entry in Settings → Apps. The Windows ZIP on GitHub is a portable copy: open `riff-desktop.exe` or `Install Roy Music.cmd`. The executable keeps its previous filename for compatibility with installed Rust versions. The interface is a working preview; Audio Studio is intentionally absent.

FFmpeg/FFprobe are found on PATH, through RIFF_FFMPEG/RIFF_FFPROBE, or in Roy Music's own tools folder, where Setup (or Settings → Playback → Media engine) puts a verified copy. yt-dlp works the same way. Helpers are downloaded, never bundled. No Electron or Node application runtime is required.

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
