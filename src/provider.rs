use crate::{model::Track, store, tools};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

type StreamCache = std::collections::HashMap<String, (std::time::Instant, Value)>;
static STREAMS: std::sync::OnceLock<std::sync::Mutex<StreamCache>> = std::sync::OnceLock::new();
static DIRECT_FAILURES: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>,
> = std::sync::OnceLock::new();

pub struct Providers {
    pub cookies: Option<PathBuf>,
    pub proxy: Option<String>,
    pub browser: Option<String>,
}
impl Providers {
    fn args(&self) -> Vec<String> {
        let mut a = vec![
            "--no-warnings".into(),
            "--no-progress".into(),
            "--socket-timeout".into(),
            "20".into(),
        ];
        if let Some(proxy) = &self.proxy {
            a.extend(["--proxy".into(), proxy.clone()]);
        }
        if let Some(path) = &self.cookies {
            a.extend(["--cookies".into(), path.to_string_lossy().into_owned()]);
        } else if let Some(browser) = &self.browser {
            a.extend(["--cookies-from-browser".into(), browser.clone()]);
        }
        a
    }
    pub fn search(&self, query: &str, platform: &str, limit: usize) -> Result<Vec<Track>> {
        ensure!(!query.trim().is_empty(), "Empty search");
        ensure!((1..=100).contains(&limit), "Limit must be 1..100");
        let prefix = match platform {
            "youtube" | "ytmusic" => "ytsearch",
            "soundcloud" => "scsearch",
            _ => anyhow::bail!("Provider must be youtube, ytmusic or soundcloud"),
        };
        let mut a = self.args();
        a.extend([
            "--flat-playlist".into(),
            "--dump-single-json".into(),
            "--".into(),
            format!("{prefix}{limit}:{query}"),
        ]);
        let value = tools::json("yt-dlp", &a)?;
        Ok(entries(&value).into_iter().filter_map(map_track).collect())
    }
    pub fn collection(&self, url: &str, limit: usize) -> Result<Value> {
        validate_url(url)?;
        ensure!(
            (1..=10000).contains(&limit),
            "Collection limit must be 1..10000"
        );
        let (url, limit) = collection_target(url, limit);
        let mut a = self.args();
        a.extend([
            "--flat-playlist".into(),
            "--dump-single-json".into(),
            "--playlist-end".into(),
            limit.to_string(),
            "--".into(),
            url,
        ]);
        let value = tools::json("yt-dlp", &a)?;
        let tracks: Vec<_> = entries(&value).into_iter().filter_map(map_track).collect();
        Ok(json!({"name":value["title"].as_str().unwrap_or("Imported playlist"),"tracks":tracks}))
    }
    fn cache_key(&self, track: &Track) -> String {
        format!(
            "match-v4|{}|{}|{}|{}|{}|{:?}|{:?}|{:?}",
            track.platform,
            track.source,
            track.title,
            track.artist,
            track.duration,
            self.cookies,
            self.proxy,
            self.browser
        )
    }
    pub fn invalidate(&self, track: &Track) {
        if let Ok(mut cache) = STREAMS.get_or_init(Default::default).lock() {
            cache.remove(&self.cache_key(track));
        }
    }
    pub(crate) fn recover_stream(&self, track: &Track) {
        if let Some(stream) = self.cached(track)
            && stream["resolver"] == "direct_youtube"
        {
            let source = stream["playback_source"].as_str().unwrap_or(&track.source);
            if let Ok(mut failures) = DIRECT_FAILURES.get_or_init(Default::default).lock() {
                failures.retain(|_, time| time.elapsed() < Duration::from_secs(60));
                if failures.len() >= 64 {
                    failures.clear();
                }
                failures.insert(source.into(), std::time::Instant::now());
            }
            if let Ok(mut cache) = STREAMS.get_or_init(Default::default).lock() {
                cache.retain(|_, (_, value)| value["url"] != stream["url"]);
            }
        }
        self.invalidate(track);
    }
    pub fn cached(&self, track: &Track) -> Option<Value> {
        STREAMS
            .get_or_init(Default::default)
            .lock()
            .ok()?
            .get(&self.cache_key(track))
            .filter(|(when, _)| when.elapsed() < Duration::from_secs(900))
            .map(|(_, v)| v.clone())
    }
    pub fn resolve(&self, track: &Track) -> Result<Value> {
        if let Some(v) = self.cached(track) {
            if !["spotify", "catalog"].contains(&track.platform.as_str())
                || validate_match_duration(track, &v).is_ok()
            {
                return Ok(v);
            }
            self.invalidate(track);
        }
        let gate = resolve_gate(&self.cache_key(track));
        let _guard = loop {
            crate::jobs::check_cancelled()?;
            match gate.try_lock() {
                Ok(guard) => break guard,
                Err(std::sync::TryLockError::Poisoned(error)) => break error.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => {
                    std::thread::sleep(Duration::from_millis(10))
                }
            }
        };
        // Prewarm and a click on the same recording must share the extraction result.
        if let Some(value) = self.cached(track)
            && (!["spotify", "catalog"].contains(&track.platform.as_str())
                || validate_match_duration(track, &value).is_ok())
        {
            return Ok(value);
        }
        let value = self.resolve_uncached(track)?;
        crate::jobs::check_cancelled()?;
        if let Ok(mut cache) = STREAMS.get_or_init(Default::default).lock() {
            cache.retain(|_, (when, _)| when.elapsed() < Duration::from_secs(900));
            if cache.len() >= 64 {
                cache.clear();
            }
            cache.insert(
                self.cache_key(track),
                (std::time::Instant::now(), value.clone()),
            );
        }
        Ok(value)
    }
    fn resolve_uncached(&self, track: &Track) -> Result<Value> {
        if ["spotify", "catalog"].contains(&track.platform.as_str()) {
            let client = crate::network::Config {
                proxy: self.proxy.clone(),
            }
            .client()?;
            let api = crate::youtube::Music {
                client: &client,
                cookies: self.cookies.as_ref(),
            };
            let queries = crate::spotify::match_queries(track);
            let tracks_of = |result: Result<Value>| -> Vec<Track> {
                result
                    .ok()
                    .and_then(|r| serde_json::from_value(r["tracks"].clone()).ok())
                    .unwrap_or_default()
            };
            let mut rejected = Vec::new();
            let mut tried = std::collections::HashSet::new();
            // First the quick, exact search; only when it finds nothing playable, widen it: the lead artist
            // alone, the bare title, and YouTube videos as well as songs (remix uploads are usually videos).
            for stage in 0..2 {
                let candidates: Vec<Track> = if stage == 0 {
                    tracks_of(api.search_filtered(&queries[0], "songs"))
                } else {
                    let jobs: Vec<(String, bool)> = queries
                        .iter()
                        .flat_map(|q| [(q.clone(), true), (q.clone(), false)])
                        .filter(|(q, songs)| !(*songs && *q == queries[0]))
                        .collect();
                    let token = crate::jobs::thread_cancel_token();
                    let mut merged: Vec<Track> = std::thread::scope(|scope| {
                        let workers: Vec<_> = jobs
                            .iter()
                            .map(|(q, songs)| {
                                let (api, token) = (&api, token.clone());
                                scope.spawn(move || {
                                    if let Some(token) = token {
                                        crate::jobs::set_thread_cancel_token(token);
                                    }
                                    if *songs {
                                        api.search_filtered(q, "songs")
                                    } else {
                                        api.search(q)
                                    }
                                })
                            })
                            .collect();
                        workers
                            .into_iter()
                            .flat_map(|w| tracks_of(w.join().unwrap_or_else(|_| Err(anyhow::anyhow!("search failed")))))
                            .collect()
                    });
                    let mut seen = std::collections::HashSet::new();
                    merged.retain(|t| seen.insert(t.id.clone()));
                    merged
                };
                crate::jobs::check_cancelled()?;
                let ranked = crate::spotify::ranked_matches(track, &candidates);
                for matched in ranked.into_iter().filter(|m| tried.insert(m.id.clone())).take(5) {
                    crate::jobs::check_cancelled()?;
                    match self.resolve(matched) {
                        Ok(mut value) => {
                            if let Err(error) = validate_match_duration(track, &value) {
                                rejected.push(error.to_string());
                                continue;
                            }
                            value["playback_source"] = json!(matched.source);
                            value["matched_title"] = json!(matched.title);
                            value["matched_artist"] = json!(matched.artist);
                            value["requested_duration"] = json!(track.duration);
                            value["verified_duration"] = json!(true);
                            return Ok(value);
                        }
                        Err(error) => rejected.push(error.to_string()),
                    }
                }
            }
            anyhow::bail!(
                "No verified recording matching '{}' ({:.0}s). Refusing to play a different version. {}",
                track.title,
                track.duration,
                rejected.join("; ")
            );
        }

        validate_url(&track.source)?;
        #[cfg(test)]
        EXTRACTIONS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        if matches!(track.platform.as_str(), "youtube" | "ytmusic")
            && self.proxy.is_none()
            && !DIRECT_FAILURES
                .get_or_init(Default::default)
                .lock()
                .ok()
                .is_some_and(|failures| {
                    failures
                        .get(&track.source)
                        .is_some_and(|time| time.elapsed() < Duration::from_secs(60))
                })
            && let Some(stream) = crate::youtube_stream::resolve(&track.source)
        {
            return Ok(stream);
        }
        crate::jobs::check_cancelled()?;
        // Prefer a single, directly readable audio stream for YouTube playback.
        // Keep the broader selector for videos without a progressive audio format.
        let format = if matches!(track.platform.as_str(), "youtube" | "ytmusic") {
            "ba[ext=m4a][protocol=https]/ba[ext=webm][protocol=https]/bestaudio/best"
        } else {
            "bestaudio/best"
        };
        let mut a = self.args();
        a.extend([
            "--no-playlist".into(),
            "-f".into(),
            format.into(),
            "--dump-single-json".into(),
            "--".into(),
            track.source.clone(),
        ]);
        let v = tools::json("yt-dlp", &a)?;
        let url = v["url"]
            .as_str()
            .context("Provider did not return a playable stream")?;
        Ok(
            json!({"url":url,"headers":v["http_headers"],"duration":v["duration"],"format":v["ext"]}),
        )
    }
    pub fn download(&self, track: &Track, dir: &Path, format: &str) -> Result<PathBuf> {
        if ["spotify", "catalog"].contains(&track.platform.as_str()) {
            let resolved = self.resolve(track)?;
            let mut matched = track.clone();
            matched.source = resolved["playback_source"]
                .as_str()
                .context("Missing matched audio source")?
                .to_owned();
            matched.platform = "youtube".into();
            return self.download(&matched, dir, format);
        }
        ensure!(
            ["mp3", "flac", "opus", "wav", "m4a"].contains(&format),
            "Unsupported audio format"
        );
        validate_url(&track.source)?;
        std::fs::create_dir_all(dir)?;
        let stem = store::key(&track.id);
        let target = dir.join(format!("{stem}.{format}"));
        if target.is_file() {
            return Ok(target);
        }
        let work = tempfile::tempdir_in(dir)?;
        let pattern = work.path().join(format!("{stem}.%(ext)s"));
        let mut a = self.args();
        a.extend([
            "--no-playlist".into(),
            "-f".into(),
            "bestaudio/best".into(),
            "-x".into(),
            "--audio-format".into(),
            format.into(),
            "--embed-metadata".into(),
            "--embed-thumbnail".into(),
            "-o".into(),
            pattern.to_string_lossy().into_owned(),
            "--".into(),
            track.source.clone(),
        ]);
        tools::run("yt-dlp", &a, Duration::from_secs(1800))?;
        let produced = work.path().join(format!("{stem}.{format}"));
        ensure!(
            produced.is_file(),
            "Download did not produce expected audio file"
        );
        std::fs::rename(produced, &target)?;
        Ok(target)
    }
}
/// What yt-dlp should read for a YouTube link to a playlist. A song opened from a playlist (`watch?v=…&list=PL…`)
/// becomes the playlist itself. A Mix or radio (`list=RD…`) never ends, so only its first 50 songs are taken.
pub fn collection_target(url: &str, limit: usize) -> (String, usize) {
    let Ok(parsed) = url::Url::parse(url) else {
        return (url.into(), limit);
    };
    let host = parsed.host_str().unwrap_or_default().to_lowercase();
    if !(host == "youtube.com" || host.ends_with(".youtube.com") || host == "youtu.be") {
        return (url.into(), limit);
    }
    let Some(list) = parsed.query_pairs().find(|(k, _)| k == "list").map(|(_, v)| v.into_owned()) else {
        return (url.into(), limit);
    };
    // A radio started from a playlist names that playlist after RDAMPL: import the playlist, not the radio.
    let list = list.strip_prefix("RDAMPL").map(str::to_owned).unwrap_or(list);
    // RDCLAK… lists are albums and editorial playlists; every other RD… list is a mix that runs to hundreds of songs.
    if list.starts_with("RD") && !list.starts_with("RDCLAK") {
        return (url.into(), limit.min(50));
    }
    let site = if host.starts_with("music.") { "https://music.youtube.com" } else { "https://www.youtube.com" };
    (format!("{site}/playlist?list={list}"), limit)
}

