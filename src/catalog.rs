use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

pub fn metadata_with_proxy(
    client: &reqwest::blocking::Client,
    input: &str,
    token: Option<&str>,
    proxy: Option<&str>,
) -> Result<Value> {
    let parsed = url::Url::parse(input)?;
    let host = parsed.host_str().unwrap_or("");
    if host == "tiktok.com" || host.ends_with(".tiktok.com") {
        return crate::imports::metadata_with_proxy(client, input, proxy);
    }
    metadata(client, input, token)
}

/// Preserve catalog metadata and resolve a verified recording only when played.
pub fn import_plan(metadata: &Value, source: &str, allow_partial: bool) -> Result<Value> {
    ensure!(
        allow_partial || metadata["partial"] != true,
        "Provider exposed only part of this collection; import was not applied"
    );
    if metadata["spotify_tracks"].is_array() {
        return Ok(
            json!({"name":metadata["name"],"tracks":metadata["spotify_tracks"],"total":metadata["total"],"complete":metadata["complete"],"access":metadata["access"],"provider":"spotify","unmatched":[],"matching":"deferred_until_playback"}),
        );
    }
    let items = metadata["tracks"].as_array().context("No catalog tracks")?;
    let mut tracks = Vec::new();
    let mut unmatched = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let title = item["title"].as_str().unwrap_or("").trim();
        let artist = item["artist"].as_str().unwrap_or("").trim();
        if title.is_empty() || artist.is_empty() {
            unmatched.push(json!({"track":item,"error":"Missing title or artist"}));
            continue;
        }
        tracks.push(crate::model::Track {
            id: format!(
                "catalog:{}",
                crate::store::key(&format!("{source}|{index}|{artist}|{title}"))
            ),
            title: title.into(),
            artist: artist.into(),
            album: item["album"].as_str().unwrap_or("").into(),
            duration: item["duration"].as_f64().unwrap_or(0.),
            source: source.into(),
            platform: "catalog".into(),
            cover: item["cover"]
                .as_str()
                .or(metadata["cover"].as_str())
                .map(str::to_owned),
            local_path: None,
            cache_path: None,
        });
    }
    Ok(
        json!({"name":metadata["name"],"tracks":tracks,"total":metadata["total"],"partial":metadata["partial"],"unmatched":unmatched,"matching":"deferred_until_playback"}),
    )
}

pub fn metadata(
    client: &reqwest::blocking::Client,
    input: &str,
    spotify_token: Option<&str>,
) -> Result<Value> {
    let normalized = if let Some(id) = input.strip_prefix("spotify:playlist:") {
        format!("https://open.spotify.com/playlist/{id}")
    } else {
        input.to_owned()
    };
    let parsed = url::Url::parse(&normalized)?;
    if parsed.host_str() == Some("open.spotify.com") {
        crate::spotify_public::entity_id(&normalized, "playlist")?;
        if spotify_token.is_some()
            && let Ok(value) = metadata_api(client, &normalized, spotify_token)
        {
            return Ok(value);
        }
        return crate::spotify_public::playlist(client, &normalized);
    }
    metadata_api(client, &normalized, spotify_token)
}

