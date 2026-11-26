use crate::{
    model::{Library, Track},
    youtube::Music,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Mix {
    pub name: String,
    pub tracks: Vec<Track>,
    /// The song this mix was built around, when there is one.
    #[serde(default)]
    pub seed: String,
}

/// What the listening history says about this person: who to seed mixes from, which artists they
/// favour, and what they just heard. Skips (a track cut off by the next one within half a minute) count
/// against a track, repeats and recent plays count for it, and old plays fade with a three-week half-life.
#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct Taste {
    pub seeds: Vec<Track>,
    /// Your most-played and most-loved songs right now, for the "On repeat" mix.
    pub rotation: Vec<Track>,
    pub affinity: HashMap<String, f32>,
    pub recent: HashSet<String>,
    pub favorites: HashSet<String>,
}

const SEEDS: usize = 6;

fn artist_key(track: &Track) -> String {
    let name = track.artist.trim().to_lowercase();
    if name.is_empty() { track.id.clone() } else { name }
}

pub(crate) fn taste(library: &Library, now: u64) -> Taste {
    let mut scores = HashMap::<&str, f32>::new();
    let mut recent = HashSet::new();
    let newest_first: Vec<_> = library.history.iter().rev().take(600).collect();
    for (i, event) in newest_first.iter().enumerate() {
        let age_days = now.saturating_sub(event.started_at) as f32 / 86_400.;
        if age_days > 120. {
            continue;
        }
        let recency = 0.5_f32.powf(age_days / 21.);
        let skipped = i > 0 && newest_first[i - 1].started_at.saturating_sub(event.started_at) < 30;
        *scores.entry(&event.track_id).or_default() += if skipped { -0.6 * recency } else { recency };
        if age_days < 3. && !skipped {
            recent.insert(event.track_id.clone());
        }
    }
    let favorites: HashSet<String> = library.favorites.iter().cloned().collect();
    for id in &library.favorites {
        // A heart is the clearest signal there is: worth several plays, and it does not fade.
        *scores.entry(id).or_default() += 5.;
    }
    let mut affinity = HashMap::<String, f32>::new();
    let mut ranked: Vec<(f32, &Track)> = scores
        .into_iter()
        .filter_map(|(id, score)| {
            let track = library.tracks.get(id)?;
            if score > 0. {
                *affinity.entry(artist_key(track)).or_default() += score;
            }
            (score > 0.
                && matches!(
                    track.platform.as_str(),
                    "youtube" | "ytmusic" | "spotify" | "catalog"
                ))
            .then_some((score, track))
        })
        .collect();
    ranked.sort_by(|(a_score, a), (b_score, b)| {
        b_score.total_cmp(a_score).then_with(|| a.id.cmp(&b.id))
    });
    let rotation: Vec<Track> = ranked
        .iter()
        .filter(|(_, track)| crate::youtube_stream::video_id(&track.source).is_some())
        .take(24)
        .map(|(_, track)| (*track).clone())
        .collect();
    let mut artists = HashSet::new();
    let seeds = ranked
        .into_iter()
        .filter(|(_, track)| artists.insert(artist_key(track)))
        .take(SEEDS)
        .map(|(_, track)| track.clone())
        .collect();
    let top = affinity.values().copied().fold(0., f32::max).max(1.);
    for value in affinity.values_mut() {
        *value /= top;
    }
    Taste { seeds, rotation, affinity, recent, favorites }
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
                    build_with(music, seed, 40)
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
    build_with(music, seed, 24)
}

fn build_with(music: &Music<'_>, seed: &Track, size: usize) -> Result<Mix> {
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
    let radio = music.radio(&video, size, None)?;
    let tracks = serde_json::from_value::<Vec<Track>>(radio["tracks"].clone())?;
    let name = if seed.artist.trim().is_empty() {
        &seed.title
    } else {
        &seed.artist
    };
    Ok(Mix {
        name: format!("{name} Mix"),
        tracks,
        seed: seed.id.clone(),
    })
}

pub(crate) fn picks(mixes: &[Mix]) -> Vec<&Track> {
    if let Some(first) = mixes.iter().find(|mix| mix.name == FOR_YOU) {
        return first.tracks.iter().take(12).collect();
    }
    let mut seen = HashSet::new();
    (0..24)
        .flat_map(|index| mixes.iter().filter_map(move |mix| mix.tracks.get(index)))
        .filter(|track| seen.insert(track.id.as_str()))
        .take(12)
        .collect()
}

