use anyhow::{Context, Result, ensure};
use scraper::{Html, Selector};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

pub fn metadata(client: &reqwest::blocking::Client, input: &str) -> Result<Value> {
    metadata_with_proxy(client, input, None)
}
pub fn metadata_with_proxy(
    client: &reqwest::blocking::Client,
    input: &str,
    proxy: Option<&str>,
) -> Result<Value> {
    let url = url::Url::parse(input)?;
    ensure!(
        url.scheme() == "https" && url.username().is_empty() && url.password().is_none(),
        "Import needs a public HTTPS URL without credentials"
    );
    let host = url.host_str().context("Missing host")?;
    if ["music.yandex.ru", "music.yandex.com", "music.yandex.kz"].contains(&host) {
        return yandex(client, &url);
    }
    if host == "tiktok.com" || host.ends_with(".tiktok.com") {
        let mut args = vec![
            "--no-warnings".to_owned(),
            "--skip-download".into(),
            "--dump-single-json".into(),
        ];
        if let Some(proxy) = proxy {
            args.extend(["--proxy".into(), proxy.into()]);
        }
        args.extend(["--".into(), input.into()]);
        let v = crate::tools::json("yt-dlp", args)?;
        let title = v["track"]
            .as_str()
            .or(v["title"].as_str())
            .context("TikTok did not expose track metadata")?;
        let artist = v["artist"]
            .as_str()
            .or(v["uploader"].as_str())
            .unwrap_or("TikTok");
        return Ok(
            json!({"name":title,"tracks":[{"title":title,"artist":artist,"duration":v["duration"]}],"matching_required":true}),
        );
    }
    ensure!(
        [
            "music.apple.com",
            "vk.com",
            "www.vk.com",
            "m.vk.com",
            "vk.ru",
            "www.vk.ru"
        ]
        .contains(&host),
        "Unsupported import provider"
    );
    let response=client.get(input).header("User-Agent","Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131.0.0.0 Safari/537.36").send()?.error_for_status()?;
    let mut bytes = Vec::new();
    use std::io::Read;
    response
        .take(20 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 20 * 1024 * 1024,
        "Provider page exceeds 20 MB"
    );
    parse_public_html(&String::from_utf8(bytes)?)
}
pub fn parse_public_html(text: &str) -> Result<Value> {
    let doc = Html::parse_document(text);
    let mut tracks = Vec::new();
    let mut seen = HashSet::new();
    let mut total = 0usize;
    let selector =
        Selector::parse("script[type='application/json'],script[type='application/ld+json']")
            .unwrap();
    for script in doc.select(&selector) {
        if let Ok(value) = serde_json::from_str::<Value>(&script.inner_html()) {
            collect_metadata(&value, &mut tracks, &mut seen, &mut total);
        }
    }
    for element in doc.select(&Selector::parse("[data-audio]").unwrap()) {
        if let Some(raw) = element.value().attr("data-audio")
            && let Ok(v) = serde_json::from_str::<Value>(raw)
            && let Some(row) = v.as_array()
            && row.len() >= 6
        {
            let clean = |s: &str| {
                Html::parse_fragment(s)
                    .root_element()
                    .text()
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let title = clean(row[3].as_str().unwrap_or(""));
            let artist = clean(row[4].as_str().unwrap_or(""));
            let key = format!("{}\0{}", title.to_lowercase(), artist.to_lowercase());
            if !title.is_empty() && !artist.is_empty() && seen.insert(key) {
                tracks.push(json!({"title":title,"artist":artist,"duration":row[5]}));
            }
        }
    }
    ensure!(
        !tracks.is_empty(),
        "Provider exposed no public tracks. The playlist may require sign-in, be private, or use an unsupported page format"
    );
    let name = doc
        .select(&Selector::parse("meta[property='og:title']").unwrap())
        .next()
        .and_then(|e| e.value().attr("content"))
        .map(str::to_string)
        .or_else(|| {
            doc.select(&Selector::parse("title").unwrap())
                .next()
                .map(|e| e.text().collect())
        })
        .unwrap_or("Imported playlist".into());
    Ok(
        json!({"name":name,"partial":total>tracks.len(),"total":total.max(tracks.len()),"tracks":tracks}),
    )
}
fn collect_metadata(
    v: &Value,
    tracks: &mut Vec<Value>,
    seen: &mut HashSet<String>,
    total: &mut usize,
) {
    match v {
        Value::Array(a) => {
            for v in a {
                collect_metadata(v, tracks, seen, total);
            }
        }
        Value::Object(m) => {
            for key in ["trackCount", "numberOfTracks"] {
                if let Some(n) = v[key].as_u64() {
                    *total = (*total).max(n as usize);
                }
            }
            let attrs = v.get("attributes").unwrap_or(v);
            let kind = v["@type"]
                .as_str()
                .or(v["type"].as_str())
                .or(v["contentDescriptor"]["kind"].as_str())
                .unwrap_or("");
            let title = attrs["title"]
                .as_str()
                .or(attrs["name"].as_str())
                .unwrap_or("");
            let artist = attrs["artistName"]
                .as_str()
                .or(attrs["artist"].as_str())
                .or(attrs["byArtist"]["name"].as_str())
                .unwrap_or("");
            if !title.is_empty()
                && !artist.is_empty()
                && ([
                    "MusicRecording",
                    "songs",
                    "song",
                    "library-songs",
                    "music-video",
                ]
                .contains(&kind)
                    || attrs["durationInMillis"].is_number())
            {
                let duration = attrs["durationInMillis"]
                    .as_f64()
                    .or(attrs["duration_ms"].as_f64())
                    .map(|n| n / 1000.)
                    .or(attrs["duration"].as_f64())
                    .unwrap_or_else(|| iso_duration(attrs["duration"].as_str().unwrap_or("")));
                if seen.insert(format!(
                    "{}\0{}",
                    title.to_lowercase(),
                    artist.to_lowercase()
                )) {
                    tracks.push(json!({"title":title,"artist":artist,"duration":duration}));
                }
                return;
            }
            for v in m.values() {
                collect_metadata(v, tracks, seen, total);
            }
        }
        _ => {}
    }
}
pub fn iso_duration(s: &str) -> f64 {
    let regex = regex::Regex::new(r"^PT(?:(\d+)H)?(?:(\d+)M)?(?:(\d+(?:\.\d+)?)S)?$").unwrap();
    regex
        .captures(s)
        .map(|c| {
            [3600., 60., 1.]
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    c.get(i + 1)
                        .and_then(|v| v.as_str().parse::<f64>().ok())
                        .unwrap_or(0.)
                        * m
                })
                .sum()
        })
        .unwrap_or(0.)
}
fn yandex(client: &reqwest::blocking::Client, url: &url::Url) -> Result<Value> {
    let parts: Vec<_> = url
        .path_segments()
        .context("Invalid Yandex link")?
        .collect();
    ensure!(
        parts.iter().all(|p| p
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c))),
        "Invalid Yandex path"
    );
    let api = if parts.len() >= 4 && parts[0] == "users" && parts[2] == "playlists" {
        format!("users/{}/playlists/{}", parts[1], parts[3])
    } else if parts.len() >= 2 && parts[0] == "playlists" {
        format!("playlist/{}", parts[1])
    } else if parts.len() >= 2 && parts[0] == "album" {
        format!("albums/{}/with-tracks", parts[1])
    } else {
        anyhow::bail!("Yandex import expects a public playlist or album")
    };
    let v: Value = client
        .get(format!("https://api.music.yandex.net/{api}"))
        .header("User-Agent", "Yandex-Music-API")
        .send()?
        .error_for_status()?
        .json()?;
    let result = v.get("result").unwrap_or(&v);
    let mut rows = if let Some(rows) = result["tracks"].as_array() {
        rows.clone()
    } else {
        result["volumes"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|a| a.as_array().into_iter().flatten().cloned())
            .collect()
    };
    let missing: Vec<_> = rows
        .iter()
        .filter(|r| r["title"].is_null() && r["track"]["title"].is_null())
        .filter_map(|r| {
            r["id"]
                .as_str()
                .map(str::to_string)
                .or_else(|| r["id"].as_u64().map(|n| n.to_string()))
        })
        .collect();
    let mut found = HashMap::new();
    for chunk in missing.chunks(100) {
        crate::jobs::check_cancelled()?;
        let v: Value = client
            .post("https://api.music.yandex.net/tracks")
            .header("User-Agent", "Yandex-Music-API")
            .form(&[("track-ids", chunk.join(","))])
            .send()?
            .error_for_status()?
            .json()?;
        for t in v["result"].as_array().into_iter().flatten() {
            found.insert(t["id"].to_string().trim_matches('"').to_string(), t.clone());
        }
    }
    let mut tracks = Vec::new();
    let mut missing = 0usize;
    for row in &mut rows {
        let t = if row["track"].is_object() {
            row["track"].clone()
        } else if row["title"].is_string() {
            row.clone()
        } else {
            found
                .get(row["id"].to_string().trim_matches('"'))
                .cloned()
                .unwrap_or(Value::Null)
        };
        if let Some(title) = t["title"].as_str() {
            let artist = t["artists"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| a["name"].as_str())
                .collect::<Vec<_>>()
                .join(", ");
            tracks.push(json!({"title":title,"artist":artist,"duration":t["durationMs"].as_f64().unwrap_or(0.)/1000.}));
        } else {
            missing += 1;
        }
    }
    ensure!(
        !tracks.is_empty(),
        "Yandex returned no accessible track metadata"
    );
    Ok(
        json!({"name":result["title"].as_str().unwrap_or("Yandex Music"),"tracks":tracks,"skipped":missing,"partial":missing>0}),
    )
}
