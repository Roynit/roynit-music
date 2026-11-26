//! Roy Music for Android: the Rust core behind a tiny JNI surface. Kotlin owns the screen, the queue and
//! the audio (Media3 in a foreground service); this library only knows how to search YouTube Music and how
//! to turn a video into an address the player can read.
//!
//! Every call takes and returns JSON text, blocks until it is done (call it from a worker thread) and never
//! throws: a failure comes back as `{"error": "..."}`.
use anyhow::{Context, Result};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::jstring,
};
use riff_core::{model::Track, provider::Providers, youtube::Music};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::OnceLock,
    time::Duration,
};

fn client() -> Result<&'static reqwest::blocking::Client> {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    let built = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .pool_max_idle_per_host(1)
        .build()
        .context("Could not start the network client")?;
    Ok(CLIENT.get_or_init(|| built))
}

/// Songs matching a search, as `{"tracks":[{id,title,artist,album,duration,source,platform,cover}]}`.
fn search(query: &str) -> Result<Value> {
    anyhow::ensure!(!query.trim().is_empty(), "Type something to search for");
    Music { client: client()?, cookies: None }.search_filtered(query, "songs")
}

/// YouTube Music's home page: `{"tracks":[...]}` (the first screen of songs).
fn home() -> Result<Value> {
    Music { client: client()?, cookies: None }.home()
}

/// Songs that follow on from one video, the way YouTube Music's radio does: `{"tracks":[...]}`.
fn radio(video_id: &str, limit: usize) -> Result<Value> {
    Music { client: client()?, cookies: None }.radio(video_id, limit, None)
}

/// A readable address for a YouTube video: `{"url","duration","format","headers"}`.
fn resolve(video_id: &str) -> Result<Value> {
    riff_core::youtube::validate_video(video_id)?;
    let track = Track {
        id: video_id.to_owned(),
        title: String::new(),
        artist: String::new(),
        album: String::new(),
        duration: 0.,
        source: format!("https://music.youtube.com/watch?v={video_id}"),
        platform: "ytmusic".into(),
        cover: None,
        local_path: None,
        cache_path: None,
    };
    Providers { cookies: None, proxy: None, browser: None }
        .resolve(&track)
        .map_err(|e| anyhow::anyhow!("{e:#} [{}]", riff_core::youtube_stream::last_notes()))
}

/// Mixes made from what this person likes and plays, given as `{"tracks":{..},"favorites":[..],"history":[..]}`
/// (the same shape as the cloud library): `{"picks":[..],"mixes":[{name,tracks}..]}`.
fn mixes(library: &str) -> Result<Value> {
    let library: riff_core::model::Library = serde_json::from_str(library).context("Could not read the library")?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
    riff_core::mixes::for_library(&Music { client: client()?, cookies: None }, &library, now)
}

/// YouTube Music's charts page.
fn charts() -> Result<Value> {
    Music { client: client()?, cookies: None }.charts()
}

/// Songs, albums, artists or playlists matching a search: `{"tracks":[..],"items":[{id,kind,title,subtitle,cover}]}`.
fn search_kind(query: &str, kind: &str) -> Result<Value> {
    anyhow::ensure!(!query.trim().is_empty(), "Type something to search for");
    Music { client: client()?, cookies: None }.search_filtered(query, kind)
}

/// An artist's or album's page: its songs (`tracks`) and the albums and artists that go with it (`items`).
fn browse(id: &str, params: &str) -> Result<Value> {
    Music { client: client()?, cookies: None }.browse(id, Some(params).filter(|p| !p.is_empty()))
}

/// A podcast show: `{"title":..,"tracks":[episodes]}`.
fn podcast(id: &str) -> Result<Value> {
    Music { client: client()?, cookies: None }.podcast(id)
}

/// Search one source: "ytmusic", "youtube", "spotify" or "soundcloud". `{"tracks":[..]}`.
fn search_source(query: &str, source: &str) -> Result<Value> {
    let client = client()?;
    match source {
        "youtube" => riff_core::sources::youtube_search(client, query),
        "spotify" => riff_core::sources::spotify_search(client, query, 12),
        "soundcloud" => riff_core::sources::soundcloud_search(client, query, 12),
        _ => Music { client, cookies: None }.search_filtered(query, "songs"),
    }
}

/// An address for a song from any source. `id` is a YouTube video id, "sc:<number>" for SoundCloud, or "sp:<id>" for
/// a Spotify song, which is played from the same song on YouTube Music (found from its title, artist and length).
fn resolve_any(id: &str, title: &str, artist: &str, seconds: f64) -> Result<Value> {
    if let Some(number) = id.strip_prefix("sc:") {
        return riff_core::sources::soundcloud_resolve(client()?, number);
    }
    if id.starts_with("sp:") {
        let video = riff_core::sources::match_video(&Music { client: client()?, cookies: None }, title, artist, seconds)?;
        let mut found = resolve(&video)?;
        found["video"] = json!(video);
        return Ok(found);
    }
    resolve(id)
}

