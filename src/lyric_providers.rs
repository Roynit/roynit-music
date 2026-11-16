use anyhow::{Context, Result, ensure};
use base64::Engine as _;
use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::time::Duration;
fn normalized(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
fn matched(title: &str, artist: &str, t: &str, a: &str) -> bool {
    let artist = normalized(artist);
    normalized(title) == normalized(t)
        && (artist.is_empty()
            || normalized(a).contains(&artist)
            || artist.contains(&normalized(a)) && !a.is_empty())
}
fn get(client: &Client, url: &str, query: &[(&str, String)]) -> Result<Value> {
    crate::jobs::check_cancelled()?;
    let v = client
        .get(url)
        .timeout(Duration::from_secs(10))
        .query(query)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .send()?
        .error_for_status()?
        .json()?;
    crate::jobs::check_cancelled()?;
    Ok(v)
}
fn result(source: &str, text: &str) -> Result<Value> {
    ensure!(!text.trim().is_empty(), "{source} returned empty lyrics");
    Ok(json!({"source":source,"text":text,"lines":crate::lyrics::parse(text)}))
}
pub fn fetch(
    client: &Client,
    source: &str,
    title: &str,
    artist: &str,
    duration: f64,
) -> Result<Value> {
    match source {
        "lrclib" => {
            let mut q = vec![("track_name", title.into()), ("artist_name", artist.into())];
            if duration.is_finite() && duration > 0. {
                q.push(("duration", duration.round().to_string()));
            }
            let v = get(client, "https://lrclib.net/api/get", &q)?;
            if v["instrumental"] == true {
                return Ok(json!({"source":"lrclib","instrumental":true,"text":"","lines":[]}));
            }
            result(
                source,
                v["syncedLyrics"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .or(v["plainLyrics"].as_str())
                    .unwrap_or(""),
            )
        }
        "netease" => {
            let search = get(
                client,
                "https://music.163.com/api/search/get/web",
                &[
                    ("s", format!("{title} {artist}")),
                    ("type", "1".into()),
                    ("limit", "10".into()),
                ],
            )?;
            let song = search["result"]["songs"]
                .as_array()
                .and_then(|songs| {
                    songs.iter().find(|song| {
                        let names = song
                            .get("artists")
                            .or(song.get("ar"))
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(|a| a["name"].as_str())
                            .collect::<Vec<_>>()
                            .join(" ");
                        let length = song["duration"]
                            .as_f64()
                            .or(song["dt"].as_f64())
                            .unwrap_or(0.)
                            / 1000.;
                        matched(title, artist, song["name"].as_str().unwrap_or(""), &names)
                            && (duration <= 0. || length <= 0. || (length - duration).abs() < 5.)
                    })
                })
                .context("No matching NetEase lyrics")?;
            let id = song["id"].as_u64().context("Missing NetEase song ID")?;
            let v = get(
                client,
                "https://music.163.com/api/song/lyric",
                &[
                    ("id", id.to_string()),
                    ("lv", "1".into()),
                    ("kv", "1".into()),
                    ("tv", "-1".into()),
                    ("yv", "1".into()),
                ],
            )?;
            if let Some(yrc) = v["yrc"]["lyric"].as_str() {
                let lines = crate::lyrics::parse_yrc(yrc);
                if !lines.is_empty() {
                    return Ok(
                        json!({"source":source,"text":v["lrc"]["lyric"].as_str().unwrap_or(""),"lines":lines}),
                    );
                }
            }
            result(source, v["lrc"]["lyric"].as_str().unwrap_or(""))
        }
        "kugou" => {
            let v = get(
                client,
                "https://lyrics.kugou.com/search",
                &[
                    ("ver", "1".into()),
                    ("man", "yes".into()),
                    ("client", "pc".into()),
                    ("keyword", format!("{artist} - {title}")),
                    ("duration", (duration * 1000.).round().to_string()),
                ],
            )?;
            let candidate = v["candidates"]
                .as_array()
                .and_then(|a| {
                    a.iter().find(|c| {
                        matched(
                            title,
                            artist,
                            c["songname"].as_str().unwrap_or(""),
                            c["singername"].as_str().unwrap_or(""),
                        )
                    })
                })
                .context("No matching Kugou lyrics")?;
            let id = candidate["id"]
                .as_str()
                .map(str::to_string)
                .or_else(|| candidate["id"].as_u64().map(|n| n.to_string()))
                .context("Missing Kugou song ID")?;
            let v = get(
                client,
                "https://lyrics.kugou.com/download",
                &[
                    ("ver", "1".into()),
                    ("client", "pc".into()),
                    ("id", id),
                    (
                        "accesskey",
                        candidate["accesskey"]
                            .as_str()
                            .context("Missing access key")?
                            .into(),
                    ),
                    ("fmt", "lrc".into()),
                    ("charset", "utf8".into()),
                ],
            )?;
            let bytes = base64::engine::general_purpose::STANDARD.decode(
                v["content"]
                    .as_str()
                    .context("Kugou did not return lyrics")?,
            )?;
            result(source, &String::from_utf8(bytes)?)
        }
        "musixmatch" => {
            let token = get(
                client,
                "https://apic-desktop.musixmatch.com/ws/1.1/token.get",
                &[("app_id", "web-desktop-app-v1.0".into())],
            )?;
            let token = token["message"]["body"]["user_token"]
                .as_str()
                .context("Musixmatch did not issue a token")?;
            let v = get(
                client,
                "https://apic-desktop.musixmatch.com/ws/1.1/macro.subtitles.get",
                &[
                    ("app_id", "web-desktop-app-v1.0".into()),
                    ("usertoken", token.into()),
                    ("q_track", title.into()),
                    ("q_artist", artist.into()),
                ],
            )?;
            let calls = &v["message"]["body"]["macro_calls"];
            let track = &calls["matcher.track.get"]["message"]["body"]["track"];
            ensure!(
                matched(
                    title,
                    artist,
                    track["track_name"].as_str().unwrap_or(""),
                    track["artist_name"].as_str().unwrap_or("")
                ),
                "Musixmatch returned a different song"
            );
            let synced=calls["track.subtitles.get"]["message"]["body"]["subtitle_list"][0]["subtitle"]["subtitle_body"].as_str().unwrap_or("");
            let plain = calls["track.lyrics.get"]["message"]["body"]["lyrics"]["lyrics_body"]
                .as_str()
                .unwrap_or("");
            result(source, if synced.is_empty() { plain } else { synced })
        }
        "genius" => {
            let v = get(
                client,
                "https://genius.com/api/search/multi",
                &[("q", format!("{title} {artist}")), ("per_page", "5".into())],
            )?;
            let song = v["response"]["sections"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|s| s["hits"].as_array().into_iter().flatten())
                .map(|h| &h["result"])
                .find(|s| {
                    matched(
                        title,
                        artist,
                        s["title"].as_str().unwrap_or(""),
                        s["primary_artist"]["name"].as_str().unwrap_or(""),
                    )
                })
                .context("No matching Genius lyrics")?;
            let url = url::Url::parse(song["url"].as_str().context("Missing Genius song URL")?)?;
            ensure!(
                url.scheme() == "https"
                    && url
                        .host_str()
                        .is_some_and(|h| h == "genius.com" || h == "www.genius.com"),
                "Unexpected Genius URL"
            );
            let html = client
                .get(url)
                .timeout(Duration::from_secs(10))
                .send()?
                .error_for_status()?
                .text()?;
            let doc = scraper::Html::parse_document(&html);
            let selector = scraper::Selector::parse("[data-lyrics-container='true']").unwrap();
            let text = doc
                .select(&selector)
                .map(|el| el.text().collect::<Vec<_>>().join("\n"))
                .collect::<Vec<_>>()
                .join("\n\n");
            result(source, &text)
        }
        _ => anyhow::bail!("Unknown lyric provider"),
    }
}