pub const FOR_YOU: &str = "Made for you";
pub const DISCOVER: &str = "Discover";
pub const ON_REPEAT: &str = "On repeat";

/// Every radio votes: a song that several of your seeds lead to ranks above one that only one of them
/// does. Artists you play a lot get a boost, what you heard in the last days is held back, and
/// "Discover" keeps only artists you have not been listening to.
pub(crate) fn blend(mixes: &[Mix], taste: &Taste) -> Vec<Mix> {
    let mut scores = HashMap::<String, (f32, &Track)>::new();
    for (rank, mix) in mixes.iter().enumerate() {
        let weight = 0.85_f32.powi(rank as i32);
        for (position, track) in mix.tracks.iter().enumerate() {
            let vote = weight / (1. + position as f32 / 5.);
            scores
                .entry(track.id.clone())
                .and_modify(|(score, _)| *score += vote)
                .or_insert((vote, track));
        }
    }
    let mut ranked: Vec<(f32, &Track, bool)> = scores
        .into_values()
        .map(|(score, track)| {
            let liking = taste.affinity.get(&artist_key(track)).copied();
            let mut score = score * (1. + 0.6 * liking.unwrap_or(0.));
            if taste.recent.contains(&track.id) {
                score *= 0.25;
            }
            if taste.favorites.contains(&track.id) {
                score *= 0.6;
            }
            (score, track, liking.is_none())
        })
        .collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
    let take = |new_only: bool| -> Vec<Track> {
        let eligible: Vec<usize> = (0..ranked.len())
            .filter(|&i| !new_only || ranked[i].2)
            .collect();
        let mut per_artist = HashMap::<String, usize>::new();
        let mut chosen: Vec<usize> = Vec::new();
        let mut add = |i: usize, chosen: &mut Vec<usize>| {
            let seen = per_artist.entry(artist_key(ranked[i].1)).or_default();
            if *seen < 3 && !chosen.contains(&i) {
                *seen += 1;
                chosen.push(i);
            }
        };
        if !new_only {
            // Give every seed a say first (its three best-ranked songs), so a second taste, say
            // Vietnamese pop next to English indie, is not crowded out by the louder one.
            for mix in mixes {
                let ids: HashSet<&str> = mix.tracks.iter().map(|t| t.id.as_str()).collect();
                eligible
                    .iter()
                    .filter(|&&i| ids.contains(ranked[i].1.id.as_str()))
                    .take(3)
                    .for_each(|&i| add(i, &mut chosen));
            }
        }
        for &i in &eligible {
            if chosen.len() >= 24 {
                break;
            }
            add(i, &mut chosen);
        }
        chosen.sort_unstable();
        chosen.truncate(24);
        chosen.into_iter().map(|i| ranked[i].1.clone()).collect()
    };
    [(FOR_YOU, take(false)), (DISCOVER, take(true))]
        .into_iter()
        .filter(|(_, tracks)| tracks.len() >= 6)
        .map(|(name, tracks)| Mix { name: name.into(), tracks, ..Default::default() })
        .collect()
}

