//! Native client for YouTube Music's web endpoints. The provider supplies the order;
//! we do not substitute artist searches for recommendations or fabricate results.
use crate::model::Track;
use anyhow::{Context, Result, ensure};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use std::{
    collections::HashSet,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Music<'a> {
    pub client: &'a Client,
    pub cookies: Option<&'a PathBuf>,
}
fn bootstrap(client: &Client) -> &'static (String, Option<String>) {
    static BOOTSTRAP: std::sync::OnceLock<(String, Option<String>)> = std::sync::OnceLock::new();
    BOOTSTRAP.get_or_init(|| {
        let home = client
            .get("https://music.youtube.com/")
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(reqwest::blocking::Response::text)
            .unwrap_or_default();
        let config = |key: &str| -> Option<String> {
            regex::Regex::new(&format!(r#""{}"\s*:\s*"([^"]+)""#, regex::escape(key)))
                .ok()?
                .captures(&home)
                .map(|c| c[1].into())
        };
        (
            config("INNERTUBE_CLIENT_VERSION")
                .or_else(|| config("INNERTUBE_CONTEXT_CLIENT_VERSION"))
                .unwrap_or_else(|| format!("1.{}.01.00", chrono::Utc::now().format("%Y%m%d"))),
            config("VISITOR_DATA"),
        )
    })
}
impl Music<'_> {
    fn request(&self, endpoint: &str, mut body: Value) -> Result<Value> {
        crate::jobs::check_cancelled()?;
        let (version, visitor) = bootstrap(self.client);
        body["context"] = json!({"client":{"clientName":"WEB_REMIX","clientVersion":version,"hl":"en","gl":"US","utcOffsetMinutes":0},"user":{}});
        let mut request = self
            .client
            .post(format!(
                "https://music.youtube.com/youtubei/v1/{endpoint}?prettyPrint=false"
            ))
            .header("Origin", "https://music.youtube.com")
            .header("Referer", "https://music.youtube.com/")
            .header("X-Youtube-Client-Name", "67")
            .header("X-Youtube-Client-Version", version.as_str());
        request=request.header("User-Agent","Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36");
        if let Some(visitor) = visitor {
            request = request.header("X-Goog-Visitor-Id", visitor.as_str());
            body["context"]["client"]["visitorData"] = json!(visitor);
        }
        if let Some(path) = self.cookies {
            let raw = std::fs::read_to_string(path)
                .context("Cannot read configured YouTube cookie file")?;
            let cookies = parse_cookies(&raw);
            ensure!(
                !cookies.is_empty(),
                "No valid YouTube/Google cookies found in file"
            );
            let header = cookies
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("; ");
            request = request
                .header("Cookie", header)
                .header("X-Goog-AuthUser", "0");
            if let Some((_, sapisid)) = cookies
                .iter()
                .find(|(k, _)| k == "SAPISID" || k == "__Secure-3PAPISID")
            {
                let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
                let hash = format!(
                    "{:x}",
                    Sha1::digest(
                        format!("{timestamp} {sapisid} https://music.youtube.com").as_bytes()
                    )
                );
                request =
                    request.header("Authorization", format!("SAPISIDHASH {timestamp}_{hash}"));
            }
        }
        let response = request.json(&body).send()?.error_for_status()?;
        let value: Value = response.json()?;
        crate::jobs::check_cancelled()?;
        ensure!(
            value.get("error").is_none(),
            "YouTube Music rejected request: {}",
            value["error"]["message"]
                .as_str()
                .unwrap_or("provider error")
        );
        Ok(value)
    }
    pub fn radio(&self, video_id: &str, limit: usize, continuation: Option<&str>) -> Result<Value> {
        validate_video(video_id)?;
        ensure!((1..=200).contains(&limit), "Radio limit must be 1..200");
        let mut body = json!({"videoId":video_id,"playlistId":format!("RDAMVM{video_id}"),"enablePersistentPlaylistPanel":true,"isAudioOnly":true,"tunerSettingValue":"AUTOMIX_SETTING_NORMAL","params":"wAEB"});
        if let Some(token) = continuation {
            ensure!(token.len() < 20000, "Invalid continuation token");
            body["continuation"] = json!(token);
        }
        let value = self.request("next", body)?;
        let mut tracks = parse_tracks(&value);
        tracks.retain(|t| t.id != format!("youtube:{video_id}"));
        tracks.truncate(limit);
        ensure!(
            !tracks.is_empty(),
            "YouTube Music returned no radio tracks; try a music video or song, or configure cookies"
        );
        let token = find_continuation(&value);
        Ok(
            json!({"source":"youtube_music_radio","provider_order":true,"session_cookie_configured":self.cookies.is_some(),"authenticated_session":value.pointer("/responseContext/mainAppWebResponseContext/loggedOut").and_then(Value::as_bool).map(|out|!out),"personalization":"provider_controlled_not_guaranteed","seed":video_id,"tracks":tracks,"continuation":token}),
        )
    }
    pub fn search(&self, query: &str) -> Result<Value> {
        ensure!(
            !query.trim().is_empty() && query.len() <= 1000,
            "Invalid music search query"
        );
        let value = self.request("search", json!({"query":query}))?;
        Ok(
            json!({"source":"youtube_music","tracks":parse_tracks(&value),"items":parse_items(&value),"continuation":find_continuation(&value)}),
        )
    }
    pub fn search_filtered(&self, query: &str, kind: &str) -> Result<Value> {
        ensure!(
            !query.trim().is_empty() && query.len() <= 1000,
            "Invalid search query"
        );
        let code = match kind {
            "songs" => "II",
            "albums" => "IY",
            "artists" => "Ig",
            "playlists" => "Io",
            _ => anyhow::bail!("Filter must be songs, albums, artists or playlists"),
        };
        let value = self.request(
            "search",
            json!({"query":query,"params":format!("EgWKAQ{code}AWoMEA4QChADEAQQCRAF")}),
        )?;
        Ok(
            json!({"source":"youtube_music","filter":kind,"tracks":parse_tracks(&value),"items":parse_items(&value),"continuation":find_continuation(&value)}),
        )
    }
    pub fn browse(&self, id: &str, params: Option<&str>) -> Result<Value> {
        ensure!(
            id.len() <= 256
                && !id.is_empty()
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c)),
            "Invalid browse ID"
        );
        let mut body = json!({"browseId":id});
        if let Some(params) = params {
            body["params"] = json!(params);
        }
        let value = self.request("browse", body)?;
        Ok(
            json!({"source":"youtube_music","browse_id":id,"generated_mixes":if id=="FEmusic_home"{parse_generated_mixes(&value)}else{vec![]},"authenticated_session":value.pointer("/responseContext/mainAppWebResponseContext/loggedOut").and_then(Value::as_bool).map(|out|!out),"tracks":parse_tracks(&value),"items":parse_items(&value),"continuation":find_continuation(&value)}),
        )
    }
    pub fn home(&self) -> Result<Value> {
        self.browse("FEmusic_home", None)
    }
    pub(crate) fn generated_mix(
        &self,
        id: &str,
        video: Option<&str>,
        params: Option<&str>,
    ) -> Result<Vec<Track>> {
        ensure!(is_generated_mix_id(id), "Invalid generated playlist ID");
        let mut body = json!({"playlistId":id,"isAudioOnly":true});
        if let Some(video) = video {
            validate_video(video)?;
            body["videoId"] = json!(video);
        }
        if let Some(params) = params.filter(|p| p.len() <= 20000) {
            body["params"] = json!(params);
        }
        let response = self.request("next", body)?;
        let tracks: Vec<_> = parse_tracks(&response).into_iter().take(24).collect();
        ensure!(
            !tracks.is_empty(),
            "YouTube Music returned no tracks for this mix"
        );
        Ok(tracks)
    }
    pub fn charts(&self) -> Result<Value> {
        self.browse("FEmusic_charts", None)
    }
}
pub fn validate_video(id: &str) -> Result<()> {
    ensure!(
        id.len() == 11
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c)),
        "Expected an 11-character YouTube video ID"
    );
    Ok(())
}
fn parse_cookies(text: &str) -> Vec<(String, String)> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    text.lines()
        .filter_map(|row| {
            let row = row.strip_prefix("#HttpOnly_").unwrap_or(row);
            if row.starts_with('#') {
                return None;
            }
            let p: Vec<_> = row.split('\t').collect();
            if p.len() != 7 {
                return None;
            }
            let domain = p[0].trim_start_matches('.');
            if !["youtube.com", "google.com", "music.youtube.com"].contains(&domain) {
                return None;
            }
            let expiry = p[4].parse::<u64>().ok()?;
            if expiry > 0 && expiry <= now {
                return None;
            }
            if p[5].contains(['\r', '\n', ';']) || p[6].contains(['\r', '\n', ';']) {
                return None;
            }
            Some((p[5].into(), p[6].into()))
        })
        .collect()
}
pub fn text(value: &Value) -> String {
    value["simpleText"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| {
            value["runs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|r| r["text"].as_str())
                .collect()
        })
}
fn duration(s: &str) -> f64 {
    let parts: Vec<_> = s.trim().split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return 0.;
    }
    let values: Option<Vec<u32>> = parts
        .iter()
        .map(|p| {
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
                None
            } else {
                p.parse().ok()
            }
        })
        .collect();
    let Some(values) = values else {
        return 0.;
    };
    if values.iter().skip(1).any(|n| *n >= 60) {
        return 0.;
    }
    values.into_iter().fold(0_f64, |a, n| a * 60. + n as f64)
}
fn walk(value: &Value, key: &str, result: &mut Vec<Value>) {
    match value {
        Value::Object(map) => {
            if let Some(item) = map.get(key) {
                result.push(item.clone());
            }
            for v in map.values() {
                walk(v, key, result);
            }
        }
        Value::Array(a) => {
            for v in a {
                walk(v, key, result);
            }
        }
        _ => {}
    }
}
fn first(value: &Value, key: &str) -> Option<Value> {
    let mut values = Vec::new();
    walk(value, key, &mut values);
    values.into_iter().next()
}
pub fn parse_tracks(value: &Value) -> Vec<Track> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    // A single depth-first walk preserves provider order across renderer variants.
    fn collect(v: &Value, out: &mut Vec<(String, Value)>) {
        match v {
            Value::Object(m) => {
                for (k, v) in m {
                    if k == "playlistPanelVideoWrapperRenderer" {
                        collect(&v["primaryRenderer"], out);
                        continue;
                    }
                    if [
                        "playlistPanelVideoRenderer",
                        "musicResponsiveListItemRenderer",
                        "musicTwoRowItemRenderer",
                    ]
                    .contains(&k.as_str())
                    {
                        out.push((k.clone(), v.clone()));
                    } else {
                        collect(v, out);
                    }
                }
            }
            Value::Array(a) => {
                for v in a {
                    collect(v, out);
                }
            }
            _ => {}
        }
    }
    let mut rows = Vec::new();
    collect(value, &mut rows);
    for (kind, row) in rows {
        if row.get("unplayableText").is_some() {
            continue;
        }
        if row["navigationEndpoint"]["browseEndpoint"].is_object()
            && row["videoId"].is_null()
            && row["playlistItemData"]["videoId"].is_null()
        {
            continue;
        }
        let id = row["videoId"]
            .as_str()
            .or(row["playlistItemData"]["videoId"].as_str())
            .or(row["navigationEndpoint"]["watchEndpoint"]["videoId"].as_str())
            .or(row["onTap"]["watchEndpoint"]["videoId"].as_str())
            .or(
                row["flexColumns"][0]["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"]
                    [0]["navigationEndpoint"]["watchEndpoint"]["videoId"]
                    .as_str(),
            )
            .map(str::to_string)
            .or_else(|| {
                first(&row, "watchEndpoint")?["videoId"]
                    .as_str()
                    .map(str::to_string)
            });
        let Some(id) = id else {
            continue;
        };
        if validate_video(&id).is_err() || !seen.insert(id.clone()) {
            continue;
        }
        let title = if kind == "musicResponsiveListItemRenderer" {
            text(&row["flexColumns"][0]["musicResponsiveListItemFlexColumnRenderer"]["text"])
        } else {
            text(&row["title"])
        };
        if title.is_empty() {
            continue;
        }
        let byline = if kind == "musicResponsiveListItemRenderer" {
            row["flexColumns"][1]["musicResponsiveListItemFlexColumnRenderer"]["text"].clone()
        } else {
            row.get("longBylineText")
                .or(row.get("shortBylineText"))
                .or(row.get("subtitle"))
                .cloned()
                .unwrap_or(Value::Null)
        };
        let mut artist = Vec::new();
        let mut album = String::new();
        for run in byline["runs"].as_array().into_iter().flatten() {
            let browse = &run["navigationEndpoint"]["browseEndpoint"];
            let name = run["text"].as_str().unwrap_or("");
            if browse["browseId"]
                .as_str()
                .is_some_and(|id| id.starts_with("MPRE"))
            {
                album = name.into();
            } else if browse["browseId"]
                .as_str()
                .is_some_and(|id| id.starts_with("UC"))
            {
                artist.push(name);
            }
        }
        let artist = if artist.is_empty() {
            let byline = text(&byline);
            byline
                .split(" • ")
                .map(str::trim)
                .find(|part| {
                    !part.is_empty()
                        && !["Song", "Video", "Music video"].contains(part)
                        && duration(part) == 0.
                })
                .unwrap_or("")
                .to_string()
        } else {
            artist.join(", ")
        };
        let cover = first(&row, "thumbnails")
            .and_then(|t| t.as_array()?.last()?["url"].as_str().map(str::to_string));
        let length = text(&row["lengthText"]);
        let duration = if length.is_empty() {
            let mut fields = Vec::new();
            walk(&row["fixedColumns"], "text", &mut fields);
            fields
                .iter()
                .map(text)
                .map(|s| duration(&s))
                .find(|n| *n > 0.)
                .or_else(|| {
                    byline["runs"]
                        .as_array()?
                        .iter()
                        .filter_map(|r| r["text"].as_str())
                        .filter(|s| s.contains(':'))
                        .map(duration)
                        .find(|d| *d > 0.)
                })
                .or_else(|| {
                    let columns = row["flexColumns"].as_array()?;
                    for column in columns.iter().skip(1) {
                        let field = &column["musicResponsiveListItemFlexColumnRenderer"]["text"];
                        for run in field["runs"].as_array().into_iter().flatten() {
                            let value = duration(run["text"].as_str().unwrap_or(""));
                            if value > 0. {
                                return Some(value);
                            }
                        }
                    }
                    None
                })
                .unwrap_or(0.)
        } else {
            duration(&length)
        };
        result.push(Track {
            id: format!("youtube:{id}"),
            title,
            artist,
            album,
            duration,
            source: format!("https://music.youtube.com/watch?v={id}"),
            platform: "ytmusic".into(),
            cover,
            local_path: None,
            cache_path: None,
        });
    }
    result
}
pub fn parse_items(value: &Value) -> Vec<Value> {
    let mut rows = Vec::new();
    walk(value, "musicTwoRowItemRenderer", &mut rows);
    walk(value, "musicResponsiveListItemRenderer", &mut rows);
    let mut seen = HashSet::new();
    rows.into_iter().filter_map(|row|{let browse=row.get("navigationEndpoint").and_then(|n|n.get("browseEndpoint")).cloned().or_else(||first(&row["flexColumns"][0],"browseEndpoint"))?;let id=browse["browseId"].as_str()?;if !seen.insert(id.to_string()){return None;}
        let kind=if id.starts_with("UC"){"artist"}else if id.starts_with("MPRE"){"album"}else{"playlist"};
        let title=if row["title"].is_object(){text(&row["title"])}else{text(&row["flexColumns"][0]["musicResponsiveListItemFlexColumnRenderer"]["text"])};
        let subtitle = if row["subtitle"].is_object() { text(&row["subtitle"]) } else {
            text(&row["flexColumns"][1]["musicResponsiveListItemFlexColumnRenderer"]["text"])
        };
        Some(json!({"id":id,"kind":kind,"title":title,"subtitle":subtitle,"params":browse["params"],"cover":first(&row,"thumbnails").and_then(|t|t.as_array()?.last()?["url"].as_str().map(str::to_string))}))
    }).collect()
}
fn is_generated_mix_id(id: &str) -> bool {
    id.starts_with("RD")
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}