/// Word-by-word timings from a provider, merged into the lines the app already has (the same merge the PC app does).
fn lyric_words(title: &str, artist: &str, seconds: f64, lines: &str) -> Result<Value> {
    let mut ours: Vec<riff_core::lyrics::Line> = serde_json::from_str(lines)?;
    // Real word timings when a provider has them; otherwise each line's words are spread over the time to the next line.
    if let Ok(timed) = riff_core::lyrics::words_online(client()?, title, artist, seconds)
        && let Ok(timed) = serde_json::from_value::<Vec<riff_core::lyrics::Line>>(timed["lines"].clone())
    {
        riff_core::lyrics::merge_word_timings(&mut ours, &timed);
    }
    let snapshot = ours.clone();
    for (i, line) in ours.iter_mut().enumerate() {
        if line.words.is_empty() {
            line.words = riff_core::lyrics::estimate_word_timings(&snapshot[i], snapshot.get(i + 1), seconds);
        }
    }
    Ok(json!({"lines": ours}))
}

/// Lyrics for a song, from the first provider that has them: `{"lines":[{time,text,..}],"text"?,"instrumental"?}`.
/// `dir` is where found lyrics are kept so the next play needs no network.
fn lyrics(title: &str, artist: &str, seconds: f64, dir: &str) -> Result<Value> {
    riff_core::lyrics::fetch(client()?, std::path::Path::new(dir), title, artist, seconds, true)
}

fn find_header(value: &Value) -> Option<&Value> {
    match value {
        Value::Object(map) => ["musicResponsiveHeaderRenderer", "musicDetailHeaderRenderer", "musicEditablePlaylistDetailHeaderRenderer"]
            .iter()
            .find_map(|key| map.get(*key))
            .or_else(|| map.values().find_map(find_header)),
        Value::Array(items) => items.iter().find_map(find_header),
        _ => None,
    }
}

/// A playlist from a link: `{"name":..,"tracks":[..]}`. YouTube and YouTube Music playlists are read directly, Spotify
/// songs are kept as Spotify songs (found on YouTube Music when played), and other catalogs are matched to YouTube
/// Music now, a few at a time.
fn import_playlist(link: &str) -> Result<Value> {
    let url = url::Url::parse(link.trim()).context("That is not a link")?;
    let host = url.host_str().unwrap_or("").to_lowercase();
    let client = client()?;
    if host.contains("youtube.com") || host == "youtu.be" {
        let list = url.query_pairs().find(|(k, _)| k == "list").map(|(_, v)| v.into_owned()).context("This YouTube link has no playlist in it")?;
        let music = Music { client, cookies: None };
        let id = format!("VL{list}");
        let page = music.browse(&id, None)?;
        let name = music
            .browse_raw(&id)
            .ok()
            .and_then(|raw| find_header(&raw).and_then(|h| h["title"]["runs"][0]["text"].as_str().map(str::to_owned)))
            .unwrap_or_else(|| "Imported playlist".into());
        return Ok(json!({"name": name, "tracks": page["tracks"]}));
    }
    anyhow::ensure!(!host.contains("soundcloud.com"), "SoundCloud playlists are not supported on the phone yet");
    let metadata = riff_core::catalog::metadata_with_proxy(client, link.trim(), None, None)?;
    let plan = riff_core::catalog::import_plan(&metadata, link.trim(), true)?;
    let name = plan["name"].as_str().unwrap_or("Imported playlist").to_owned();
    let tracks = plan["tracks"].as_array().cloned().unwrap_or_default();
    if plan["provider"] == "spotify" {
        return Ok(json!({"name": name, "tracks": tracks}));
    }
    // Other catalogs name songs only: find each on YouTube Music, a handful in parallel (the first 150).
    let wanted: Vec<Value> = tracks.into_iter().take(150).collect();
    let found = std::sync::Mutex::new(Vec::<(usize, Value)>::new());
    std::thread::scope(|scope| {
        for lane in 0..5 {
            let (wanted, found) = (&wanted, &found);
            scope.spawn(move || {
                let music = Music { client, cookies: None };
                for (index, t) in wanted.iter().enumerate().filter(|(i, _)| i % 5 == lane) {
                    let (title, artist) = (t["title"].as_str().unwrap_or(""), t["artist"].as_str().unwrap_or(""));
                    if let Ok(video) = riff_core::sources::match_video(&music, title, artist, t["duration"].as_f64().unwrap_or(0.)) {
                        let song = json!({"id": video, "title": title, "artist": artist, "album": t["album"], "duration": t["duration"], "cover": t["cover"],
                            "platform": "ytmusic", "source": format!("https://music.youtube.com/watch?v={video}")});
                        found.lock().unwrap().push((index, song));
                    }
                }
            });
        }
    });
    let mut found = found.into_inner().unwrap();
    found.sort_by_key(|(i, _)| *i);
    Ok(json!({"name": name, "tracks": found.into_iter().map(|(_, s)| s).collect::<Vec<_>>(), "asked": wanted.len()}))
}

