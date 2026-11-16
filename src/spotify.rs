use crate::model::Track;
use anyhow::{Result, ensure};
use serde_json::Value;

pub fn search_auto(
    client: &reqwest::blocking::Client,
    token: Option<&str>,
    query: &str,
    limit: usize,
) -> Result<Value> {
    if let Some(token) = token
        && !query.starts_with("https://open.spotify.com/")
        && !query.starts_with("spotify:")
        && let Ok(value) = search(client, token, query, limit.min(10))
    {
        return Ok(value);
    }
    crate::spotify_public::search(client, query, limit)
}

/// Spotify catalog metadata only. Full Spotify audio is not exposed by Web API.
pub fn search(
    client: &reqwest::blocking::Client,
    token: &str,
    query: &str,
    limit: usize,
) -> Result<Value> {
    ensure!(!query.trim().is_empty(), "Empty search");
    ensure!(
        (1..=10).contains(&limit),
        "Spotify search limit must be 1..10"
    );
    let response = client
        .get("https://api.spotify.com/v1/search")
        .bearer_auth(token)
        .query(&[
            ("q", query),
            ("type", "track"),
            ("limit", &limit.to_string()),
        ])
        .send()?;
    if response.status().as_u16() == 429 {
        anyhow::bail!(
            "Spotify rate limited search; retry after {} seconds",
            response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("a few")
        );
    }
    let value: Value = response.error_for_status()?.json()?;
    Ok(
        serde_json::json!({"tracks":parse_tracks(&value),"provider":"spotify","playback":"matched_youtube_music","attribution_url":"https://open.spotify.com"}),
    )
}
pub fn parse_tracks(value: &Value) -> Vec<Track> {
    value["tracks"]["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| {
            let id = v["id"].as_str()?;
            let source = v["external_urls"]["spotify"].as_str()?.to_owned();
            let url = url::Url::parse(&source).ok()?;
            if url.scheme() != "https" || url.host_str() != Some("open.spotify.com") {
                return None;
            }
            Some(Track {
                id: format!("spotify:{id}"),
                title: v["name"].as_str()?.into(),
                artist: v["artists"]
                    .as_array()?
                    .iter()
                    .filter_map(|a| a["name"].as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                album: v["album"]["name"].as_str().unwrap_or("").into(),
                duration: v["duration_ms"].as_f64().unwrap_or(0.) / 1000.,
                source,
                platform: "spotify".into(),
                cover: v["album"]["images"][0]["url"].as_str().map(str::to_owned),
                local_path: None,
                cache_path: None,
            })
        })
        .collect()
}

pub fn library(client: &reqwest::blocking::Client, token: &str, kind: &str) -> Result<Value> {
    ensure!(
        ["liked", "playlists"].contains(&kind),
        "Spotify library kind must be liked or playlists"
    );
    let mut next = Some(format!(
        "https://api.spotify.com/v1/me/{}?limit=50",
        if kind == "liked" {
            "tracks"
        } else {
            "playlists"
        }
    ));
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();
    while let Some(url) = next {
        crate::jobs::check_cancelled()?;
        let parsed = url::Url::parse(&url)?;
        ensure!(
            parsed.scheme() == "https"
                && parsed.host_str() == Some("api.spotify.com")
                && seen.insert(url.clone())
                && items.len() < 10000,
            "Invalid Spotify library pagination"
        );
        let response = client.get(url).bearer_auth(token).send()?;
        ensure!(
            response.status().as_u16() != 403,
            "Spotify refused account access; check the app permissions and Spotify developer account limits"
        );
        let page: Value = response.error_for_status()?.json()?;
        let entries = page["items"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Invalid Spotify library response"))?;
        items.extend(entries.iter().cloned());
        next = page["next"].as_str().map(str::to_owned);
    }
    if kind == "liked" {
        let raw: Vec<_> = items.iter().map(|v| v["track"].clone()).collect();
        let tracks = parse_tracks(&serde_json::json!({"tracks":{"items":raw}}));
        Ok(
            serde_json::json!({"kind":kind,"tracks":tracks,"total":items.len(),"skipped":items.len()-tracks.len()}),
        )
    } else {
        Ok(serde_json::json!({"kind":kind,"items":items}))
    }
}
fn words(text: &str) -> std::collections::HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}
/// Compare arrangement and identity before resolving; unknown time is provisional only.
pub fn match_score(track: &Track, candidate: &Track) -> Option<f64> {
    fn title_words(title: &str) -> std::collections::HashSet<String> {
        static EDITION: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let re=EDITION.get_or_init(||regex::Regex::new(r"(?i)\s*(?:[-\u{2013}]\s*)?(?:[\(\[])?(?:\d{4}\s+)?remaster(?:ed)?(?:\s+\d{4})?(?:[\)\]])?\s*$").expect("constant regex"));
        let clean = re.replace(title, "");
        words(&clean)
            .into_iter()
            .filter(|w| {
                ![
                    "official",
                    "audio",
                    "video",
                    "lyrics",
                    "feat",
                    "ft",
                    "featuring",
                ]
                .contains(&w.as_str())
            })
            .collect()
    }
    let title = title_words(&track.title);
    let other = title_words(&candidate.title);
    let artist = words(&track.artist);
    let performers = words(&candidate.artist);
    let overlap = title.intersection(&other).count() as f64;
    let recall = overlap / title.len().max(1) as f64;
    let precision = overlap / other.len().max(1) as f64;
    let artist_score = artist.intersection(&performers).count() as f64 / artist.len().max(1) as f64;
    if recall < 0.8 || precision < 0.45 || artist_score < 0.5 {
        return None;
    }
    let variants = [
        "live",
        "liveshow",
        "remix",
        "mix",
        "dub",
        "extended",
        "instrumental",
        "karaoke",
        "cover",
        "sped",
        "slowed",
        "acoustic",
        "demo",
        "rework",
        "edit",
        "short",
        "workout",
        "medley",
        "mashup",
        "podcast",
        "reaction",
        "explained",
        "review",
    ];
    for variant in variants {
        if title.contains(variant) != other.contains(variant) {
            return None;
        }
    }
    if track.duration > 0.
        && candidate.duration > 0.
        && !duration_matches(track.duration, candidate.duration)
    {
        return None;
    }
    let timing = if track.duration > 0. && candidate.duration > 0. {
        1. - (track.duration - candidate.duration).abs() / duration_tolerance(track.duration)
    } else {
        0.
    };
    Some(recall + precision + artist_score + timing.max(0.) * 0.5)
}
pub fn duration_tolerance(expected: f64) -> f64 {
    (expected * 0.03).clamp(3., 10.)
}
pub fn duration_matches(expected: f64, actual: f64) -> bool {
    expected.is_finite()
        && actual.is_finite()
        && expected > 0.
        && actual > 0.
        && (expected - actual).abs() <= duration_tolerance(expected)
}
pub fn ranked_matches<'a>(track: &Track, candidates: &'a [Track]) -> Vec<&'a Track> {
    let mut scored: Vec<_> = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, t)| Some((t, match_score(track, t)?, index)))
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.2.cmp(&b.2)));
    scored.into_iter().map(|(t, _, _)| t).collect()
}
pub fn best_match<'a>(track: &Track, candidates: &'a [Track]) -> Option<&'a Track> {
    ranked_matches(track, candidates).into_iter().next()
}