pub(crate) fn validate_tracks(value: &Value) -> Result<Vec<Track>> {
    let tracks: Vec<Track> = serde_json::from_value(value.clone())?;
    ensure!(
        (1..=60).contains(&tracks.len()),
        "Mix must contain 1–60 tracks"
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

/// "Made for you", "Discover", "On repeat" and the recommended picks, built from a library (favourites and
/// history) alone: what the phone app calls, since it has no YouTube account session.
pub fn for_library(music: &Music<'_>, library: &Library, now: u64) -> Result<Value> {
    account_mixes(music, None, &taste(library, now))
}

pub(crate) fn account_mixes(
    music: &Music<'_>,
    token: Option<&str>,
    taste: &Taste,
) -> Result<Value> {
    let local = taste.seeds.clone();
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
                            ..Default::default()
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
    // Radios built around a song you hearted read "Because you liked …", like YouTube's own.
    for mix in &mut mixes {
        if taste.favorites.contains(&mix.seed)
            && let Some(seed) = taste.seeds.iter().find(|t| t.id == mix.seed)
        {
            mix.name = format!("Because you liked {}", seed.title);
        }
    }
    let mut blended = blend(&mixes, taste);
    if taste.rotation.len() >= 8 {
        let at = blended.iter().position(|m| m.name == DISCOVER).unwrap_or(blended.len());
        blended.insert(
            at,
            Mix { name: ON_REPEAT.into(), tracks: taste.rotation.clone(), ..Default::default() },
        );
    }
    mixes.extend(generated);
    mixes.splice(0..0, blended);
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
                if seeds.len() == SEEDS {
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

#[cfg(test)]
mod taste_tests {
    use super::*;
    use crate::model::PlayEvent;
    use serde_json::json;

    fn track(id: &str, artist: &str) -> Track {
        serde_json::from_value(
            json!({"id":id,"title":id,"artist":artist,"source":"fixture","platform":"youtube"}),
        )
        .unwrap()
    }

    fn library(plays: &[(&str, &str, u64)], favorites: &[&str]) -> Library {
        let mut library = Library::default();
        for (id, artist, at) in plays {
            library.tracks.insert((*id).into(), track(id, artist));
            library.history.push(PlayEvent { track_id: (*id).into(), started_at: *at });
        }
        library.favorites = favorites.iter().map(|id| (*id).to_owned()).collect();
        library
    }

    const NOW: u64 = 10_000_000;

    #[test]
    fn a_track_cut_off_by_the_next_one_is_a_skip_and_not_a_seed() {
        // "skipped" is followed by "kept" 8 seconds later; "kept" plays on.
        let library = library(
            &[("skipped", "A", NOW - 3600), ("kept", "B", NOW - 3592), ("later", "C", NOW - 600)],
            &[],
        );
        let ids: Vec<_> = taste(&library, NOW).seeds.into_iter().map(|t| t.id).collect();
        assert!(!ids.contains(&"skipped".to_owned()), "{ids:?}");
        assert!(ids.contains(&"kept".to_owned()));
    }

    #[test]
    fn recent_plays_and_old_plays_weigh_differently() {
        let library = library(&[("old", "A", NOW - 100 * 86_400), ("fresh", "B", NOW - 3600)], &[]);
        let taste = taste(&library, NOW);
        assert_eq!(taste.seeds[0].id, "fresh");
        assert!(taste.affinity["b"] > taste.affinity["a"]);
        assert!(taste.recent.contains("fresh") && !taste.recent.contains("old"));
    }

    #[test]
    fn songs_several_seeds_lead_to_rank_first_and_discover_skips_known_artists() {
        let mix = |name: &str, ids: &[(&str, &str)]| Mix {
            name: name.into(),
            tracks: ids.iter().map(|(id, artist)| track(id, artist)).collect(),
            ..Default::default()
        };
        let shared = ("shared", "Newcomer");
        let m1 = mix("one", &[shared, ("a1", "Known"), ("a2", "Known"), ("a3", "Fresh1"), ("a4", "Fresh2"), ("a5", "Fresh3")]);
        let m2 = mix("two", &[shared, ("b1", "Fresh4"), ("b2", "Fresh5"), ("b3", "Fresh6"), ("b4", "Fresh7"), ("b5", "Fresh8")]);
        let mut taste = Taste::default();
        taste.affinity.insert("known".into(), 1.);
        let blended = blend(&[m1, m2], &taste);
        let for_you = blended.iter().find(|m| m.name == FOR_YOU).unwrap();
        assert_eq!(for_you.tracks[0].id, "shared", "the song both radios lead to goes first");
        let discover = blended.iter().find(|m| m.name == DISCOVER).unwrap();
        assert!(discover.tracks.iter().all(|t| t.artist != "Known"));
        assert!(discover.tracks.len() <= 24 && for_you.tracks.len() <= 24);
    }

    #[test]
    fn a_hearted_song_outweighs_several_plays_of_another() {
        let library = library(
            &[
                ("loved", "A", NOW - 20 * 86_400),
                ("played", "B", NOW - 6 * 3600),
                ("played", "B", NOW - 4 * 3600),
                ("played", "B", NOW - 7200),
            ],
            &["loved"],
        );
        let taste = taste(&library, NOW);
        assert_eq!(taste.seeds[0].id, "loved", "{:?}", taste.seeds.iter().map(|t| &t.id).collect::<Vec<_>>());
        assert!(taste.affinity["a"] > taste.affinity["b"]);
    }
}
