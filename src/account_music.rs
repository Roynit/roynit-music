use crate::{
    model::{Library, PlayEvent, Playlist, Track},
    provider,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub tracks: BTreeMap<String, Track>,
    pub playlists: BTreeMap<String, Playlist>,
    pub favorites: Vec<String>,
    #[serde(default)]
    pub dislikes: Vec<String>,
    pub history: Vec<PlayEvent>,
}

impl Snapshot {
    fn online(track: &Track) -> bool {
        let Ok(url) = url::Url::parse(&track.source) else {
            return false;
        };
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
        {
            return false;
        }
        match track.platform.as_str() {
            "spotify" => {
                url.host_str() == Some("open.spotify.com") && url.path().starts_with("/track/")
            }
            "youtube" | "ytmusic" | "soundcloud" => provider::validate_url(&track.source).is_ok(),
            _ => false,
        }
    }

    fn portable(mut track: Track) -> Track {
        track.local_path = None;
        track.cache_path = None;
        if let Ok(mut url) = url::Url::parse(&track.source) {
            let video = url
                .query_pairs()
                .find(|(key, _)| key == "v")
                .map(|(_, value)| value.into_owned());
            url.set_query(None);
            url.set_fragment(None);
            if matches!(track.platform.as_str(), "youtube" | "ytmusic")
                && let Some(video) = video
            {
                url.query_pairs_mut().append_pair("v", &video);
            }
            track.source = url.into();
        }
        track.cover = track.cover.filter(|address| {
            let Ok(url) = url::Url::parse(address) else {
                return false;
            };
            url.scheme() == "https"
                && url.query().is_none()
                && url.port().is_none()
                && url.username().is_empty()
                && url.password().is_none()
                && url.host_str().is_some_and(|host| {
                    [
                        "ytimg.com",
                        "googleusercontent.com",
                        "scdn.co",
                        "sndcdn.com",
                    ]
                    .iter()
                    .any(|domain| {
                        host == *domain
                            || host
                                .strip_suffix(domain)
                                .is_some_and(|prefix| prefix.ends_with('.'))
                    })
                })
        });
        track
    }

    pub fn from_library(library: &Library) -> Self {
        let history: Vec<_> = library
            .history
            .iter()
            .rev()
            .filter(|event| {
                library
                    .tracks
                    .get(&event.track_id)
                    .is_some_and(Self::online)
            })
            .take(1600)
            .cloned()
            .collect();
        let mut snapshot = Self {
            favorites: library.favorites.clone(),
            dislikes: library.dislikes.clone(),
            playlists: library.playlists.clone(),
            history: history.into_iter().rev().collect(),
            ..Self::default()
        };
        for id in snapshot
            .favorites
            .iter()
            .chain(
                snapshot
                    .playlists
                    .values()
                    .flat_map(|playlist| &playlist.tracks),
            )
            .chain(snapshot.history.iter().map(|event| &event.track_id))
            .chain(&snapshot.dislikes)
        {
            if let Some(track) = library.tracks.get(id).filter(|track| Self::online(track)) {
                snapshot
                    .tracks
                    .entry(id.to_owned())
                    .or_insert_with(|| Self::portable(track.clone()));
            }
        }
        snapshot.retain_references();
        snapshot
    }

    fn retain_references(&mut self) {
        self.favorites.retain(|id| self.tracks.contains_key(id));
        self.dislikes.retain(|id| self.tracks.contains_key(id));
        for playlist in self.playlists.values_mut() {
            playlist.tracks.retain(|id| self.tracks.contains_key(id));
        }
        self.history
            .retain(|event| self.tracks.contains_key(&event.track_id));
    }

    pub fn merge_into(mut self, library: &mut Library) {
        self.tracks
            .retain(|id, track| id == &track.id && Self::online(track));
        self.retain_references();
        for (id, track) in self.tracks {
            library
                .tracks
                .entry(id)
                .or_insert_with(|| Self::portable(track));
        }
        for (id, playlist) in self.playlists {
            if id == playlist.id {
                let version = playlist.cover;
                let local = library.playlists.entry(id).or_insert(playlist);
                // The newest change to the picture wins, whichever device made it.
                if version.abs() > local.cover.abs() {
                    local.cover = version;
                }
            }
        }
        let mut favorites: HashSet<_> = library.favorites.iter().cloned().collect();
        library.favorites.extend(
            self.favorites
                .into_iter()
                .filter(|id| favorites.insert(id.clone())),
        );
        let mut disliked: HashSet<_> = library.dislikes.iter().cloned().collect();
        library.dislikes.extend(
            self.dislikes
                .into_iter()
                .filter(|id| disliked.insert(id.clone())),
        );
        library.favorites.retain(|id| !disliked.contains(id));
        let mut seen: HashSet<_> = library
            .history
            .iter()
            .map(|event| (event.track_id.clone(), event.started_at))
            .collect();
        library.history.extend(
            self.history
                .into_iter()
                .filter(|event| seen.insert((event.track_id.clone(), event.started_at))),
        );
        library.history.sort_by_key(|event| event.started_at);
    }
}
