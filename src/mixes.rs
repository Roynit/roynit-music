use crate::{
    model::{Library, Track},
    youtube::Music,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mix {
    pub name: String,
    pub tracks: Vec<Track>,
}

pub(crate) fn seeds(library: &Library, now: u64) -> Vec<Track> {
    let mut scores = HashMap::<&str, u64>::new();
    for event in library.history.iter().rev().take(150) {
        let age_days = now.saturating_sub(event.started_at) / 86_400;
        if age_days < 90 {
            *scores.entry(&event.track_id).or_default() += 1 + 12 / (1 + age_days / 3);
        }
    }
    for id in &library.favorites {
        *scores.entry(id).or_default() += 18;
    }
    let mut ranked: Vec<_> = scores
        .into_iter()
        .filter_map(|(id, score)| {
            let track = library.tracks.get(id)?;
            matches!(
                track.platform.as_str(),
                "youtube" | "ytmusic" | "spotify" | "catalog"
            )
            .then_some((score, track))
        })
        .collect();
    ranked.sort_by(|(a_score, a), (b_score, b)| b_score.cmp(a_score).then_with(|| a.id.cmp(&b.id)));
    let mut artists = HashSet::new();
    ranked
        .into_iter()
        .filter(|(_, track)| {
            let name = track.artist.trim().to_lowercase();
            artists.insert(if name.is_empty() {
                track.id.clone()
            } else {
                name
            })
        })
        .take(3)
        .map(|(_, track)| track.clone())
        .collect()
}

pub(crate) fn generate(music: &Music<'_>, seeds: &[Track]) -> Result<Vec<Mix>> {
    let token = crate::jobs::thread_cancel_token();
    let results = std::thread::scope(|scope| {
        let workers: Vec<_> = seeds
            .iter()
            .map(|seed| {
                let token = token.clone();
                scope.spawn(move || {
                    if let Some(token) = token {
                        crate::jobs::set_thread_cancel_token(token);
                    }
                    build(music, seed)
                })
            })
            .collect();
        workers
            .into_iter()
            .filter_map(|worker| worker.join().ok()?.ok())
            .collect::<Vec<_>>()
    });
    crate::jobs::check_cancelled()?;
    ensure!(
        seeds.is_empty() || !results.is_empty(),
        "YouTube Music could not build a mix from these songs"
    );
    Ok(results)
}

pub(crate) fn build(music: &Music<'_>, seed: &Track) -> Result<Mix> {
    let video = if let Some(id) = crate::youtube_stream::video_id(&seed.source) {
        id
    } else {
        let result = music.search_filtered(&format!("{} {}", seed.artist, seed.title), "songs")?;
        let tracks: Vec<Track> = serde_json::from_value(result["tracks"].clone())?;
        crate::spotify::ranked_matches(seed, &tracks)
            .into_iter()
            .find_map(|candidate| crate::youtube_stream::video_id(&candidate.source))
            .ok_or_else(|| anyhow::anyhow!("No matching seed recording"))?
    };
    let radio = music.radio(&video, 24, None)?;
    let tracks = serde_json::from_value::<Vec<Track>>(radio["tracks"].clone())?;
    let name = if seed.artist.trim().is_empty() {
        &seed.title
    } else {
        &seed.artist
    };
    Ok(Mix {
        name: format!("{name} Mix"),
        tracks,
    })
}

pub(crate) fn picks(mixes: &[Mix]) -> Vec<&Track> {
    let mut seen = HashSet::new();
    (0..24)
        .flat_map(|index| mixes.iter().filter_map(move |mix| mix.tracks.get(index)))
        .filter(|track| seen.insert(track.id.as_str()))
        .take(12)
        .collect()
}

pub(crate) fn validate_tracks(value: &Value) -> Result<Vec<Track>> {
    let tracks: Vec<Track> = serde_json::from_value(value.clone())?;
    ensure!(
        (1..=24).contains(&tracks.len()),
        "Mix must contain 1–24 tracks"
    );
    ensure!(
        tracks.iter().all(|track| !track.id.is_empty()
            && !track.title.is_empty()
            && track.duration.is_finite()
            && track.duration >= 0.
            && crate::youtube_stream::video_id(&track.source).is_some()),
        "Invalid mix track"
    );
    Ok(tracks)
}

pub(crate) fn account_mixes(
    music: &Music<'_>,
    token: Option<&str>,
    local: Vec<Track>,
) -> Result<Value> {
    let mut likes = Vec::new();
    let mut playlist_tracks = Vec::new();
    let mut generated = Vec::new();
    let mut playlists = Vec::new();
    let mut warnings = Vec::new();
    if let Some(token) = token {
        match oauth_preferences(music.client, token) {
            Ok((liked, saved, lists)) => {
                likes = liked;
                playlist_tracks = saved;
                playlists = lists;
            }
            Err(_) => warnings.push("Could not read YouTube account likes or playlists."),
        }
    }
    if music.cookies.is_some() {
        match music.home() {
            Ok(home) if home["authenticated_session"] == true => {
                if let Ok(liked) = music.browse("VLLM", None) {
                    likes.extend(tracks_in(&liked));
                }
                if let Ok(saved) = music.browse("FEmusic_liked_playlists", None) {
                    for item in entries(&saved, "items").take(6) {
                        if let Some(id) = item["id"].as_str().filter(|id| id.starts_with("VL"))
                            && let Ok(page) = music.browse(id, item["params"].as_str())
                        {
                            playlist_tracks.extend(tracks_in(&page).into_iter().take(12));
                        }
                    }
                }
                for item in entries(&home, "generated_mixes").take(3) {
                    if let Some(id) = item["id"].as_str()
                        && let Ok(tracks) =
                            music.generated_mix(id, item["video"].as_str(), item["params"].as_str())
                    {
                        let title = item["title"]
                            .as_str()
                            .filter(|title| !title.is_empty())
                            .unwrap_or("YouTube Mix");
                        generated.push(Mix {
                            name: title.into(),
                            tracks,
                        });
                    }
                }
            }
            _ => warnings.push("The YouTube Music web session could not be verified."),
        }
    }
    let generated_tracks: Vec<Track> = generated
        .iter()
        .flat_map(|mix| mix.tracks.iter().take(2).cloned())
        .collect();
    let seeds = account_seeds(&likes, &playlist_tracks, &generated_tracks, &local);
    let mut mixes = match generate(music, &seeds) {
        Ok(mixes) => mixes,
        Err(error) if generated.is_empty() => return Err(error),
        Err(_) => Vec::new(),
    };
    crate::jobs::check_cancelled()?;
    let generated_count = generated.len();
    mixes.extend(generated);
    Ok(serde_json::json!({
        "picks":picks(&mixes), "mixes":mixes, "playlists":playlists, "youtube_oauth":token.is_some(),
        "youtube_signal":{"likes": likes.len(),"playlist_tracks":playlist_tracks.len(),"generated_mixes":generated_count},
        "warnings":warnings
    }))
}

fn entries<'a>(value: &'a Value, field: &str) -> impl Iterator<Item = &'a Value> {
    value[field].as_array().into_iter().flatten()
}