pub(crate) fn parse_generated_mixes(value: &Value) -> Vec<Value> {
    let mut rows = Vec::new();
    walk(value, "musicTwoRowItemRenderer", &mut rows);
    walk(value, "musicResponsiveListItemRenderer", &mut rows);
    let mut seen = HashSet::new();
    rows.into_iter().filter_map(|row| {
        let nav = &row["navigationEndpoint"];
        let endpoint = [&nav["watchPlaylistEndpoint"], &nav["watchEndpoint"], &nav["browseEndpoint"]]
            .into_iter().find(|endpoint| !endpoint.is_null()).cloned()
            .or_else(|| first(&row, "watchPlaylistEndpoint"))?;
        let id = endpoint["playlistId"].as_str().or(endpoint["browseId"].as_str())?;
        let id = id.strip_prefix("VL").unwrap_or(id);
        if !is_generated_mix_id(id) || !seen.insert(id.to_owned()) {
            return None;
        }
        let title = text(&row["title"]);
        let title = if title.is_empty() {
            text(&row["flexColumns"][0]["musicResponsiveListItemFlexColumnRenderer"]["text"])
        } else { title };
        Some(json!({"id":id,"title":title,"video":endpoint["videoId"],"params":endpoint["params"]}))
    }).collect()
}

fn find_continuation(value: &Value) -> Option<String> {
    for key in ["nextRadioContinuationData", "nextContinuationData"] {
        if let Some(v) = first(value, key)
            && let Some(t) = v["continuation"].as_str()
        {
            return Some(t.into());
        }
    }
    None
}

#[cfg(test)]
mod generated_mix_tests {
    use super::*;

    #[test]
    fn recognizes_generated_playlist_navigation_without_including_owned_playlists() {
        let row = |id: &str, endpoint: &str| {
            json!({"musicTwoRowItemRenderer":{"title":{"simpleText":"Mix"},
                "navigationEndpoint":{(endpoint):{"playlistId":id,"videoId":"dQw4w9WgXcQ"}}}})
        };
        let home = json!({"contents":[row("RDMM","watchPlaylistEndpoint"),row("PLmine","watchPlaylistEndpoint"),row("RDMM","watchEndpoint"),row("RDTMAKexample","watchEndpoint")]});
        let result = parse_generated_mixes(&home);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0]["id"], "RDMM");
        assert_eq!(result[1]["id"], "RDTMAKexample");
        assert_eq!(result[0]["video"], "dQw4w9WgXcQ");
    }
}
