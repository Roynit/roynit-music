# Roy Music for Android

A small native app: Kotlin views and Media3 for the screen and the sound, the Rust core (`../mobile`) for
searching YouTube Music and finding a readable address for a song.

## What keeps the music playing

`PlaybackService` is a Media3 `MediaSessionService` running as a foreground service of type `mediaPlayback`,
with a media notification. The player lives in the service, not in the screen, so:

- the sound continues when the app goes to the background or the screen turns off;
- swiping the app out of the recents list does not stop it (`onTaskRemoved` only stops an idle service);
- the next song starts by itself, and a song that cannot be opened is skipped.

Checked on an Android 16 emulator with 2 GB of RAM: playing, then Home, then removing the task, then
screen off. The state stayed `PLAYING` and the position kept advancing in all of them.

## Memory and size

- Audio-only ExoPlayer (no video renderers), a 6 MB buffer target, no image library, plain views.
- Process memory while playing (Android 16 emulator, debug build): about 59 MB PSS with the screen closed,
  about 73 MB with the screen open.
- Release APK, both ABIs: about 8 MB. The Rust library is 3.4 MB (arm64) / 3.8 MB (x86_64).

## Build

One time: Android SDK with platform 35 and NDK 27, JDK 21, `rustup target add aarch64-linux-android
x86_64-linux-android`, `cargo install cargo-ndk`.

```powershell
# the Rust core (needs API 26 because it links AAudio)
cd mobile
$env:ANDROID_NDK_HOME = "$env:LOCALAPPDATA\Android\Sdk\ndk\27.2.12479018"
cargo ndk -P 26 -t arm64-v8a -t x86_64 -o ../android/app/src/main/jniLibs build --release

# the app
cd ../android
./gradlew.bat assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

`local.properties` needs `sdk.dir=C:/Users/<you>/AppData/Local/Android/Sdk`.

## Layout

- `Core.kt`: the JNI calls into the Rust core (`search`, `home`, `radio`, `resolve`, `lyrics`).
- `PlaybackService.kt`: the player, the queue (saved to `queue.json`), radio autoplay and the foreground service.
- `MainActivity.kt`, `Pages.kt`, `FriendsPage.kt`, `NowPlayingView.kt`: Home, Search, Library, Friends and the full player.
- `Account.kt`: sign-in to the Roy account (roynit.dangz.org), friends and "listening now".
- `Library.kt`: favourites, history and playlists in `library.json`.
- `../mobile/src/lib.rs`: the JNI side. Every call takes and returns JSON and never throws.

## Releasing

The signing key is yours and never goes in the repository. Make it once:

```
keytool -genkeypair -v -keystore roy-release.jks -alias roy -keyalg RSA -keysize 2048 -validity 10000
```

Keep `roy-release.jks` safe (losing it means users cannot update). Then build one APK per CPU:

```
./gradlew.bat assembleRelease -Proy.keystore=C:/path/roy-release.jks -Proy.storePass=... -Proy.keyAlias=roy -Proy.keyPass=...
```

The phones need `app-arm64-v8a-release.apk`. Publish its SHA-256 next to the download.

## Not done yet

Chat, avatars and Orbs, Google sign-in, an in-app updater, and iOS.