pub fn validate_url(input: &str) -> Result<()> {
    let url = url::Url::parse(input)?;
    ensure!(
        ["https", "http"].contains(&url.scheme()),
        "Only HTTP(S) provider URLs are supported"
    );
    let h = url.host_str().context("URL has no host")?;
    ensure!(
        [
            "youtube.com",
            "youtu.be",
            "soundcloud.com",
            "bandcamp.com",
            "vimeo.com"
        ]
        .iter()
        .any(|d| h == *d || h.ends_with(&format!(".{d}"))),
        "Unsupported provider URL host"
    );
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "Credentials in provider URLs are not allowed"
    );
    Ok(())
}
fn entries(value: &Value) -> Vec<&Value> {
    if let Some(e) = value["entries"].as_array() {
        e.iter().collect()
    } else {
        vec![value]
    }
}
pub fn map_track(v: &Value) -> Option<Track> {
    let id = v["id"].as_str()?;
    let extractor = v["extractor_key"]
        .as_str()
        .or(v["ie_key"].as_str())
        .or(v["extractor"].as_str())
        .unwrap_or("youtube")
        .to_lowercase();
    let platform = if extractor.contains("soundcloud")
        || v["webpage_url"]
            .as_str()
            .or(v["url"].as_str())
            .and_then(|s| url::Url::parse(s).ok())
            .and_then(|u| u.host_str().map(str::to_owned))
            .is_some_and(|h| h == "soundcloud.com" || h.ends_with(".soundcloud.com"))
    {
        "soundcloud"
    } else {
        "youtube"
    };
    let raw = v["webpage_url"]
        .as_str()
        .or(v["url"].as_str())
        .unwrap_or("");
    let source = if raw.starts_with("https://") || raw.starts_with("http://") {
        raw.into()
    } else {
        format!("https://www.youtube.com/watch?v={id}")
    };
    let cover = v["thumbnail"].as_str().map(str::to_string).or_else(|| {
        v["thumbnails"].as_array()?.last()?["url"]
            .as_str()
            .map(str::to_string)
    });
    Some(Track {
        id: format!("{platform}:{id}"),
        title: v["title"].as_str().unwrap_or("Untitled").into(),
        artist: v["artist"]
            .as_str()
            .or(v["uploader"].as_str())
            .or(v["channel"].as_str())
            .unwrap_or("")
            .into(),
        album: v["album"].as_str().unwrap_or("").into(),
        duration: v["duration"].as_f64().unwrap_or(0.),
        source,
        platform: platform.into(),
        cover,
        local_path: None,
        cache_path: None,
    })
}

