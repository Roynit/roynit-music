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

/// Lyrics for a song, from the first provider that has them: `{"lines":[{time,text,..}],"text"?,"instrumental"?}`.
/// `dir` is where found lyrics are kept so the next play needs no network.
fn lyrics(title: &str, artist: &str, seconds: f64, dir: &str) -> Result<Value> {
    riff_core::lyrics::fetch(client()?, std::path::Path::new(dir), title, artist, seconds, true)
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