fn answer(work: impl FnOnce() -> Result<Value>) -> String {
    match catch_unwind(AssertUnwindSafe(work)) {
        Ok(Ok(value)) => value.to_string(),
        Ok(Err(error)) => json!({"error": format!("{error:#}")}).to_string(),
        Err(_) => json!({"error": "Something went wrong inside the core"}).to_string(),
    }
}

fn text(env: &mut JNIEnv, value: JString) -> String {
    env.get_string(&value).map(Into::into).unwrap_or_default()
}

fn reply(env: &mut JNIEnv, value: String) -> jstring {
    env.new_string(value).map_or(std::ptr::null_mut(), |s| s.into_raw())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_roynit_music_Core_search(mut env: JNIEnv, _: JClass, query: JString) -> jstring {
    let query = text(&mut env, query);
    reply(&mut env, answer(|| search(&query)))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_roynit_music_Core_resolve(mut env: JNIEnv, _: JClass, video_id: JString) -> jstring {
    let id = text(&mut env, video_id);
    reply(&mut env, answer(|| resolve(&id)))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_roynit_music_Core_home(mut env: JNIEnv, _: JClass) -> jstring {
    reply(&mut env, answer(home))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_roynit_music_Core_radio(mut env: JNIEnv, _: JClass, video_id: JString, limit: jni::sys::jint) -> jstring {
    let id = text(&mut env, video_id);
    reply(&mut env, answer(|| radio(&id, limit.clamp(1, 50) as usize)))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_roynit_music_Core_lyricWords(
    mut env: JNIEnv,
    _: JClass,
    title: JString,
    artist: JString,
    seconds: jni::sys::jdouble,
    lines: JString,
) -> jstring {
    let (title, artist, lines) = (text(&mut env, title), text(&mut env, artist), text(&mut env, lines));
    reply(&mut env, answer(|| lyric_words(&title, &artist, seconds, &lines)))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_roynit_music_Core_importPlaylist(mut env: JNIEnv, _: JClass, link: JString) -> jstring {
    let link = text(&mut env, link);
    reply(&mut env, answer(|| import_playlist(&link)))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_roynit_music_Core_lyrics(
    mut env: JNIEnv,
    _: JClass,
    title: JString,
    artist: JString,
    seconds: jni::sys::jdouble,
    dir: JString,
) -> jstring {
    let (title, artist, dir) = (text(&mut env, title), text(&mut env, artist), text(&mut env, dir));
    reply(&mut env, answer(|| lyrics(&title, &artist, seconds, &dir)))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_roynit_music_Core_mixes(mut env: JNIEnv, _: JClass, library: JString) -> jstring {
    let library = text(&mut env, library);
    reply(&mut env, answer(|| mixes(&library)))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_roynit_music_Core_charts(mut env: JNIEnv, _: JClass) -> jstring {
    reply(&mut env, answer(charts))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_roynit_music_Core_searchKind(mut env: JNIEnv, _: JClass, query: JString, kind: JString) -> jstring {
    let (query, kind) = (text(&mut env, query), text(&mut env, kind));
    reply(&mut env, answer(|| search_kind(&query, &kind)))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_roynit_music_Core_browse(mut env: JNIEnv, _: JClass, id: JString, params: JString) -> jstring {
    let (id, params) = (text(&mut env, id), text(&mut env, params));
    reply(&mut env, answer(|| browse(&id, &params)))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_roynit_music_Core_podcast(mut env: JNIEnv, _: JClass, id: JString) -> jstring {
    let id = text(&mut env, id);
    reply(&mut env, answer(|| podcast(&id)))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_roynit_music_Core_searchSource(mut env: JNIEnv, _: JClass, query: JString, source: JString) -> jstring {
    let (query, source) = (text(&mut env, query), text(&mut env, source));
    reply(&mut env, answer(|| search_source(&query, &source)))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_roynit_music_Core_resolveAny(mut env: JNIEnv, _: JClass, id: JString, title: JString, artist: JString, seconds: jni::sys::jdouble) -> jstring {
    let (id, title, artist) = (text(&mut env, id), text(&mut env, title), text(&mut env, artist));
    reply(&mut env, answer(|| resolve_any(&id, &title, &artist, seconds)))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_roynit_music_Core_version(mut env: JNIEnv, _: JClass) -> jstring {
    reply(&mut env, env!("CARGO_PKG_VERSION").to_owned())
}
