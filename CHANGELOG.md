# Roy Music 0.1.10

- Far fewer "No verified recording matching ..." errors on Spotify and SoundCloud songs, remixes included. When the first search finds nothing playable, Roy Music now widens it: the lead artist alone, the bare title, accent-free spelling, and YouTube videos as well as songs. It also stops requiring every credited artist to match, accepts an artist named in the video title, and ignores accents. Wrong versions are still refused: a different artist, an original instead of a remix, a live cut, or a different length.

# Roy Music 0.1.9

- Fewer freezes while playing, and no long wait to resume: about twenty seconds of decoded sound is now kept ahead of the speakers (it was under a tenth of a second), the network side reads ahead on its own thread and retries a failed or slow request, and a stalled connection is given up after 8 seconds instead of 20.
- The next songs in your queue get their stream links ready while the current one plays, so Next starts at once.

# Roy Music 0.1.8

- Mixes for you now builds playlists from your taste, the way YouTube does: "Made for you", "On repeat" (what you play and love most), "Discover" (new artists near your taste) and "Because you liked ..." mixes built around songs you hearted. A heart counts far more than a play. Mixes hold up to 40 songs.
- Each mix has a visible + to add it to your playlists (a check once it is there), next to the options menu.

# Roy Music 0.1.7

- Playback starts in about 0.2 s instead of 3-6 s on many songs. YouTube refused the whole-file request FFmpeg makes for larger tracks, which forced a slow fallback; Roy Music now relays it in small ranges. Stream links are also prepared in advance for the songs on your home page, and the connection is warmed when the app opens.
- Better recommendations: Mixes now learn from skips and repeats, favour recent listening, and add "Made for you" (songs several of your seeds lead to) and "Discover" (new artists near your taste). Six seeds instead of three, and your home picks come from the blended mix.

# Roy Music 0.1.6

- Roy Music icon (black R on a green disc) is now embedded in the app and Setup, so Explorer, Start, the taskbar and shortcuts show it.

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