/// Extraction duration is the final authority, never the unverified search snippet.
pub fn validate_match_duration(track: &Track, resolved: &Value) -> Result<()> {
    let actual = resolved["duration"]
        .as_f64()
        .context("Resolved recording has no duration; cannot verify it")?;
    ensure!(
        crate::spotify::duration_matches(track.duration, actual),
        "Duration mismatch: requested {:.0}s, resolved {:.0}s",
        track.duration,
        actual
    );
    Ok(())
}

fn resolve_gate(key: &str) -> std::sync::Arc<std::sync::Mutex<()>> {
    type Gates = std::collections::HashMap<String, std::sync::Weak<std::sync::Mutex<()>>>;
    static GATES: std::sync::OnceLock<std::sync::Mutex<Gates>> = std::sync::OnceLock::new();
    let mut gates = GATES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    gates.retain(|_, gate| gate.strong_count() > 0);
    if let Some(gate) = gates.get(key).and_then(std::sync::Weak::upgrade) {
        return gate;
    }
    let gate = std::sync::Arc::new(std::sync::Mutex::new(()));
    gates.insert(key.into(), std::sync::Arc::downgrade(&gate));
    gate
}

#[cfg(test)]
static EXTRACTIONS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[cfg(test)]
mod concurrent_tests {
    use super::*;
    #[test]
    #[ignore = "Live extractor concurrency test requires yt-dlp/network"]
    fn simultaneous_warmup_and_play_share_one_extraction() {
        let track:Track=serde_json::from_value(json!({"id":"youtube:jNQXAC9IVRw","title":"Me at the zoo","source":"https://www.youtube.com/watch?v=jNQXAC9IVRw","platform":"youtube"})).unwrap();
        let make = || Providers {
            cookies: None,
            proxy: None,
            browser: None,
        };
        make().invalidate(&track);
        let before = EXTRACTIONS.load(std::sync::atomic::Ordering::SeqCst);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let barrier = barrier.clone();
            let track = track.clone();
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                Providers {
                    cookies: None,
                    proxy: None,
                    browser: None,
                }
                .resolve(&track)
                .unwrap()
            }));
        }
        barrier.wait();
        let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
        assert_eq!(results[0], results[1]);
        assert_eq!(
            EXTRACTIONS.load(std::sync::atomic::Ordering::SeqCst) - before,
            1
        );
        make().invalidate(&track);
    }
}
