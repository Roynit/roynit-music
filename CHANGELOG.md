# Roy Music 0.3.1

- Orbs shop and daily listening quests: earn Orbs by listening, spend them on avatar decorations, nameplates, profile frames and effects.
- What friends wear shows on their avatar, rows and profile. Open the shop from the menu or your profile.

# Roy Music 0.3.0

- Friend profile cards show the friends you share, and since when you are friends.
- Profile pictures are no longer stretched out of shape.

# Roy Music 0.2.9

- Notifications for new messages from friends, with a short "ting".
- A good-morning wish each morning, and a happy-birthday wish (notification and a card in the app) on your birthday.
- Add your birthday in your profile. Only you see it.

# Roy Music 0.2.8

- Dislike: a thumbs-down button next to the heart. A disliked song is never suggested, queued or mixed in again; its artist ranks lower and is left out only after repeated dislikes. Syncs with your account.
- Blend with a friend from the friend's menu, listening stats in the menu, podcasts in search, and artist names open the artist's page.
- Fixed "Unknown track ID" when pressing the heart or dislike.

# Roy Music 0.2.7

- Friends: "People you may know" suggests friends of your friends, with one click to add.
- Your library now follows your account automatically; the manual sync buttons and switch are gone.
- Roomier menus and drop-downs with a soft shadow.
- Smoother playback of long songs and a fix for the crop slider.

# Roy Music 0.2.6

- Roy Music can start with Windows, on by default. It opens quietly in the tray, so no window pops up when you sign in. Turn it off in Settings → Playback → "Start with Windows" or in Task Manager → Startup apps; either way the switch in Settings follows.
- Opening Roy Music while it is already running (for example from the shortcut after it started with Windows) brings the running window forward instead of failing or opening a second copy.
- "Show Roy Music" in the tray menu now reliably brings the window back from the tray.
- Updating removes the leftover "Riff Rust" shortcut from Start and the desktop.

# Roy Music 0.2.5

- Fixed lyrics that began with page clutter ("39 Contributors", a list of translations, a blurb ending in "Read More") on some songs. Lines are also joined correctly when a phrase is annotated, and lyrics saved with the clutter are fetched again.
- Your profile card in the Friends sidebar now fits the width of the Direct Messages column. Tap the Status row to switch between Online and Invisible.

# Roy Music 0.2.4

- Confirm your email: after you create an account with a password, Roy Music emails a 6-digit code and asks for it before you continue. Fix a mistyped address from the same screen. The screen reminds you to look in Spam or Junk if the email does not show up.
- Forgot your password? Tap "Forgot password?" on the sign-in form, enter your email or username, and a code is emailed to the (confirmed) address. Enter it with a new password to get back in; your other devices are signed out.

# Roy Music 0.2.3

- Updates install themselves: when a newer version exists, Roy Music downloads it quietly (checked with SHA-256) and installs it the next time you open the app. Turn it off in Settings → Data & updates → "Install updates automatically".
- Click your card at the bottom of the Friends sidebar to open your profile like Discord does: cover, picture, name, @username, bio, what you are listening to, Edit profile, your status (Online or Invisible) and your Orbs, the currency for the shop that is coming later. Edit profile now opens over any page.
- Discord now says "Listening to Roy Music" with the Roy Music icon (a new Discord application replaces the old "Riff" one, and saved settings are moved over automatically).
- Smaller fixes to the friend search box and the Add friend button.

# Roy Music 0.2.2

- Passwords now need only 6 characters (it was 12), when you create an account and when you set or change one.

# Roy Music 0.2.1

- Change your username in Settings → Accounts. Every username belongs to one person: as you type, Roy Music tells you whether it is free or already taken, and the server refuses a taken one.
- If you signed up with Google, you can now set a password there, so you can also sign in with your email or username. Accounts with a password can change it (needs the current one); changing it signs your other devices out.
- If you signed up with an email and password, you can link Google from the same place. After linking, "Continue with Google" signs you in to this same account.

# Roy Music 0.1.11

- Friends: a new Friends button beside Home opens a space like Discord's. Add friends by Roynit username, accept or decline requests, see who is online and the song each friend is listening to right now (with a live progress bar), and chat with direct messages. Set yourself to Invisible from your status card.
- Profiles show a cover picture behind the avatar. Choose or remove yours in Edit profile; friends see it on your profile card.
- Playlists can be reordered by dragging the grip on the right of a tile, and deleted from the tile's menu or by right-click. Songs in a playlist or the queue can be dragged into a new order the same way. Selecting songs with Ctrl+click adds "Delete from playlist". Rows slide, lift and fade smoothly (off with Reduce motion).
- Windows now shows "Roy Music" (not "riff-desktop") in the volume mixer and Task Manager: the installed program is "Roy Music.exe". Installing over an older version removes the old file names.
- The remaining Riff wording is gone, and so is importing the original Riff account and settings.

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