fn metadata_api(
    client: &reqwest::blocking::Client,
    url: &str,
    spotify_token: Option<&str>,
) -> Result<Value> {
    let parsed = url::Url::parse(url)?;
    let host = parsed.host_str().unwrap_or("");
    if (host == "music.apple.com" && parsed.path().contains("/playlist/"))
        || [
            "music.yandex.ru",
            "music.yandex.com",
            "music.yandex.kz",
            "vk.com",
            "www.vk.com",
            "m.vk.com",
            "vk.ru",
            "www.vk.ru",
        ]
        .contains(&host)
        || host == "tiktok.com"
        || host.ends_with(".tiktok.com")
    {
        return crate::imports::metadata(client, url);
    }
    let parts: Vec<_> = parsed
        .path_segments()
        .context("Invalid playlist URL")?
        .collect();
    let mut tracks = Vec::new();
    let name;
    if host == "www.deezer.com" || host == "deezer.com" {
        let at = parts
            .iter()
            .position(|x| *x == "playlist")
            .context("Expected Deezer playlist URL")?;
        let id = parts.get(at + 1).context("Missing playlist ID")?;
        ensure!(
            id.chars().all(|c| c.is_ascii_digit()),
            "Invalid playlist ID"
        );
        let value: Value = client
            .get(format!("https://api.deezer.com/playlist/{id}"))
            .send()?
            .error_for_status()?
            .json()?;
        ensure!(value.get("error").is_none(), "Deezer playlist unavailable");
        name = value["title"]
            .as_str()
            .unwrap_or("Deezer playlist")
            .to_string();
        let mut page = value["tracks"].clone();
        let mut seen = std::collections::HashSet::new();
        loop {
            for t in page["data"].as_array().into_iter().flatten() {
                tracks.push(json!({"title":t["title"],"artist":t["artist"]["name"],"duration":t["duration"]}));
            }
            let Some(next) = page["next"].as_str() else {
                break;
            };
            let parsed = url::Url::parse(next)?;
            ensure!(
                parsed.host_str() == Some("api.deezer.com")
                    && seen.insert(next.to_string())
                    && tracks.len() < 10000,
                "Invalid pagination"
            );
            page = client.get(next).send()?.error_for_status()?.json()?;
        }
    } else if host == "open.spotify.com" {
        let at = parts
            .iter()
            .position(|x| *x == "playlist")
            .context("Expected Spotify playlist URL")?;
        let id = parts.get(at + 1).context("Missing playlist ID")?;
        ensure!(
            id.chars().all(|c| c.is_ascii_alphanumeric()),
            "Invalid playlist ID"
        );
        let token =
            spotify_token.context("Spotify playlist import requires spotify OAuth sign-in")?;
        let value: Value = client
            .get(format!("https://api.spotify.com/v1/playlists/{id}"))
            .bearer_auth(token)
            .send()?
            .error_for_status()?
            .json()?;
        name = value["name"]
            .as_str()
            .unwrap_or("Spotify playlist")
            .to_string();
        let mut page = if value["tracks"].is_object() {
            value["tracks"].clone()
        } else {
            value["items"].clone()
        };
        let total = page["total"].as_u64();
        let mut native = Vec::new();
        let mut seen = std::collections::HashSet::new();
        loop {
            for item in page["items"]
                .as_array()
                .context("Spotify did not expose playlist contents")?
            {
                let track = if item["track"].is_object() {
                    &item["track"]
                } else {
                    &item["item"]
                };
                let parsed = crate::spotify::parse_tracks(&json!({"tracks":{"items":[track]}}));
                ensure!(
                    parsed.len() == 1,
                    "Spotify exposed an unavailable playlist entry"
                );
                native.extend(parsed);
            }
            let Some(next) = page["next"].as_str() else {
                break;
            };
            let url = url::Url::parse(next)?;
            ensure!(
                url.scheme() == "https"
                    && url.host_str() == Some("api.spotify.com")
                    && seen.insert(next.to_owned())
                    && native.len() < 10000,
                "Invalid Spotify pagination"
            );
            page = client
                .get(next)
                .bearer_auth(token)
                .send()?
                .error_for_status()?
                .json()?;
        }
        ensure!(
            total.is_none_or(|n| n as usize == native.len()),
            "Spotify returned an incomplete playlist"
        );
        let metadata:Vec<_>=native.iter().map(|t|json!({"title":t.title,"artist":t.artist,"duration":t.duration,"cover":t.cover})).collect();
        return Ok(
            json!({"name":name,"tracks":metadata,"total":native.len(),"complete":true,"spotify_tracks":native,"provider":"spotify","access":"account"}),
        );
    } else if host == "music.apple.com" {
        let at = parts
            .iter()
            .position(|x| *x == "album")
            .context("Apple Music currently supports albums only")?;
        let id = parts.get(at + 2).context("Missing Apple album ID")?;
        ensure!(id.chars().all(|c| c.is_ascii_digit()), "Invalid album ID");
        let country = parts.first().copied().unwrap_or("us");
        let value: Value = client
            .get("https://itunes.apple.com/lookup")
            .query(&[("id", *id), ("entity", "song"), ("country", country)])
            .send()?
            .error_for_status()?
            .json()?;
        let results = value["results"]
            .as_array()
            .context("Invalid Apple response")?;
        ensure!(
            !results.is_empty(),
            "Apple Music album was not found in this country"
        );
        name = results
            .first()
            .and_then(|t| t["collectionName"].as_str())
            .unwrap_or("Apple Music album")
            .to_string();
        for t in results.iter().filter(|t| t["wrapperType"] == "track") {
            tracks.push(json!({"title":t["trackName"],"artist":t["artistName"],"duration":t["trackTimeMillis"].as_f64().unwrap_or(0.)/1000.}));
        }
    } else {
        anyhow::bail!(
            "Supported metadata imports: Spotify playlist, Deezer playlist, Apple Music album"
        );
    }
    Ok(json!({"name":name,"tracks":tracks}))
}
