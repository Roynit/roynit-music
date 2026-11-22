# Roy Music 0.1.5

- A real Windows Setup: a branded wizard that checks the PC, lists what is missing and installs it with animated progress. It installs FFmpeg/FFprobe (the media engine) and yt-dlp with SHA-256 verification, creates shortcuts and adds Roy Music to Settings → Apps with an uninstaller.
- The app and Setup no longer need the Visual C++ redistributable (the C runtime is linked statically).
- In-app updates run the same Setup in update mode and relaunch Roy Music when done.
- Settings → Playback → Media engine, and an "Install media engine" button on the missing-FFmpeg error, install a checked copy on demand.
- Interface rebuilt toward a Spotify-style layout: floating rounded panels on black, real bold type, a green accent, a greeting with quick picks, cards with hover Play, slim seek and volume bars, and eased hover and page transitions (all off with Reduce motion).
- Roynit ID profile in the Spotify style: a round profile picture you can choose, crop-to-square and remove, a bio, a tinted header, top artists and an Edit profile dialog. Needs the updated account Worker (migrations `0002_profile.sql` and `0003_links.sql`); older servers keep working and just say pictures are not supported yet.
- Link Google / YouTube and Spotify from your Roynit ID profile. Settings → Accounts then connects automatically (likes, playlists, Mixes, Spotify imports) with no client ID; the server keeps the access and the app only gets short-lived tokens. Unlinking disconnects the app too.
- Sign-in is now a dialog over the app, and it is required before first use. Settings opens as a dialog instead of switching tabs. New Import tab: pick Spotify, YouTube Music, SoundCloud, Apple Music, Deezer, Yandex, TikTok or VK, paste a link and choose a new playlist or Favorites. Spotify account linking is hidden while Spotify is down.

# Roy Music 0.1.4

- Windows Setup downloads and verifies yt-dlp when missing so online playback can start on a fresh installation.
- Existing working yt-dlp installations and music library data are retained.

# Roy Music 0.1.3

Windows x64 release by dang.

- Roynit ID with email or username sign-in, Google sign-in, profiles, and opt-in music library sync.
- Search across YouTube Music, Spotify, and SoundCloud, with instant suggestions and grouped search results.
- Redesigned library, player, and Windows title bar.
- Expanded lyrics with line and word modes, progressive highlighting, and fallback timing.
- Standalone per-user setup executable. Existing local music data is kept in its original directory.

The installer is unsigned. External media tools may be required for some sources.

# Roynit Music 0.1.2

First standalone Rust-source release by dang.

- Official update channel, startup check and verified staging.
- Standalone assets and required notices; old Electron source excluded.
- Public Spotify search, verified recording selection, faster repeat search and shared stream preparation.
- Buffered atomic library writes, decoder failure handling, queue cancellation and extension isolation.
- Account controls, original appearance import, support-report export and receipt-verified rollback preparation.

Windows x64 preview. External FFmpeg/FFprobe/yt-dlp required. This release is unsigned unless signing-status.json explicitly reports valid signatures. Private account end-to-end and long-duration/regional/device coverage remain subject to live validation. JavaScript/CSS extensions need native rewrites.
