//! More places to find music than YouTube Music, for the phone app (the desktop app reaches them through yt-dlp):
//! plain YouTube, SoundCloud, and Spotify's catalogue, whose songs play from the matching YouTube Music song.
use crate::{model::Track, spotify, youtube::Music};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

const BROWSER: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

fn walk(value: &Value, key: &str, out: &mut Vec<Value>) {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(key) {
                out.push(found.clone());
            }
            map.values().for_each(|v| walk(v, key, out));
        }
        Value::Array(items) => items.iter().for_each(|v| walk(v, key, out)),
        _ => {}
    }
}

fn runs(value: &Value) -> String {
    value["simpleText"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value["runs"].as_array().into_iter().flatten().filter_map(|r| r["text"].as_str()).collect())
}

fn clock(text: &str) -> f64 {
    text.split(':').try_fold(0_f64, |total, part| part.trim().parse::<f64>().ok().map(|n| total * 60. + n)).unwrap_or(0.)
}

/// Plain YouTube (not the music site): videos matching the words, as tracks.
pub fn youtube_search(client: &reqwest::blocking::Client, query: &str) -> Result<Value> {
    ensure!(!query.trim().is_empty() && query.len() <= 1000, "Type something to search for");
    let response: Value = client
        .post("https://www.youtube.com/youtubei/v1/search?prettyPrint=false")
        .header("User-Agent", BROWSER)
        .json(&json!({"query":query,"context":{"client":{"clientName":"WEB","clientVersion":"2.20250101.00.00","hl":"en","gl":"US"}}}))
        .send()?
        .error_for_status()?
        .json()?;
    let mut rows = Vec::new();
    walk(&response, "videoRenderer", &mut rows);
    let tracks: Vec<Track> = rows
        .iter()
        .filter_map(|row| {
            let id = row["videoId"].as_str()?;
            let duration = clock(&runs(&row["lengthText"]));
            // Live streams and shorts have no length: they are not songs.
            (duration > 0.).then(|| Track {
                id: format!("youtube:{id}"),
                title: runs(&row["title"]),
                artist: runs(&row["ownerText"]),
                album: String::new(),
                duration,
                source: format!("https://www.youtube.com/watch?v={id}"),
                platform: "youtube".into(),
                cover: Some(format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg")),
                local_path: None,
                cache_path: None,
            })
        })
        .collect();
    Ok(json!({"tracks":tracks,"source":"youtube"}))
}

// ---- SoundCloud ----

fn soundcloud_id(client: &reqwest::blocking::Client) -> Result<String> {
    static KEPT: Mutex<Option<(Instant, String)>> = Mutex::new(None);
    if let Ok(kept) = KEPT.lock()
        && let Some((at, id)) = kept.as_ref()
        && at.elapsed() < Duration::from_secs(3600)
    {
        return Ok(id.clone());
    }
    let page = client.get("https://soundcloud.com/").header("User-Agent", BROWSER).send()?.text()?;
    let scripts: Vec<&str> = page.split("src=\"").skip(1).filter_map(|s| s.split('"').next()).filter(|s| s.starts_with("https://a-v2.sndcdn.com/assets/") && s.ends_with(".js")).collect();
    let pattern = regex::Regex::new(r#"client_id\s*[:=]\s*"([A-Za-z0-9]{32})""#)?;
    for script in scripts.iter().rev() {
        let Ok(body) = client.get(*script).header("User-Agent", BROWSER).send().and_then(|r| r.text()) else {
            continue;
        };
        if let Some(found) = pattern.captures(&body) {
            let id = found[1].to_owned();
            if let Ok(mut kept) = KEPT.lock() {
                *kept = Some((Instant::now(), id.clone()));
            }
            return Ok(id);
        }
    }
    bail!("Could not reach SoundCloud right now")
}

pub fn soundcloud_search(client: &reqwest::blocking::Client, query: &str, limit: usize) -> Result<Value> {
    ensure!(!query.trim().is_empty(), "Type something to search for");
    let id = soundcloud_id(client)?;
    let response: Value = client
        .get("https://api-v2.soundcloud.com/search/tracks")
        .query(&[("q", query), ("client_id", &id), ("limit", &(limit * 3).to_string())])
        .header("User-Agent", BROWSER)
        .send()?
        .error_for_status()?
        .json()?;
    let tracks: Vec<Track> = response["collection"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let number = row["id"].as_u64()?;
            // Previews of tracks that need a SoundCloud Go+ subscription are not the whole song.
            if row["policy"].as_str() == Some("SNIP") || row["streamable"] == false {
                return None;
            }
            // Only whole tracks: a transcoding marked "snipped" is the 30 second preview.
            if !row["media"]["transcodings"].as_array().into_iter().flatten().any(|t| t["snipped"] != true && t["format"]["protocol"] == "hls") {
                return None;
            }
            let art = row["artwork_url"].as_str().or_else(|| row["user"]["avatar_url"].as_str()).map(|u| u.replace("-large", "-t300x300"));
            Some(Track {
                id: format!("soundcloud:{number}"),
                title: row["title"].as_str()?.to_owned(),
                artist: row["user"]["username"].as_str().unwrap_or_default().to_owned(),
                album: String::new(),
                duration: row["duration"].as_f64().unwrap_or(0.) / 1000.,
                source: row["permalink_url"].as_str()?.to_owned(),
                platform: "soundcloud".into(),
                cover: art,
                local_path: None,
                cache_path: None,
            })
        })
        .take(limit)
        .collect();
    Ok(json!({"tracks":tracks,"source":"soundcloud"}))
}

/// The address of a SoundCloud track's segmented (HLS) stream, which the phone's player reads.
pub fn soundcloud_resolve(client: &reqwest::blocking::Client, number: &str) -> Result<Value> {
    ensure!(!number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()), "Invalid SoundCloud track");
    let id = soundcloud_id(client)?;
    let track: Value = client
        .get(format!("https://api-v2.soundcloud.com/tracks/{number}"))
        .query(&[("client_id", id.as_str())])
        .header("User-Agent", BROWSER)
        .send()?
        .error_for_status()?
        .json()?;
    let authorization = track["track_authorization"].as_str().unwrap_or_default();
    let transcoding = track["media"]["transcodings"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|t| t["snipped"] != true && t["format"]["protocol"] == "hls" && t["format"]["mime_type"].as_str().is_some_and(|m| m.starts_with("audio/mpeg") || m.starts_with("audio/mp4")))
        .context("This SoundCloud track has no audio")?;
    let link: Value = client
        .get(transcoding["url"].as_str().context("No audio address")?)
        .query(&[("client_id", id.as_str()), ("track_authorization", authorization)])
        .header("User-Agent", BROWSER)
        .send()?
        .error_for_status()?
        .json()?;
    let url = link["url"].as_str().context("SoundCloud did not give an audio address")?;
    Ok(json!({"url":url,"duration":track["duration"].as_f64().unwrap_or(0.) / 1000.,"format":"hls","resolver":"soundcloud"}))
}

// ---- Spotify: the catalogue, played from the matching YouTube Music song ----

pub fn spotify_search(client: &reqwest::blocking::Client, query: &str, limit: usize) -> Result<Value> {
    spotify::search_auto(client, None, query, limit)
}

/// The YouTube video that is the same song as a Spotify (or any other catalogue) track.
pub fn match_video(music: &Music<'_>, title: &str, artist: &str, seconds: f64) -> Result<String> {
    let wanted = Track {
        id: String::new(),
        title: title.to_owned(),
        artist: artist.to_owned(),
        album: String::new(),
        duration: seconds,
        source: String::new(),
        platform: "catalog".into(),
        cover: None,
        local_path: None,
        cache_path: None,
    };
    for query in spotify::match_queries(&wanted) {
        let found = music.search_filtered(&query, "songs")?;
        let tracks: Vec<Track> = serde_json::from_value(found["tracks"].clone()).unwrap_or_default();
        if let Some(best) = spotify::best_match(&wanted, &tracks)
            && let Some(id) = best.source.split("v=").nth(1)
        {
            return Ok(id.split('&').next().unwrap_or(id).to_owned());
        }
    }
    bail!("Could not find \"{title}\" on YouTube Music")
}
