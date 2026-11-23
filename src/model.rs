use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Track {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub duration: f64,
    pub source: String,
    #[serde(default = "local")]
    pub platform: String,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub local_path: Option<PathBuf>,
    #[serde(default)]
    pub cache_path: Option<PathBuf>,
}
fn local() -> String {
    "local".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub tracks: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Queue {
    pub tracks: Vec<String>,
    pub index: Option<usize>,
    pub repeat: Repeat,
    pub shuffle: bool,
    #[serde(default)]
    pub back_stack: Vec<usize>,
}

impl Queue {
    pub fn current(&self) -> Option<&str> {
        self.index
            .and_then(|i| self.tracks.get(i))
            .map(String::as_str)
    }
    pub fn advance(&mut self, automatic: bool) -> Option<&str> {
        if self.tracks.is_empty() {
            self.index = None;
            return None;
        }
        if automatic && self.repeat == Repeat::One && self.index.is_some() {
            return self.current();
        }
        let next = match self.index {
            None => 0,
            Some(i) if self.shuffle && self.tracks.len() > 1 => {
                let n = rand::random_range(0..self.tracks.len() - 1);
                if n >= i { n + 1 } else { n }
            }
            Some(i) if i + 1 < self.tracks.len() => i + 1,
            Some(_) if self.repeat == Repeat::All => 0,
            _ => {
                self.index = None;
                return None;
            }
        };
        if let Some(i) = self.index {
            self.back_stack.push(i);
        }
        self.index = Some(next);
        self.current()
    }
    pub fn previous(&mut self) -> Option<&str> {
        self.index = self
            .back_stack
            .pop()
            .or_else(|| self.index.map(|i| i.saturating_sub(1)));
        self.current()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Effects {
    pub volume: f32,
    pub muted: bool,
    pub speed: f64,
    pub speed_pitch: f64,
    pub pitch: f64,
    pub eq_bands: [f64; 10],
    pub bass: f64,
    pub mid: f64,
    pub treble: f64,
    pub reverb: f64,
    pub echo: f64,
    pub distortion: f64,
    pub normalize: bool,
    pub crossfade_seconds: f64,
    pub hq: HqEffects,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HqEffects {
    pub enabled: bool,
    pub bass: f64,
    pub vocal: f64,
    pub air: f64,
}
impl Default for Effects {
    fn default() -> Self {
        Self {
            volume: 0.85,
            muted: false,
            speed: 1.,
            speed_pitch: 1.,
            pitch: 0.,
            eq_bands: [0.; 10],
            bass: 0.,
            mid: 0.,
            treble: 0.,
            reverb: 0.,
            echo: 0.,
            distortion: 0.,
            normalize: false,
            crossfade_seconds: 0.,
            hq: HqEffects::default(),
        }
    }
}
impl Effects {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.volume.is_finite() && (0.0..=1.0).contains(&self.volume),
            "volume must be 0..1"
        );
        anyhow::ensure!((0.25..=4.).contains(&self.speed), "speed must be 0.25..4");
        anyhow::ensure!(
            (0.25..=4.).contains(&self.speed_pitch),
            "speed_pitch must be 0.25..4"
        );
        anyhow::ensure!(
            [self.hq.bass, self.hq.vocal, self.hq.air]
                .iter()
                .all(|x| (-24.0..=24.).contains(x)),
            "HQ gains must be -24..24 dB"
        );
        anyhow::ensure!(
            (-24.0..=24.).contains(&self.pitch),
            "pitch must be -24..24 semitones"
        );
        anyhow::ensure!(
            self.eq_bands
                .iter()
                .chain([&self.bass, &self.mid, &self.treble])
                .all(|x| (-24.0..=24.).contains(x)),
            "EQ gain must be -24..24 dB"
        );
        anyhow::ensure!(
            [self.echo, self.reverb, self.distortion]
                .iter()
                .all(|x| (0.0..=100.).contains(x)),
            "effects must be 0..100"
        );
        anyhow::ensure!(
            (0.0..=12.).contains(&self.crossfade_seconds),
            "crossfade must be 0..12 seconds"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayEvent {
    pub track_id: String,
    pub started_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub schema_version: u32,
    pub tracks: BTreeMap<String, Track>,
    pub playlists: BTreeMap<String, Playlist>,
    /// The order playlists are shown in; ones missing here (new or imported) follow, sorted by name.
    pub playlist_order: Vec<String>,
    pub favorites: Vec<String>,
    pub history: Vec<PlayEvent>,
    pub queue: Queue,
    pub effects: Effects,
    pub settings: BTreeMap<String, Value>,
}
impl Library {
    /// Playlists in the order the listener arranged them, then any others by name.
    pub fn ordered_playlists(&self) -> Vec<&Playlist> {
        let mut seen = std::collections::HashSet::new();
        let mut list: Vec<&Playlist> = self
            .playlist_order
            .iter()
            .filter_map(|id| self.playlists.get(id))
            .filter(|p| seen.insert(p.id.as_str()))
            .collect();
        let mut rest: Vec<&Playlist> = self
            .playlists
            .values()
            .filter(|p| !seen.contains(p.id.as_str()))
            .collect();
        rest.sort_by_cached_key(|p| p.name.to_lowercase());
        list.extend(rest);
        list
    }
}
impl Default for Library {
    fn default() -> Self {
        Self {
            schema_version: 1,
            tracks: BTreeMap::new(),
            playlists: BTreeMap::new(),
            playlist_order: vec![],
            favorites: vec![],
            history: vec![],
            queue: Queue::default(),
            effects: Effects::default(),
            settings: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod order_tests {
    use super::*;

    fn library(names: &[(&str, &str)], order: &[&str]) -> Library {
        let mut library = Library::default();
        for (id, name) in names {
            library.playlists.insert(
                (*id).into(),
                Playlist { id: (*id).into(), name: (*name).into(), tracks: vec![] },
            );
        }
        library.playlist_order = order.iter().map(|id| (*id).to_owned()).collect();
        library
    }

    #[test]
    fn playlists_follow_the_chosen_order_then_new_ones_by_name() {
        let library = library(&[("a", "Zed"), ("b", "Beta"), ("c", "Alpha"), ("d", "Delta")], &["b", "a"]);
        let ids: Vec<_> = library.ordered_playlists().iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["b", "a", "c", "d"], "chosen first, then Alpha, Delta");
    }

    #[test]
    fn unknown_and_repeated_ids_in_the_order_are_ignored() {
        let library = library(&[("a", "A"), ("b", "B")], &["gone", "b", "b", "a"]);
        let ids: Vec<_> = library.ordered_playlists().iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["b", "a"]);
    }
}
