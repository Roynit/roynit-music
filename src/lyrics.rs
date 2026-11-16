use crate::store;
use anyhow::{Result, ensure};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Word {
    pub time: f64,
    pub text: String,
    #[serde(default)]
    pub end: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Line {
    pub time: f64,
    pub text: String,
    pub words: Vec<Word>,
}

pub fn parse(text: &str) -> Vec<Line> {
    let time = Regex::new(r"\[(\d+):(\d{2})(?:[.:](\d{1,3}))?\]").unwrap();
    let word = Regex::new(r"<(\d+):(\d{2})(?:[.:](\d{1,3}))?>").unwrap();
    let offset = Regex::new(r"(?i)\[offset:([+-]?\d+)\]")
        .unwrap()
        .captures(text)
        .and_then(|c| c[1].parse::<f64>().ok())
        .unwrap_or(0.)
        / 1000.;
    let seconds = |c: &regex::Captures<'_>| -> f64 {
        c[1].parse::<f64>().unwrap_or(0.) * 60.
            + c[2].parse::<f64>().unwrap_or(0.)
            + c.get(3)
                .map(|m| m.as_str().parse::<f64>().unwrap_or(0.) / 10_f64.powi(m.len() as i32))
                .unwrap_or(0.)
            + offset
    };
    let mut lines = Vec::new();
    for row in text.lines() {
        let stamps: Vec<_> = time.captures_iter(row).collect();
        if stamps.is_empty() {
            continue;
        }
        let body = time.replace_all(row, "");
        let captures: Vec<_> = word.captures_iter(&body).collect();
        let words = captures
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let end = captures
                    .get(i + 1)
                    .map(|c| c.get(0).unwrap().start())
                    .unwrap_or(body.len());
                Word {
                    time: seconds(c).max(0.),
                    text: body[c.get(0).unwrap().end()..end].to_string(),
                    end: captures.get(i + 1).map(|c| seconds(c).max(0.)),
                }
            })
            .collect::<Vec<_>>();
        let plain = word.replace_all(&body, "").trim().to_string();
        for stamp in stamps {
            lines.push(Line {
                time: seconds(&stamp).max(0.),
                text: plain.clone(),
                words: words.clone(),
            });
        }
    }
    lines.sort_by(|a, b| a.time.total_cmp(&b.time));
    lines
}

pub fn parse_yrc(text: &str) -> Vec<Line> {
    let row = Regex::new(r"^\[(\d+),(\d+)\](.*)$").unwrap();
    let stamp = Regex::new(r"\((\d+),(\d+)(?:,\d+)?\)([^\(]*)").unwrap();
    let mut lines = Vec::new();
    for raw in text.lines() {
        let Some(c) = row.captures(raw.trim()) else {
            continue;
        };
        let words: Vec<_> = stamp
            .captures_iter(&c[3])
            .map(|w| {
                let time = w[1].parse::<f64>().unwrap_or(0.) / 1000.;
                Word {
                    time,
                    end: Some(time + w[2].parse::<f64>().unwrap_or(0.) / 1000.),
                    text: w[3].into(),
                }
            })
            .collect();
        if words.is_empty() {
            continue;
        }
        lines.push(Line {
            time: c[1].parse::<f64>().unwrap_or(0.) / 1000.,
            text: words.iter().map(|w| w.text.as_str()).collect(),
            words,
        });
    }
    lines.sort_by(|a, b| a.time.total_cmp(&b.time));
    lines
}

pub fn words_online(
    client: &reqwest::blocking::Client,
    title: &str,
    artist: &str,
    duration: f64,
) -> Result<Value> {
    let search: Value = client
        .get("https://music.163.com/api/search/get/web")
        .header("Referer", "https://music.163.com")
        .query(&[
            ("s", format!("{title} {artist}")),
            ("type", "1".into()),
            ("limit", "10".into()),
        ])
        .send()?
        .error_for_status()?
        .json()?;
    let normalize = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let title_norm = normalize(title);
    let artist_norm = normalize(artist);
    let song = search["result"]["songs"]
        .as_array()
        .and_then(|songs| {
            songs.iter().find(|s| {
                let name = s["name"].as_str().unwrap_or("");
                let artists = s.get("artists").or_else(|| s.get("ar"));
                let names: String = artists
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|a| a["name"].as_str())
                    .collect();
                let length = s["duration"].as_f64().or(s["dt"].as_f64()).unwrap_or(0.) / 1000.;
                normalize(name) == title_norm
                    && (artist_norm.is_empty() || normalize(&names).contains(&artist_norm))
                    && (duration <= 0. || (length - duration).abs() <= 3.)
            })
        })
        .ok_or_else(|| anyhow::anyhow!("No matching word-synced lyrics"))?;
    let id = song["id"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("Missing song ID"))?;
    let value: Value = client
        .get("https://music.163.com/api/song/lyric")
        .header("Referer", "https://music.163.com")
        .query(&[
            ("id", id.to_string()),
            ("lv", "1".into()),
            ("kv", "1".into()),
            ("tv", "-1".into()),
            ("yv", "1".into()),
        ])
        .send()?
        .error_for_status()?
        .json()?;
    let text = value["yrc"]["lyric"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Word timing is unavailable for this song"))?;
    let lines = parse_yrc(text);
    ensure!(!lines.is_empty(), "Word timing is empty");
    Ok(json!({"source":"netease","lines":lines}))
}
pub fn active(lines: &[Line], position: f64, offset: f64) -> Option<usize> {
    let count = lines.partition_point(|line| line.time <= position + offset);
    count.checked_sub(1)
}
pub fn cache_path(root: &Path, title: &str, artist: &str) -> std::path::PathBuf {
    root.join("lyrics").join(format!(
        "{}.json",
        store::key(&format!("{title}\0{artist}"))
    ))
}
pub fn save(root: &Path, title: &str, artist: &str, text: &str) -> Result<Value> {
    ensure!(text.len() <= 2 * 1024 * 1024, "Lyrics exceed 2 MB");
    let value = json!({"source":"custom","text":text,"lines":parse(text)});
    store::write_json(&cache_path(root, title, artist), &value)?;
    Ok(value)
}
pub fn fetch(
    client: &reqwest::blocking::Client,
    root: &Path,
    title: &str,
    artist: &str,
    duration: f64,
    online: bool,
) -> Result<Value> {
    fetch_source(client, root, title, artist, duration, online, "auto")
}
pub fn fetch_source(
    client: &reqwest::blocking::Client,
    root: &Path,
    title: &str,
    artist: &str,
    duration: f64,
    online: bool,
    source: &str,
) -> Result<Value> {
    let path = cache_path(root, title, artist);
    if path.is_file() {
        let cached: Value = serde_json::from_reader(std::fs::File::open(&path)?)?;
        if source == "auto" || !online || cached["source"] == source || cached["source"] == "custom"
        {
            return Ok(cached);
        }
    }
    ensure!(online, "No cached lyrics; online lookup disabled");
    let sources: Vec<_> = if source == "auto" {
        vec!["lrclib", "netease", "kugou", "musixmatch", "genius"]
    } else {
        vec![source]
    };
    for source in sources {
        crate::jobs::check_cancelled()?;
        if let Ok(value) = crate::lyric_providers::fetch(client, source, title, artist, duration) {
            crate::jobs::check_cancelled()?;
            store::write_json(&path, &value)?;
            return Ok(value);
        }
    }
    anyhow::bail!("No matching lyrics available from the requested providers")
}