fn tracks_in(value: &Value) -> Vec<Track> {
    serde_json::from_value(value["tracks"].clone()).unwrap_or_default()
}

fn account_seeds(
    likes: &[Track],
    playlists: &[Track],
    mixes: &[Track],
    local: &[Track],
) -> Vec<Track> {
    let mut seen = HashSet::new();
    let mut artists = HashSet::new();
    let mut seeds = Vec::new();
    let groups = [likes, playlists, mixes, local];
    for index in 0..groups.iter().map(|group| group.len()).max().unwrap_or(0) {
        for track in groups.iter().filter_map(|group| group.get(index)) {
            let artist = track.artist.trim().to_lowercase();
            if !seen.contains(&track.id) && (artist.is_empty() || !artists.contains(&artist)) {
                seen.insert(track.id.clone());
                artists.insert(artist);
                seeds.push(track.clone());
                if seeds.len() == 3 {
                    return seeds;
                }
            }
        }
    }
    seeds
}

fn oauth_preferences(
    client: &reqwest::blocking::Client,
    token: &str,
) -> Result<(Vec<Track>, Vec<Track>, Vec<Value>)> {
    use crate::oauth::{map_youtube_tracks, youtube_library_pages};
    use serde_json::json;
    let likes = youtube_library_pages(client, token, "likes", None, 2);
    let lists = youtube_library_pages(client, token, "playlists", None, 2);
    if likes.is_err() && lists.is_err() {
        anyhow::bail!("YouTube account library unavailable");
    }
    let likes = likes.unwrap_or_default();
    let liked_items: Vec<_> = entries(&likes, "items")
        .filter(|item| item["snippet"]["categoryId"] == "10")
        .collect();
    let liked = map_youtube_tracks(&json!({"items":liked_items}));
    let lists = lists.unwrap_or_default();
    let mut playlist_ids = Vec::new();
    for id in entries(&lists, "items")
        .filter_map(|item| item["id"].as_str())
        .take(6)
    {
        crate::jobs::check_cancelled()?;
        if let Ok(page) = youtube_library_pages(client, token, "tracks", Some(id), 1) {
            let ids: Vec<_> = map_youtube_tracks(&page)
                .into_iter()
                .take(12)
                .filter_map(|track| crate::youtube_stream::video_id(&track.source))
                .collect();
            playlist_ids.push(ids);
        }
    }
    let mut seen = HashSet::new();
    let ids: Vec<_> = (0..12)
        .flat_map(|i| playlist_ids.iter().filter_map(move |list| list.get(i)))
        .filter(|id| seen.insert(id.as_str()))
        .collect();
    let mut metadata = HashMap::new();
    for chunk in ids.chunks(50) {
        crate::jobs::check_cancelled()?;
        let joined = chunk
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let response = client
            .get("https://www.googleapis.com/youtube/v3/videos")
            .bearer_auth(token)
            .timeout(std::time::Duration::from_secs(6))
            .query(&[("part", "snippet,contentDetails"), ("id", joined.as_str())])
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(reqwest::blocking::Response::json::<Value>);
        if let Ok(response) = response {
            let items: Vec<_> = entries(&response, "items")
                .filter(|item| item["snippet"]["categoryId"] == "10")
                .collect();
            for track in map_youtube_tracks(&json!({"items":items})) {
                metadata.insert(track.id.clone(), track);
            }
        }
    }
    let tracks = ids
        .into_iter()
        .filter_map(|id| metadata.remove(&format!("youtube:{id}")))
        .collect();
    Ok((liked, tracks, entries(&lists, "items").cloned().collect()))
}

#[cfg(test)]
mod account_tests {
    use super::*;
    use serde_json::json;

    fn track(id: &str, artist: &str) -> Track {
        serde_json::from_value(
            json!({"id":id,"title":id,"artist":artist,"source":"fixture","platform":"youtube"}),
        )
        .unwrap()
    }

    #[test]
    fn account_seed_groups_deduplicate_and_prioritize_distinct_artists() {
        let liked = track("liked", "A");
        let duplicate_artist = track("same-artist", "a");
        let playlist = track("playlist", "B");
        let generated = track("generated", "C");
        let result = account_seeds(
            std::slice::from_ref(&liked),
            &[duplicate_artist, playlist.clone()],
            std::slice::from_ref(&generated),
            std::slice::from_ref(&liked),
        );
        assert_eq!(result, vec![liked, generated, playlist]);
    }
}
