//! Public Spotify embed + guest Pathfinder, matching the original Riff import flow.
//! Guest tokens come only from public pages, stay in RAM and are never exported.
use crate::{jobs, model::Track};
use anyhow::{Context, Result, ensure};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::{
    io::Read,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

const PLAYLIST_HASH: &str = "8964e8eafb21aa992a7d951d256d83285c04be2105d209262901de70cb97584a";
const SEARCH_HASH: &str = "d9f785900f0710b31c07818d617f4f7600c1e21217e80f5b043d1e78d74e6026";
const UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120.0.0.0 Safari/537.36";
static GUEST: OnceLock<Mutex<Option<(Instant, String)>>> = OnceLock::new();

pub fn entity_id(input: &str, kind: &str) -> Result<String> {
    let id = if let Some(id) = input.strip_prefix(&format!("spotify:{kind}:")) {
        id.to_owned()
    } else {
        let url = url::Url::parse(input)?;
        ensure!(
            url.scheme() == "https"
                && url.host_str() == Some("open.spotify.com")
                && url.username().is_empty()
                && url.password().is_none(),
            "Expected a public Spotify HTTPS link"
        );
        let parts: Vec<_> = url
            .path_segments()
            .context("Invalid Spotify URL")?
            .collect();
        let at = parts
            .iter()
            .position(|p| *p == kind)
            .context("Unexpected Spotify link type")?;
        parts.get(at + 1).context("Missing Spotify ID")?.to_string()
    };
    ensure!(
        id.len() == 22 && id.bytes().all(|b| b.is_ascii_alphanumeric()),
        "Invalid Spotify ID"
    );
    Ok(id)
}
fn bounded(response: reqwest::blocking::Response) -> Result<String> {
    let mut text = String::new();
    response.take(6_000_001).read_to_string(&mut text)?;
    ensure!(text.len() <= 6_000_000, "Spotify response too large");
    Ok(text)
}
pub fn parse_embed(html: &str) -> Result<Value> {
    let doc = scraper::Html::parse_document(html);
    let selector = scraper::Selector::parse("script#__NEXT_DATA__").expect("constant selector");
    let script = doc
        .select(&selector)
        .next()
        .context("Spotify public page did not expose embed data")?;
    let data: Value =
        serde_json::from_str(&script.inner_html()).context("Invalid Spotify embed data")?;
    ensure!(
        data["props"]["pageProps"]["status"] != 404,
        "Spotify item was not found or is private"
    );
    Ok(data)
}
fn embed(client: &Client, kind: &str, id: &str) -> Result<Value> {
    jobs::check_cancelled()?;
    let response = client
        .get(format!("https://open.spotify.com/embed/{kind}/{id}"))
        .header("User-Agent", UA)
        .header("Accept-Language", "en-US,en;q=0.9")
        .timeout(Duration::from_secs(15))
        .send()?;
    let response = check_status(response)?;
    parse_embed(&bounded(response)?)
}
fn check_status(response: reqwest::blocking::Response) -> Result<reqwest::blocking::Response> {
    if response.status().as_u16() == 429 {
        anyhow::bail!(
            "Spotify is limiting public requests; retry after {} seconds",
            response
                .headers()
                .get("retry-after")
                .and_then(|s| s.to_str().ok())
                .unwrap_or("a few")
        );
    }
    response
        .error_for_status()
        .context("Spotify public service unavailable")
}
fn token_from(data: &Value) -> Option<String> {
    data["props"]["pageProps"]["state"]["settings"]["session"]["accessToken"]
        .as_str()
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}
fn remember(token: &str) {
    if let Ok(mut cache) = GUEST.get_or_init(Default::default).lock() {
        *cache = Some((Instant::now(), token.to_owned()));
    }
}
fn guest(client: &Client) -> Result<String> {
    if let Some(token) = GUEST
        .get_or_init(Default::default)
        .lock()
        .ok()
        .and_then(|v| {
            v.as_ref()
                .filter(|(at, _)| at.elapsed() < Duration::from_secs(600))
                .map(|(_, t)| t.clone())
        })
    {
        return Ok(token);
    }
    let value = embed(client, "track", "4uLU6hMCjMI75M1A2tKUQC")?;
    let token =
        token_from(&value).context("Spotify did not expose a public guest session; retry later")?;
    remember(&token);
    Ok(token)
}
fn query(
    client: &Client,
    token: &str,
    operation: &str,
    hash: &str,
    variables: Value,
) -> Result<Value> {
    jobs::check_cancelled()?;
    let response=client.post("https://api-partner.spotify.com/pathfinder/v1/query")
        .bearer_auth(token).header("User-Agent",UA).header("Referer","https://open.spotify.com/")
        .timeout(Duration::from_secs(15)).json(&json!({"operationName":operation,"variables":variables,"extensions":{"persistedQuery":{"version":1,"sha256Hash":hash}}})).send()?;
    if response.status().as_u16() == 401 || response.status().as_u16() == 403 {
        if let Ok(mut cache) = GUEST.get_or_init(Default::default).lock() {
            *cache = None;
        }
        anyhow::bail!("Spotify public guest session expired or was refused; retry to refresh it");
    }
    let value: Value = serde_json::from_str(&bounded(check_status(response)?)?)?;
    ensure!(
        value["errors"].as_array().is_none_or(|e| e.is_empty()),
        "Spotify public query changed or is unavailable; no substitute results were used"
    );
    Ok(value)
}
fn cover(value: &Value) -> Option<String> {
    value["coverArt"]["sources"][0]["url"]
        .as_str()
        .or(value["visualIdentity"]["image"][0]["url"].as_str())
        .or(value["image"].as_str())
        .map(str::to_owned)
}
pub fn parse_track(value: &Value, fallback_cover: Option<&str>) -> Option<Track> {
    let id = value["uri"].as_str()?.strip_prefix("spotify:track:")?;
    if id.len() != 22 || !id.bytes().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let title = value["name"].as_str().or(value["title"].as_str())?.trim();
    if title.is_empty() {
        return None;
    }
    let artists = &value["artists"];
    let list = artists
        .as_array()
        .or(artists["items"].as_array())
        .or(value["albumOfTrack"]["artists"]["items"].as_array());
    let artist = value["subtitle"]
        .as_str()
        .or(value["artist"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            list.into_iter()
                .flatten()
                .filter_map(|a| {
                    a["profile"]["name"]
                        .as_str()
                        .or(a["name"].as_str())
                        .or(a.as_str())
                })
                .collect::<Vec<_>>()
                .join(", ")
        });
    let ms = value["trackDuration"]["totalMilliseconds"]
        .as_f64()
        .or(value["duration"]["totalMilliseconds"].as_f64())
        .or(value["duration_ms"].as_f64())
        .or(value["duration"].as_f64())
        .unwrap_or(0.);
    Some(Track {
        id: format!("spotify:{id}"),
        title: title.into(),
        artist,
        album: value["albumOfTrack"]["name"].as_str().unwrap_or("").into(),
        duration: ms / 1000.,
        source: format!("https://open.spotify.com/track/{id}"),
        platform: "spotify".into(),
        cover: cover(value)
            .or_else(|| cover(&value["albumOfTrack"]))
            .or_else(|| fallback_cover.map(str::to_owned)),
        local_path: None,
        cache_path: None,
    })
}
pub fn parse_search(value: &Value) -> Vec<Track> {
    value["data"]["searchV2"]["tracksV2"]["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| parse_track(&v["item"]["data"], None))
        .collect()
}

pub fn complete_embed_tracks(entity: &Value) -> Result<Vec<Track>> {
    let list = entity["trackList"]
        .as_array()
        .context("Spotify embed did not expose tracks")?;
    let total = entity["totalCount"]
        .as_u64()
        .context("Spotify embed completeness is unknown")? as usize;
    ensure!(
        total == list.len() && total <= 100,
        "Spotify exposed only part of this playlist; import was not applied"
    );
    let art = cover(entity);
    list.iter()
        .map(|v| {
            parse_track(v, art.as_deref()).context("Spotify embed contains unavailable tracks")
        })
        .collect()
}
pub fn search(client: &Client, query_text: &str, limit: usize) -> Result<Value> {
    ensure!(!query_text.trim().is_empty(), "Empty search");
    ensure!((1..=100).contains(&limit), "Search limit must be 1..100");
    if query_text.starts_with("https://open.spotify.com/") || query_text.starts_with("spotify:") {
        if let Ok(id) = entity_id(query_text, "track") {
            let data = embed(client, "track", &id)?;
            let t = parse_track(&data["props"]["pageProps"]["state"]["data"]["entity"], None)
                .context("Spotify track unavailable")?;
            return Ok(json!({"tracks":[t],"provider":"spotify","access":"public_embed"}));
        }
        let data = playlist(client, query_text)?;
        return Ok(
            json!({"tracks":data["spotify_tracks"],"name":data["name"],"provider":"spotify","access":"public_embed","total":data["total"]}),
        );
    }
    let token = guest(client)?;
    let data = query(
        client,
        &token,
        "searchDesktop",
        SEARCH_HASH,
        json!({"searchTerm":query_text,"offset":0,"limit":limit,"numberOfTopResults":5,"includeAudiobooks":false,"includeArtistHasConcertsField":false,"includePreReleases":false,"includeLocalConcertsField":false,"includeAuthors":false}),
    )?;
    ensure!(
        data["data"]["searchV2"]["tracksV2"]["items"].is_array(),
        "Spotify did not return public search results"
    );
    Ok(
        json!({"tracks":parse_search(&data),"provider":"spotify","access":"public_guest","playback":"matched_youtube_music","attribution_url":"https://open.spotify.com"}),
    )
}
pub fn playlist(client: &Client, input: &str) -> Result<Value> {
    let id = entity_id(input, "playlist")?;
    let state = embed(client, "playlist", &id)?;
    let entity = &state["props"]["pageProps"]["state"]["data"]["entity"];
    ensure!(
        entity.is_object(),
        "Spotify playlist was not found or is private"
    );
    let name = entity["name"]
        .as_str()
        .or(entity["title"].as_str())
        .unwrap_or("Spotify playlist");
    let art = cover(entity);
    let token = token_from(&state).or_else(|| guest(client).ok());
    let full = (|| -> Result<(Vec<Track>, usize)> {
        let token = token.context("No public guest session")?;
        remember(&token);
        let mut tracks = Vec::new();
        let mut offset = 0;
        let mut expected_total = None;
        let mut seen_pages = std::collections::HashSet::new();
        loop {
            let data = query(
                client,
                &token,
                "fetchPlaylist",
                PLAYLIST_HASH,
                json!({"uri":format!("spotify:playlist:{id}"),"offset":offset,"limit":100,"enableWatchFeedEntrypoint":true}),
            )?;
            let content = &data["data"]["playlistV2"]["content"];
            let items = content["items"]
                .as_array()
                .context("Spotify public playlist unavailable")?;
            let total = content["totalCount"]
                .as_u64()
                .context("Spotify did not expose playlist size")? as usize;
            ensure!(total <= 10000, "Spotify playlist exceeds 10000 tracks");
            ensure!(
                expected_total.is_none_or(|n| n == total),
                "Spotify playlist changed during import; retry"
            );
            expected_total = Some(total);
            ensure!(
                seen_pages.insert(serde_json::to_string(items)?),
                "Spotify repeated a page; refusing an incomplete import"
            );
            for item in items {
                let v = if item["itemV2"]["data"].is_object() {
                    &item["itemV2"]["data"]
                } else {
                    &item["item"]["data"]
                };
                tracks.push(
                    parse_track(v, art.as_deref()).context(
                        "Spotify exposed an unavailable track; refusing incomplete import",
                    )?,
                );
            }
            offset += items.len();
            if offset >= total {
                ensure!(
                    tracks.len() == total,
                    "Spotify exposed an incomplete playlist"
                );
                return Ok((tracks, total));
            }
            ensure!(
                !items.is_empty() && offset < 10000,
                "Spotify exposed an incomplete playlist"
            );
        }
    })();
    let (tracks, total, access) = match full {
        Ok((t, n)) => (t, n, "public_guest"),
        Err(error) => {
            let t=complete_embed_tracks(entity).with_context(||format!("Could not read complete public playlist: {error}. Retry later or optionally connect an account."))?;
            let total = t.len();
            (t, total, "public_embed")
        }
    };
    let metadata:Vec<_>=tracks.iter().map(|t|json!({"title":t.title,"artist":t.artist,"duration":t.duration,"cover":t.cover,"spotify_url":t.source})).collect();
    Ok(
        json!({"name":name,"cover":art,"tracks":metadata,"spotify_tracks":tracks,"total":total,"complete":true,"access":access,"provider":"spotify"}),
    )
}
