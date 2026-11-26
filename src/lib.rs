pub mod account_music;
pub mod appearance;
pub mod cache;
pub mod catalog;
pub mod discord;
pub mod extensions;
pub mod imports;
pub mod installation;
pub mod instance;
pub mod integrations;
pub mod jobs;
pub mod lyric_providers;
pub mod lyrics;
pub mod media;
#[cfg(feature = "desktop")]
pub mod media_keys;
pub mod mixes;
pub mod model;
pub mod network;
pub mod oauth;
pub mod payload;
pub mod player;
pub mod plugins;
pub mod prerequisites;
mod process_guard;
pub mod provider;
mod search_cache;
pub mod services;
pub mod socks_relay;
pub mod spotify;
pub mod spotify_public;
pub mod startup;
pub mod store;
pub mod tools;
pub mod translation;
pub mod updates;
pub mod vault;
pub mod youtube;
pub mod sources;
pub mod youtube_stream;

use anyhow::{Context, Result, ensure};
use model::{Effects, PlayEvent, Playlist, Repeat, Track};
use player::Player;
use provider::Providers;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn stream_headers(value: &Value) -> Vec<(String, String)> {
    value["headers"]
        .as_object()
        .map(|headers| {
            headers
                .iter()
                .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.into())))
                .collect()
        })
        .unwrap_or_default()
}

struct AutoplayRequest {
    smart: bool,
    job: String,
    queue: Vec<String>,
}

pub struct Engine {
    pub store: store::Store,
    pub player: Player,
    pub providers: Providers,
    client: network::Client,
    search_cache: std::sync::Arc<search_cache::SearchCache>,
    oauth: oauth::OAuth,
    jobs: jobs::Jobs,
    presence: discord::Presence,
    events: std::collections::VecDeque<Value>,
    sleep_deadline: Option<std::time::Instant>,
    radio: Option<RadioSession>,
    region_end: Option<f64>,
    pending_play: Option<PendingPlayback>,
    warmed_next: Option<(String, bool)>,
    autoplay_attempted_tail: Option<String>,
    autoplay: Option<AutoplayRequest>,
    smart_pool: Vec<Track>,
    smart_picks: std::collections::HashSet<String>,
    next_audio_job: Option<String>,
    preview_audio_job: Option<(String, String)>,
    recovery_attempts: u8,
    checkpoint: std::time::Instant,
    cache_checkpoint: std::time::Instant,
    track_revision: u64,
    plugin_revision: u64,
    integrations: integrations::Integrations,
    discord_update: std::time::Instant,
    discord_signature: String,
}
#[derive(Clone)]
struct QueueCursor {
    index: Option<usize>,
    back_stack: Vec<usize>,
}
struct PendingPlayback {
    job: String,
    id: String,
    select_queue: bool,
    position: f64,
    cursor_before: Option<QueueCursor>,
    record_history: bool,
    paused: bool,
}
struct RadioSession {
    seed: String,
    generation: String,
    continuation: Option<String>,
    fetching: bool,
    retry_at: std::time::Instant,
}
/// The Discord application whose name and icon ("Roy Music") show in the Listening card.
pub const DISCORD_APP_ID: &str = "1574391018894131230";
/// The earlier application, named "Riff"; settings that still point at it are moved to the new one.
const OLD_DISCORD_APP_ID: &str = "1520100510877749358";
impl Engine {
    pub fn open(root: PathBuf) -> Result<Self> {
        let mut store = store::Store::open(root)?;
        if let Some(discord) = store.data.settings.get_mut("discord")
            && discord["application_id"] == OLD_DISCORD_APP_ID
        {
            discord["application_id"] = json!(DISCORD_APP_ID);
        }
        store
            .data
            .settings
            .entry("release_repo".into())
            .or_insert_with(|| json!(updates::DEFAULT_REPO));
        // Keep existing IDs/playlist references stable when repairing legacy provider tags.
        for track in store.data.tracks.values_mut() {
            if url::Url::parse(&track.source)
                .ok()
                .and_then(|u| u.host_str().map(str::to_owned))
                .is_some_and(|h| h == "soundcloud.com" || h.ends_with(".soundcloud.com"))
            {
                track.platform = "soundcloud".into();
            }
        }
        let cookies = store.root.join("cookies.txt");
        let cookies = store
            .data
            .settings
            .get("youtube_cookie_path")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .unwrap_or(cookies);
        let mut oauth = oauth::OAuth::default();
        let integrations = integrations::Integrations::open(store.root.clone());
        let mut events = std::collections::VecDeque::new();
        match vault::load(&store.root.join("oauth.dpapi")) {
            Ok(Some(value)) => {
                if let Err(e) = oauth.restore(value) {
                    events.push_back(json!({"event":"auth_restore_error","error":e.to_string()}));
                }
            }
            Err(e) => events.push_back(json!({"event":"auth_restore_error","error":e.to_string()})),
            _ => {}
        }
        let network = network::Config::load(&store.root)?;
        let client = network.client()?;
        let mut player = Player::default();
        player.proxy = client.proxy.clone();
        if let Some(name) = store
            .data
            .settings
            .get("audio_device")
            .and_then(Value::as_str)
            && let Err(e) = player.select_device(Some(name))
        {
            events.push_back(json!({"event":"audio_device_error","error":e.to_string()}));
        }
        let browser = store
            .data
            .settings
            .get("cookie_browser")
            .and_then(Value::as_str)
            .map(str::to_owned);
        Ok(Self {
            store,
            player,
            providers: Providers {
                cookies: cookies.is_file().then_some(cookies),
                proxy: client.proxy.clone(),
                browser,
            },
            client,
            oauth,
            jobs: jobs::Jobs::default(),
            presence: discord::Presence::default(),
            events,
            sleep_deadline: None,
            radio: None,
            region_end: None,
            pending_play: None,
            warmed_next: None,
            autoplay_attempted_tail: None,
            autoplay: None,
            smart_pool: Vec::new(),
            smart_picks: Default::default(),
            next_audio_job: None,
            preview_audio_job: None,
            search_cache: Default::default(),
            recovery_attempts: 0,
            checkpoint: std::time::Instant::now(),
            cache_checkpoint: std::time::Instant::now(),
            track_revision: 0,
            plugin_revision: 0,
            integrations,
            discord_update: std::time::Instant::now() - Duration::from_secs(20),
            discord_signature: String::new(),
        })
    }
    pub fn default_root() -> Result<PathBuf> {
        Ok(
            directories::ProjectDirs::from("io.github", "rootscripts", "RiffRust")
                .context("Cannot find application data directory")?
                .data_local_dir()
                .to_path_buf(),
        )
    }
    fn autoplay_enabled(&self) -> bool {
        self.store.data.settings.get("autoplay") != Some(&json!(false))
            && self.radio.is_none()
            && self.store.data.queue.repeat == Repeat::Off
            && !self.store.data.queue.shuffle
    }

    fn smart_shuffle(&self) -> bool {
        self.store.data.queue.shuffle
            && self.store.data.settings.get("shuffle_mode") == Some(&json!("smart"))
    }

    fn advance_queue(&mut self, automatic: bool) -> Option<String> {
        let smart = self.smart_shuffle();
        let ordered = self.store.data.queue.shuffle
            && self.store.data.settings.get("shuffle_mode") == Some(&json!("ordered"));
        let queue = &mut self.store.data.queue;
        if (!smart && !ordered)
            || queue.index.is_none()
            || (automatic && queue.repeat == Repeat::One)
        {
            return queue.advance(automatic).map(str::to_owned);
        }
        let current = queue.index?;
        let mut seen: std::collections::HashSet<_> = queue.back_stack.iter().copied().collect();
        seen.insert(current);
        let artist = |index: usize| {
            queue
                .tracks
                .get(index)
                .and_then(|id| self.store.data.tracks.get(id))
                .map(|track| track.artist.trim().to_lowercase())
                .unwrap_or_default()
        };
        let current_artist = artist(current);
        let mut available: Vec<_> = (0..queue.tracks.len())
            .filter(|index| !seen.contains(index))
            .collect();
        let since_pick = std::iter::once(&current)
            .chain(queue.back_stack.iter().rev())
            .take_while(|index| {
                queue
                    .tracks
                    .get(**index)
                    .is_some_and(|id| !self.smart_picks.contains(id))
            })
            .count();
        if smart
            && queue.tracks.len() < 4096
            && (since_pick >= 3 || available.is_empty())
            && let Some(index) = self
                .smart_pool
                .iter()
                .position(|track| {
                    !queue.tracks.contains(&track.id)
                        && !track.artist.trim().eq_ignore_ascii_case(&current_artist)
                })
                .or_else(|| {
                    self.smart_pool
                        .iter()
                        .position(|track| !queue.tracks.contains(&track.id))
                })
        {
            let track = self.smart_pool.remove(index);
            self.smart_picks.insert(track.id.clone());
            queue.back_stack.push(current);
            queue.index = Some(queue.tracks.len());
            queue.tracks.push(track.id.clone());
            self.store
                .data
                .tracks
                .entry(track.id.clone())
                .or_insert(track);
            return queue.current().map(str::to_owned);
        }
        if available.is_empty() {
            if queue.repeat == Repeat::All {
                let mut visits = vec![0usize; queue.tracks.len()];
                for index in &queue.back_stack {
                    if let Some(count) = visits.get_mut(*index) {
                        *count += 1;
                    }
                }
                let least = visits
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != current)
                    .map(|(_, count)| *count)
                    .min();
                available = visits
                    .iter()
                    .enumerate()
                    .filter(|(i, count)| *i != current && Some(**count) == least)
                    .map(|(i, _)| i)
                    .collect();
                if available.is_empty() {
                    return queue.current().map(str::to_owned);
                }
            } else {
                queue.index = None;
                return None;
            }
        }
        let different: Vec<_> = available
            .iter()
            .copied()
            .filter(|index| artist(*index) != current_artist)
            .collect();
        if !different.is_empty() {
            available = different;
        }
        queue.back_stack.push(current);
        queue.index = Some(available[rand::random_range(0..available.len())]);
        queue.current().map(str::to_owned)
    }

    fn cancel_autoplay(&mut self) {
        if let Some(request) = self.autoplay.take() {
            let _ = self.jobs.cancel(&request.job);
        }
        self.autoplay_attempted_tail = None;
    }

    fn prefill_autoplay(&mut self) {
        let smart = self.smart_shuffle() && self.radio.is_none();
        if !smart && !self.autoplay_enabled() {
            self.cancel_autoplay();
            return;
        }
        let queue = &self.store.data.queue;
        let Some(index) = queue.index else { return };
        let Some(tail) = (if smart {
            queue.tracks.get(index)
        } else {
            queue.tracks.last()
        }) else {
            return;
        };
        if self.autoplay.is_some()
            || self.player.paused()
            || self.pending_play.is_some()
            || self.player.track_id.as_deref() != queue.current()
            || (!smart && queue.tracks.len().saturating_sub(index) > 3)
            || (smart && (self.smart_pool.len() >= 4 || queue.repeat == Repeat::One))
            || queue.tracks.len() >= 4096
            || self.autoplay_attempted_tail.as_deref() == Some(tail.as_str())
        {
            return;
        }
        let Some(seed) = self
            .store
            .data
            .tracks
            .get(tail)
            .filter(|track| {
                matches!(
                    track.platform.as_str(),
                    "youtube" | "ytmusic" | "catalog" | "spotify"
                )
            })
            .cloned()
        else {
            return;
        };
        let attempted = tail.clone();
        let snapshot = queue.tracks.clone();
        self.autoplay_attempted_tail = Some(attempted);
        if let Ok(result) =
            self.submit_job(&json!({"operation":"autoplay.fill","params":{"track":seed}}))
            && let Some(id) = result["job_id"].as_str()
        {
            self.autoplay = Some(AutoplayRequest {
                smart,
                job: id.into(),
                queue: snapshot,
            });
        }
    }

    pub fn tick(&mut self) -> Result<Option<Value>> {
        self.player.tick();
        let snapshot = self.player.snapshot();
        let track = self
            .player
            .track_id
            .as_ref()
            .and_then(|id| self.store.data.tracks.get(id));
        let messages = self.integrations.sample(track, &snapshot);
        for params in messages {
            if let Err(e) = self.submit_job(&json!({"operation":"lastfm.auto","params":params})) {
                self.integrations.last_error = Some(e.to_string());
            }
        }
        self.tick_discord();
        if self.checkpoint.elapsed() >= Duration::from_secs(15) && self.player.track_id.is_some() {
            self.persist_session()?;
            self.checkpoint = std::time::Instant::now();
        }
        if self
            .region_end
            .is_some_and(|end| self.player.position() >= end)
        {
            self.region_end = None;
            self.player.pause();
            return Ok(Some(
                json!({"event":"region_finished","player":self.player.snapshot()}),
            ));
        }
        self.poll_jobs();
        if self.track_revision != self.plugin_revision {
            self.plugin_revision = self.track_revision;
            if let Some(track) = self
                .player
                .track_id
                .as_ref()
                .and_then(|id| self.store.data.tracks.get(id))
                .cloned()
                && let Ok(plugins) = plugins::list(&self.store.root)
            {
                for plugin in plugins
                    .into_iter()
                    .filter(|p| p.enabled && p.hooks.iter().any(|h| h == "track_changed"))
                {
                    if let Err(error)=self.submit_job(&json!({"operation":"plugins.run","params":{"id":plugin.id,"params":{"event":"track_changed","track":track,"player":self.player.snapshot(),"queue":self.store.data.queue}}})){self.events.push_back(json!({"event":"plugin_error","error":error.to_string()}));}
                }
            }
        }
        if self.cache_checkpoint.elapsed() >= Duration::from_secs(60) {
            self.cache_checkpoint = std::time::Instant::now();
            if let Err(error) = self.prune_cache(self.cache_limit()) {
                self.events
                    .push_back(json!({"event":"cache_error","error":error.to_string()}));
            }
        }
        let next = self
            .store
            .data
            .queue
            .index
            .and_then(|i| self.store.data.queue.tracks.get(i + 1))
            .cloned();
        self.player.expire_prepared_audio();
        let near_end =
            self.player.duration > 0. && self.player.duration - self.player.position() <= 20.;
        let next = next
            .filter(|id| {
                self.player
                    .track_id
                    .as_ref()
                    .is_some_and(|current| current != id)
            })
            .map(|id| (id, near_end));
        if next != self.warmed_next && self.next_audio_job.is_none() {
            self.warmed_next = next.clone();
            if let Some((id, _)) = &next
                && let Ok(job) =
                    self.submit_job(&json!({"operation":"stream.prepare","params":{"id":id}}))
            {
                self.next_audio_job = job["job_id"].as_str().map(str::to_owned);
            }
        }
        self.prefill_autoplay();
        let refill=self.radio.as_ref().filter(|r|!r.fetching&&r.continuation.is_some()&&std::time::Instant::now()>=r.retry_at).filter(|_|self.store.data.queue.index.is_some_and(|i|i+5>=self.store.data.queue.tracks.len())).map(|r|json!({"operation":"recommendations.radio","params":{"seed":r.seed,"continuation":r.continuation,"generation":r.generation,"append_to_queue":true,"extend":true,"limit":50}}));
        if let Some(request) = refill {
            if let Some(r) = &mut self.radio {
                r.fetching = true;
            }
            if let Err(error) = self.submit_job(&request) {
                if let Some(r) = &mut self.radio {
                    r.fetching = false;
                    r.retry_at = std::time::Instant::now() + Duration::from_secs(5);
                }
                self.events
                    .push_back(json!({"event":"radio_error","error":error.to_string()}));
            }
        }
        if let Some(event) = self.events.pop_front() {
            return Ok(Some(event));
        }
        if self
            .sleep_deadline
            .is_some_and(|d| std::time::Instant::now() >= d)
        {
            if let Some(pending) = self.pending_play.take() {
                let _ = self.jobs.cancel(&pending.job);
                if let Some(cursor) = pending.cursor_before {
                    self.restore_cursor(cursor);
                    self.store.save()?;
                }
            }
            self.player.pause();
            self.sleep_deadline = None;
            return Ok(Some(
                json!({"event":"sleep_timer","player":self.player.snapshot()}),
            ));
        }
        let fade = self.player.should_crossfade()
            && self.store.data.queue.index.is_some_and(|i| {
                i + 1 < self.store.data.queue.tracks.len()
                    || self.store.data.queue.repeat != Repeat::Off
                    || self.store.data.queue.shuffle
            });
        if (self.player.finished() || fade) && !self.player.paused() && self.pending_play.is_none()
        {
            let decoder = self.player.snapshot()["decoder_error"]
                .as_str()
                .unwrap_or("")
                .to_string();
            if !decoder.is_empty() {
                let current = self.player.track_id.clone();
                let recover = current
                    .as_ref()
                    .and_then(|id| self.store.data.tracks.get(id))
                    .is_some_and(|t| t.local_path.is_none() && t.platform != "local");
                if recover && self.recovery_attempts < 2 {
                    self.recovery_attempts += 1;
                    let position = self.player.position();
                    let id = current.context("No recovery track")?;
                    let duration = self.player.duration;
                    self.player.wait_for_stream(id.clone(), duration, position);
                    self.providers.recover_stream(self.store.track(&id)?);
                    self.request_playback(&id, false, position)?;
                    if let Some(pending) = self.pending_play.as_mut() {
                        pending.record_history = false;
                    }
                    return Ok(Some(
                        json!({"event":"playback_recovering","attempt":self.recovery_attempts,"player":self.snapshot()}),
                    ));
                }
                self.player.stop();
                return Ok(Some(json!({"event":"playback_error","error":decoder})));
            }
            if !fade
                && self.autoplay.is_some()
                && self.autoplay_enabled()
                && self
                    .store
                    .data
                    .queue
                    .index
                    .is_some_and(|i| i + 1 >= self.store.data.queue.tracks.len())
            {
                return Ok(None);
            }
            let cursor = QueueCursor {
                index: self.store.data.queue.index,
                back_stack: self.store.data.queue.back_stack.clone(),
            };
            let id = self.advance_queue(true);
            if let Some(id) = id {
                self.request_playback(&id, false, 0.)?;
                if let Some(pending) = self.pending_play.as_mut() {
                    pending.cursor_before = Some(cursor);
                }
            } else {
                self.player.stop();
                self.store.save()?;
            }
            return Ok(Some(
                json!({"event":"track_changed","player":self.player.snapshot(),"queue":self.store.data.queue}),
            ));
        }
        Ok(None)
    }
    fn poll_jobs(&mut self) {
        for mut job in self.jobs.drain() {
            let transition_cursor = self
                .pending_play
                .as_ref()
                .filter(|p| p.job == job.id)
                .and_then(|p| p.cursor_before.clone());
            let apply = (|| -> Result<()> {
                if self
                    .preview_audio_job
                    .as_ref()
                    .is_some_and(|(job_id, _)| job_id == &job.id)
                    && let Some((_, track_id)) = self.preview_audio_job.take()
                    && self.player.track_id.as_deref() != Some(&track_id)
                    && self
                        .pending_play
                        .as_ref()
                        .is_none_or(|pending| pending.id == track_id)
                    && let Ok(result) = &job.result
                    && let Some(input) = result["url"].as_str()
                {
                    let _ = self.player.prepare_audio(
                        input.into(),
                        stream_headers(result),
                        &self.store.data.effects,
                        true,
                    );
                }
                if self.next_audio_job.as_deref() == Some(&job.id) {
                    self.next_audio_job = None;
                    if let Some((id, _)) = &self.warmed_next
                        && self
                            .player
                            .track_id
                            .as_ref()
                            .is_some_and(|current| current != id)
                        && (self
                            .pending_play
                            .as_ref()
                            .is_some_and(|pending| pending.id == *id)
                            || self
                                .store
                                .data
                                .queue
                                .index
                                .and_then(|index| self.store.data.queue.tracks.get(index + 1))
                                == Some(id))
                        && self
                            .pending_play
                            .as_ref()
                            .is_none_or(|pending| pending.id == *id)
                        && let Ok(result) = &job.result
                        && let Some(input) = result["url"].as_str()
                    {
                        let _ = self.player.prepare_audio(
                            input.into(),
                            stream_headers(result),
                            &self.store.data.effects,
                            false,
                        );
                    }
                }
                if self.autoplay.as_ref().is_some_and(|r| r.job == job.id) {
                    let request = self.autoplay.take().expect("autoplay request");
                    if request.smart
                        && self.smart_shuffle()
                        && self.radio.is_none()
                        && request.queue == self.store.data.queue.tracks
                        && let Ok(value) = &job.result
                    {
                        let avoid = mixes::Avoid::new(&self.store.data);
                        let tracks: Vec<Track> = serde_json::from_value::<Vec<Track>>(value["tracks"].clone())?
                            .into_iter()
                            .filter(|t| !avoid.blocks(t))
                            .collect();
                        let mut known: std::collections::HashSet<_> = request
                            .queue
                            .iter()
                            .chain(self.smart_pool.iter().map(|track| &track.id))
                            .cloned()
                            .collect();
                        self.smart_pool.extend(
                            tracks
                                .into_iter()
                                .filter(|track| known.insert(track.id.clone()))
                                .take(8),
                        );
                    } else if !request.smart
                        && self.autoplay_enabled()
                        && self.store.data.queue.index.is_some()
                        && request.queue == self.store.data.queue.tracks
                        && let Ok(value) = &job.result
                    {
                        let avoid = mixes::Avoid::new(&self.store.data);
                        let tracks: Vec<Track> = serde_json::from_value::<Vec<Track>>(value["tracks"].clone())?
                            .into_iter()
                            .filter(|t| !avoid.blocks(t))
                            .collect();
                        let mut known: std::collections::HashSet<_> =
                            request.queue.into_iter().collect();
                        let slots = 4096usize.saturating_sub(self.store.data.queue.tracks.len());
                        for track in tracks
                            .into_iter()
                            .filter(|t| known.insert(t.id.clone()))
                            .take(slots)
                        {
                            self.store.data.queue.tracks.push(track.id.clone());
                            self.store
                                .data
                                .tracks
                                .entry(track.id.clone())
                                .or_insert(track);
                        }
                        self.store.save()?;
                    }
                }
                let Ok(result) = &job.result else {
                    return Ok(());
                };
                if job.operation == "radio.prepare" {
                    if self.radio.as_ref().is_some_and(|r| {
                        Some(r.generation.as_str()) == result["generation"].as_str()
                    }) {
                        let id = text(result, "queue_seed")?.to_owned();
                        let seed = format!("youtube:{}", text(result, "video")?);
                        if let Some(r) = self.radio.as_mut() {
                            r.seed = seed.clone();
                        }
                        if self.player.track_id.as_deref() != Some(&id) {
                            self.request_playback(&id, false, 0.)?;
                        }
                        let next=self.submit_job(&json!({"operation":"recommendations.radio","params":{"seed":seed,"queue_seed":id,"limit":50,"append_to_queue":true,"generation":result["generation"]}}))?;
                        job.result = Ok(next);
                    } else {
                        job.result = Ok(json!({"superseded":true}));
                    }
                } else if job.operation == "stream.prepare" || job.operation == "playback.prepare" {
                    if self.pending_play.as_ref().is_some_and(|p| p.job == job.id) {
                        let pending = self.pending_play.take().expect("pending playback");
                        if pending.record_history {
                            self.play_id(&pending.id)?;
                        } else {
                            self.load_id(&pending.id, false)?;
                        }
                        if pending.select_queue {
                            let index = self
                                .store
                                .data
                                .queue
                                .tracks
                                .iter()
                                .position(|id| id == &pending.id)
                                .unwrap_or_else(|| {
                                    self.store.data.queue.tracks.push(pending.id.clone());
                                    self.store.data.queue.tracks.len() - 1
                                });
                            self.store.data.queue.index = Some(index);
                        }
                        if pending.position > 0. && pending.position < self.player.duration {
                            self.player.seek(pending.position)?;
                        }
                        if pending.paused {
                            self.player.pause();
                        }
                        self.store.save()?;
                        job.result = Ok(self.player.snapshot());
                        self.events.push_back(json!({"event":"track_changed","player":self.player.snapshot(),"queue":self.store.data.queue}));
                    }
                } else if job.operation == "spotify.import" {
                    let tracks: Vec<Track> = serde_json::from_value(result["tracks"].clone())?;
                    let ids: Vec<_> = tracks.iter().map(|t| t.id.clone()).collect();
                    for t in tracks {
                        self.store.data.tracks.insert(t.id.clone(), t);
                    }
                    for id in &ids {
                        if !self.store.data.favorites.contains(id) {
                            self.store.data.favorites.push(id.clone());
                        }
                    }
                    let playlist = Playlist {
                        id: uuid::Uuid::new_v4().to_string(),
                        name: "Spotify liked tracks".into(),
                        tracks: ids,
                    };
                    self.store
                        .data
                        .playlists
                        .insert(playlist.id.clone(), playlist.clone());
                    self.store.save()?;
                    job.result =
                        Ok(json!({"playlist":playlist,"matching":"deferred_until_playback"}));
                } else if job.operation == "google.import" {
                    let tracks = oauth::map_youtube_tracks(&result["library"]);
                    let ids: Vec<_> = tracks.iter().map(|t| t.id.clone()).collect();
                    for t in tracks {
                        self.store.data.tracks.insert(t.id.clone(), t);
                    }
                    if result["kind"] == "likes" {
                        for id in &ids {
                            if !self.store.data.favorites.contains(id) {
                                self.store.data.favorites.push(id.clone());
                            }
                        }
                    }
                    let playlist = Playlist {
                        id: uuid::Uuid::new_v4().to_string(),
                        name: result["name"].as_str().unwrap_or("YouTube library").into(),
                        tracks: ids,
                    };
                    self.store
                        .data
                        .playlists
                        .insert(playlist.id.clone(), playlist.clone());
                    self.store.save()?;
                    job.result = Ok(json!({"playlist":playlist}));
                } else if job.operation == "library.scan" {
                    let tracks: Vec<Track> = serde_json::from_value(result["tracks"].clone())?;
                    for t in tracks {
                        self.store.data.tracks.insert(t.id.clone(), t);
                    }
                    self.store.save()?;
                } else if job.operation == "plugins.run" {
                    let commands = result["commands"]
                        .as_array()
                        .context("Invalid plugin result")?
                        .clone();
                    for request in commands {
                        self.dispatch(&request)?;
                    }
                } else if job.operation == "recommendations.radio"
                    && result["append_to_queue"] == true
                {
                    let seed = result["queue_seed"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| {
                            format!("youtube:{}", result["seed"].as_str().unwrap_or(""))
                        });
                    if self.radio.as_ref().is_some_and(|r| {
                        Some(r.generation.as_str()) == result["generation"].as_str()
                    }) {
                        let avoid = mixes::Avoid::new(&self.store.data);
                        let tracks: Vec<Track> = serde_json::from_value::<Vec<Track>>(result["tracks"].clone())?
                            .into_iter()
                            .filter(|t| !avoid.blocks(t))
                            .collect();
                        let extend = result["extend"] == true;
                        let ids: Vec<_> = tracks
                            .iter()
                            .filter(|t| !extend || !self.store.data.queue.tracks.contains(&t.id))
                            .map(|t| t.id.clone())
                            .collect();
                        for track in tracks {
                            self.store.data.tracks.insert(track.id.clone(), track);
                        }
                        let empty = ids.is_empty();
                        if extend {
                            self.store.data.queue.tracks.extend(ids);
                        } else {
                            self.store.data.queue.tracks =
                                std::iter::once(seed).chain(ids).collect();
                            self.store.data.queue.index = Some(0);
                            self.store.data.queue.back_stack.clear();
                        }
                        if let Some(r) = &mut self.radio {
                            r.fetching = false;
                            let next = result["continuation"].as_str().map(str::to_string);
                            r.continuation = if (extend && empty) || r.continuation == next {
                                None
                            } else {
                                next
                            };
                        }
                        self.store.save()?;
                    }
                } else if job.operation == "collection.import" || job.operation == "catalog.import"
                {
                    let tracks: Vec<Track> = serde_json::from_value(result["tracks"].clone())?;
                    let playlist = Playlist {
                        id: uuid::Uuid::new_v4().to_string(),
                        name: result["name"].as_str().unwrap_or("Imported").into(),
                        tracks: tracks.iter().map(|t| t.id.clone()).collect(),
                    };
                    for track in tracks {
                        self.store.data.tracks.insert(track.id.clone(), track);
                    }
                    self.store
                        .data
                        .playlists
                        .insert(playlist.id.clone(), playlist.clone());
                    self.store.save()?;
                    let mut output = result.clone();
                    output["playlist"] = json!(playlist);
                    job.result = Ok(output);
                } else if job.operation == "download" || job.operation == "cache.prefetch" {
                    let id = job.target.as_deref().context("Missing target track")?;
                    let track = self
                        .store
                        .data
                        .tracks
                        .get_mut(id)
                        .context("Track was removed while downloading; file remains on disk")?;
                    let path = Some(serde_json::from_value(result["path"].clone())?);
                    if job.operation == "cache.prefetch" {
                        track.cache_path = path;
                    } else {
                        track.local_path = path;
                    }
                    self.store.save()?;
                    if job.operation == "cache.prefetch" {
                        self.prune_cache(self.cache_limit())?;
                    }
                }
                Ok(())
            })();
            if let Err(error) = apply {
                job.result = Err(error);
            }
            if job.result.is_err()
                && let Some(cursor) = transition_cursor
            {
                self.restore_cursor(cursor);
                if let Err(error) = self.store.save() {
                    job.result = Err(error);
                }
            }
            if job.operation == "lastfm.auto"
                && let Err(e) = &job.result
            {
                self.integrations.last_error = Some(e.to_string());
            }
            if job.operation == "recommendations.radio"
                && job.result.is_err()
                && let Some(r) = &mut self.radio
            {
                r.fetching = false;
                r.continuation = None;
            }
            if self.pending_play.as_ref().is_some_and(|p| p.job == job.id) {
                self.pending_play = None;
            }
            self.jobs.complete(&job);
            if let Ok(status) = self.jobs.get(&job.id) {
                self.events
                    .push_back(json!({"event":"job_finished","job":status}));
            }
        }
    }
    fn submit_job(&mut self, p: &Value) -> Result<Value> {
        let operation = text(p, "operation")?.to_string();
        let params = p.get("params").cloned().unwrap_or(Value::Null);
        let providers = Providers {
            cookies: self.providers.cookies.clone(),
            proxy: self.providers.proxy.clone(),
            browser: self.providers.browser.clone(),
        };
        let client = self.client.clone();
        let spotify_token = if operation == "spotify.library" || operation == "spotify.import" {
            Some(self.current_token("spotify")?)
        } else if operation == "spotify.search" || operation.starts_with("catalog.") {
            self.current_token("spotify").ok()
        } else {
            None
        };
        let google_token = if operation == "music.mixes" {
            self.current_token("google").ok()
        } else if operation == "google.import" || operation == "google.library" {
            Some(self.current_token("google")?)
        } else {
            None
        };
        let root = self.store.root.clone();
        let search_cache = self.search_cache.clone();
        let search_key = format!("{operation}|{params}");
        let mix_taste = if operation == "music.mixes" {
            mixes::taste(
                &self.store.data,
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            )
        } else {
            mixes::Taste::default()
        };
        let mix_key = format!(
            "music.mixes|{}|{}",
            serde_json::to_string(&mix_taste.seeds)?,
            mix_taste.recent.len()
        );
        let support = if operation == "support.export" {
            let state = self.snapshot();
            Some(
                json!({"version":env!("CARGO_PKG_VERSION"),"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"track_count":self.store.data.tracks.len(),"playlist_count":self.store.data.playlists.len(),"queue_count":self.store.data.queue.tracks.len(),"player":{"loaded":state["track_id"].is_string(),"playing":state["playing"],"paused":state["paused"],"buffering":state["buffering"],"decoder_error":state["decoder_error"]},"proxy_configured":self.client.config.proxy.is_some(),"cookies_configured":self.providers.cookies.is_some(),"browser_configured":self.providers.browser.is_some(),"account_status":{"google":self.oauth.status("google"),"spotify":self.oauth.status("spotify")},"extension_health":{"theme_warning_count":extensions::theme_report(&root)?["warnings"].as_array().map(Vec::len),"plugin_warning_count":plugins::report(&root)?["warnings"].as_array().map(Vec::len)}}),
            )
        } else {
            None
        };
        let track = if operation == "stream.prepare" && params["track"].is_object() {
            let track: Track = serde_json::from_value(params["track"].clone())?;
            ensure!(
                !track.title.is_empty() && track.duration.is_finite() && track.duration >= 0.,
                "Invalid preview track"
            );
            Some(track)
        } else if operation == "download"
            || operation == "cache.prefetch"
            || operation == "stream.prepare"
            || operation == "playback.prepare"
            || operation == "radio.prepare"
        {
            Some(self.store.track(text(&params, "id")?)?.clone())
        } else {
            None
        };
        let target = track.as_ref().map(|t| t.id.clone());
        let op = operation.clone();
        ensure!(
            [
                "search",
                "network.test",
                "updates.check",
                "updates.stage",
                "support.export",
                "spotify.search",
                "spotify.library",
                "spotify.import",
                "stream.prepare",
                "playback.prepare",
                "radio.prepare",
                "collection.fetch",
                "collection.import",
                "catalog.metadata",
                "catalog.import",
                "library.scan",
                "download",
                "cache.prefetch",
                "audio.encode",
                "audio.waveform",
                "lyrics.get",
                "lyrics.words",
                "translate",
                "tools.install",
                "music.search",
                "plugins.run",
                "google.import",
                "google.library",
                "lastfm.auto",
                "autoplay.fill",
                "music.mixes",
                "music.home",
                "music.charts",
                "music.browse",
                "recommendations.radio"
            ]
            .contains(&op.as_str()),
            "Operation does not support background jobs"
        );
        self.jobs
            .submit(operation, target, move || match op.as_str() {
                "updates.check"=>updates::check(&client,params["repo"].as_str()),
                "updates.stage"=>updates::stage(&client,&root,params["repo"].as_str()),
                "support.export"=>{let mut report=support.context("Missing support snapshot")?;let tools=tools::status();let mut safe=serde_json::Map::new();for name in ["ffmpeg","ffprobe","yt-dlp"] {safe.insert(name.into(),json!({"available":tools[name]["available"],"version":tools[name]["version"]}));}report["tools"]=Value::Object(safe);store::write_json(&PathBuf::from(text(&params,"path")?),&report)?;Ok(json!({"exported":true,"private_paths_and_track_titles_excluded":true}))},
                "network.test"=>{let start=std::time::Instant::now();let response=client.get("https://www.google.com/generate_204").timeout(Duration::from_secs(10)).send()?.error_for_status()?;Ok(json!({"reachable":true,"status":response.status().as_u16(),"elapsed_ms":start.elapsed().as_millis()}))},
                "spotify.library"|"spotify.import"=>spotify::library(&client,spotify_token.as_deref().context("Connect Spotify first")?,params["kind"].as_str().unwrap_or("liked")),
                "spotify.search"=>search_cache.run(search_key,||spotify::search_auto(&client,spotify_token.as_deref(),text(&params,"query")?,params["limit"].as_u64().unwrap_or(10) as usize)),
                "radio.prepare"=>{
                    let track=track.context("Missing radio track")?;
                    let video=if track.id.starts_with("youtube:")&&track.platform!="soundcloud" {track.id.trim_start_matches("youtube:").to_owned()}
                    else{let mut catalog=track.clone();catalog.platform="catalog".into();let resolved=providers.resolve(&catalog)?;let source=resolved["playback_source"].as_str().context("No verified YouTube radio source")?;url::Url::parse(source)?.query_pairs().find(|(key,_)|key=="v").map(|(_,id)|id.into_owned()).context("Missing radio video")?};
                    youtube::validate_video(&video)?;Ok(json!({"video":video,"queue_seed":track.id,"generation":params["generation"]}))
                },
                "stream.prepare"|"playback.prepare"=>{let track=track.context("Missing track")?;if track.local_path.is_some()||track.cache_path.as_ref().is_some_and(|p|p.is_file()){Ok(json!({"local":true}))}else{providers.resolve(&track)}},
                "lastfm.auto"=>services::lastfm(&client,&params),
                "google.library"=>oauth::youtube_library(&client,google_token.as_deref().context("Sign in required")?,text(&params,"kind")?,params["playlist_id"].as_str()),
                "google.import"=>{let kind=text(&params,"kind")?;ensure!(["likes","tracks"].contains(&kind),"Import expects likes or tracks");let value=oauth::youtube_library(&client,google_token.as_deref().context("Sign in required")?,kind,params["playlist_id"].as_str())?;Ok(json!({"kind":kind,"name":params["name"].as_str().unwrap_or(if kind=="likes"{"YouTube likes"}else{"YouTube playlist"}),"library":value}))},
                "plugins.run"=>plugins::execute(&root,text(&params,"id")?,params["params"].clone()),
                "music.search" => search_cache.run(search_key,||{let api=youtube::Music{client:&client,cookies:providers.cookies.as_ref()};if let Some(filter)=params["filter"].as_str(){api.search_filtered(text(&params,"query")?,filter)}else{api.search(text(&params,"query")?)}}),
                "autoplay.fill" => {
                    let seed: Track = serde_json::from_value(params["track"].clone())?;
                    serde_json::to_value(mixes::build(
                        &youtube::Music{client:&client,cookies:providers.cookies.as_ref()}, &seed,
                    )?).map_err(Into::into)
                },
                "music.mixes" => search_cache.run(mix_key, || {
                    mixes::account_mixes(&youtube::Music{client:&client,cookies:providers.cookies.as_ref()}, google_token.as_deref(), &mix_taste)
                }),
                "music.home" => youtube::Music{client:&client,cookies:providers.cookies.as_ref()}.home(),
                "music.charts" => youtube::Music{client:&client,cookies:providers.cookies.as_ref()}.charts(),
                "music.browse" => youtube::Music{client:&client,cookies:providers.cookies.as_ref()}.browse(text(&params,"id")?,params["params"].as_str()),
                "recommendations.radio" => {
                    let seed=text(&params,"seed")?.trim_start_matches("youtube:");
                    let mut value=youtube::Music{client:&client,cookies:providers.cookies.as_ref()}.radio(seed,params["limit"].as_u64().unwrap_or(50) as usize,params["continuation"].as_str())?;
                    value["append_to_queue"]=json!(boolean(&params,"append_to_queue",false));value["generation"]=params["generation"].clone();value["extend"]=json!(boolean(&params,"extend",false));value["queue_seed"]=params["queue_seed"].clone();Ok(value)
                },
                "search" => search_cache.run(search_key,||Ok(json!(providers.search(
                    text(&params, "query")?,
                    params["platform"].as_str().unwrap_or("youtube"),
                    params["limit"].as_u64().unwrap_or(20) as usize
                )?))),
                "collection.fetch" | "collection.import" => providers.collection(
                    text(&params, "url")?,
                    params["limit"].as_u64().unwrap_or(1000) as usize,
                ),
                "library.scan" => {
                    let paths = serde_json::from_value::<Vec<PathBuf>>(params["paths"].clone())?;
                    Ok(json!(media::scan(&paths)))
                }
                "download" | "cache.prefetch" => {
                    let track = track.context("Missing track")?;
                    ensure!(track.platform != "local", "Track is already local");
                    let dir = if op == "cache.prefetch" {
                        root.join("cache")
                    } else {
                        params["directory"]
                            .as_str()
                            .map(PathBuf::from)
                            .unwrap_or_else(|| root.join("downloads"))
                    };
                    let path = providers.download(
                        &track,
                        &dir,
                        params["format"].as_str().unwrap_or("opus"),
                    )?;
                    Ok(json!({"path":path,"track_id":track.id}))
                }
                "audio.encode" => media::encode(&serde_json::from_value(params)?),
                "audio.waveform"=>media::waveform(&PathBuf::from(text(&params,"path")?),params["points"].as_u64().unwrap_or(1024) as usize),
                "lyrics.get" => lyrics::fetch_source(
                    &client,
                    &root,
                    text(&params, "title")?,
                    text(&params, "artist")?,
                    params["duration"].as_f64().unwrap_or(0.),
                    boolean(&params, "online", true),
                    params["source"].as_str().unwrap_or("auto"),
                ),
                "translate" => translation::fetch(&client, &root,&params),
                "lyrics.words"=>lyrics::words_online(&client,text(&params,"title")?,text(&params,"artist")?,params["duration"].as_f64().unwrap_or(0.)),
                "catalog.metadata"=>catalog::metadata_with_proxy(&client,text(&params,"url")?,spotify_token.as_deref(),providers.proxy.as_deref()),
                "catalog.import"=>{let source=text(&params,"url")?;let metadata=catalog::metadata_with_proxy(&client,source,spotify_token.as_deref(),providers.proxy.as_deref())?;catalog::import_plan(&metadata,source,boolean(&params,"allow_partial",false))},
                "tools.install" => match text(&params, "name")? {
                    "yt-dlp" => tools::install_ytdlp(&client),
                    "ffmpeg" => tools::install_ffmpeg(&client),
                    _ => anyhow::bail!("Managed installation supports yt-dlp and ffmpeg"),
                },
                _ => unreachable!(),
            })
    }
    fn restore_cursor(&mut self, cursor: QueueCursor) {
        self.store.data.queue.index = cursor
            .index
            .filter(|i| *i < self.store.data.queue.tracks.len());
        self.store.data.queue.back_stack = cursor
            .back_stack
            .into_iter()
            .filter(|i| *i < self.store.data.queue.tracks.len())
            .collect();
    }
    fn resume_or_start(&mut self) -> Result<Value> {
        if self.pending_play.is_some() {
            return Ok(self.snapshot());
        }
        if self.player.has_loaded_audio() && !self.player.finished() {
            self.player.resume()?;
            return Ok(self.snapshot());
        }
        let queue = &self.store.data.queue;
        if let Some(id) = queue
            .current()
            .or_else(|| queue.tracks.first().map(String::as_str))
            .filter(|id| self.store.data.tracks.contains_key(*id))
            .map(str::to_owned)
        {
            return self.request_playback(&id, true, 0.);
        }
        let saved = &self
            .store
            .data
            .settings
            .get("resume")
            .cloned()
            .unwrap_or(Value::Null);
        if let Some(id) = saved["id"]
            .as_str()
            .filter(|id| self.store.data.tracks.contains_key(*id))
        {
            let position = saved["position"]
                .as_f64()
                .filter(|x| x.is_finite() && *x >= 0.)
                .unwrap_or(0.);
            return self.request_playback(id, true, position);
        }
        Ok(self.snapshot())
    }

    fn request_playback(&mut self, id: &str, select_queue: bool, position: f64) -> Result<Value> {
        self.store.track(id)?;
        if let Some(pending) = self.pending_play.take() {
            let _ = self.jobs.cancel(&pending.job);
            if let Some(cursor) = pending.cursor_before {
                self.restore_cursor(cursor);
            }
        }
        let result =
            self.submit_job(&json!({"operation":"playback.prepare","params":{"id":id}}))?;
        self.pending_play = Some(PendingPlayback {
            job: text(&result, "job_id")?.to_owned(),
            id: id.into(),
            select_queue,
            position,
            record_history: true,
            paused: false,
            cursor_before: None,
        });
        Ok(result)
    }
    fn cache_limit(&self) -> u64 {
        self.store
            .data
            .settings
            .get("cache_limit_mb")
            .and_then(Value::as_u64)
            .unwrap_or(1024)
            .clamp(64, 65536)
            .saturating_mul(1024 * 1024)
    }
    fn prune_cache(&mut self, limit: u64) -> Result<Value> {
        let protected: Vec<_> = self
            .player
            .track_id
            .as_ref()
            .and_then(|id| self.store.data.tracks.get(id))
            .and_then(|t| t.cache_path.clone())
            .into_iter()
            .collect();
        let result = cache::prune(&self.store.root, limit, &protected)?;
        for t in self.store.data.tracks.values_mut() {
            if t.cache_path.as_ref().is_some_and(|p| !p.is_file()) {
                t.cache_path = None;
            }
        }
        self.store.save()?;
        Ok(result)
    }
    fn play_id(&mut self, id: &str) -> Result<()> {
        self.recovery_attempts = 0;
        self.load_id(id, true)
    }
    fn current_token(&mut self, provider: &str) -> Result<String> {
        if let Ok(token) = self.oauth.token(provider) {
            return Ok(token.into());
        }
        self.oauth.refresh(&self.client, provider)?;
        vault::save(
            &self.store.root.join("oauth.dpapi"),
            &self.oauth.private_session(),
        )?;
        Ok(self.oauth.token(provider)?.into())
    }
    fn tick_discord(&mut self) {
        let config = self
            .store
            .data
            .settings
            .get("discord")
            .cloned()
            .unwrap_or(Value::Null);
        if config["enabled"] != true {
            return;
        }
        if self.presence.status()["enabled"] != true {
            if self.discord_update.elapsed() < Duration::from_secs(15) {
                return;
            }
            self.discord_update = std::time::Instant::now();
            if let Some(id) = config["application_id"].as_str() {
                let _ = self.presence.connect(id);
            }
            return;
        }
        let playing = self.player.snapshot()["playing"] == true
            && self.player.snapshot()["buffering"] == false;
        let signature = format!(
            "{:?}:{}:{}",
            self.player.track_id,
            !playing,
            (self.player.position() / 15.) as u64
        );
        if signature == self.discord_signature {
            return;
        }
        self.discord_signature = signature;
        if !playing || self.player.track_id.is_none() {
            let _ = self.presence.clear();
            return;
        }
        if let Some(track) = self
            .player
            .track_id
            .as_ref()
            .and_then(|id| self.store.data.tracks.get(id))
        {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();
            let speed = self.store.data.effects.speed * self.store.data.effects.speed_pitch;
            let mut value = json!({"title":track.title,"artist":track.artist,"started_at":(now-self.player.position()/speed).max(0.)as i64});
            if let Some(cover) = &track.cover
                && cover.starts_with("https://")
            {
                value["cover"] = json!(cover);
            }
            if self.player.duration > 0. {
                value["ends_at"] = json!(
                    (now + (self.player.duration - self.player.position()).max(0.) / speed) as i64
                );
            }
            if self.presence.update(&value).is_err() {
                self.presence.close();
            }
        }
    }
    pub fn snapshot(&self) -> Value {
        let mut value = self.player.snapshot();
        value["sleep_remaining"] = json!(self.sleep_deadline.map(|d| {
            d.saturating_duration_since(std::time::Instant::now())
                .as_secs()
        }));
        value["pending_track_id"] = json!(self.pending_play.as_ref().map(|p| &p.id));
        value["tasks"] = self.jobs.activity();
        value
    }
    pub fn persist_session(&mut self) -> Result<()> {
        self.store.data.settings.insert(
            "resume".into(),
            json!({"id":self.player.track_id,"position":self.player.position()}),
        );
        self.store.save()
    }
    fn load_id(&mut self, id: &str, record_history: bool) -> Result<()> {
        let track = self.store.track(id)?.clone();
        let path = track
            .local_path
            .as_ref()
            .or_else(|| track.cache_path.as_ref().filter(|p| p.is_file()));
        let mut provenance = Value::Null;
        let (input, headers, duration) = if let Some(path) = path {
            ensure!(
                path.is_file(),
                "Local audio file is missing: {}",
                path.display()
            );
            if track.cache_path.as_ref() == Some(path) {
                cache::touch(path);
            }
            (path.to_string_lossy().into_owned(), vec![], track.duration)
        } else {
            let v = self.providers.resolve(&track)?;
            if ["spotify", "catalog"].contains(&track.platform.as_str()) {
                provenance = json!({"url":v["playback_source"],"title":v["matched_title"],"artist":v["matched_artist"],"duration":v["duration"],"verified_duration":v["verified_duration"]});
            }
            let headers = stream_headers(&v);
            (
                v["url"].as_str().context("Missing stream URL")?.into(),
                headers,
                v["duration"].as_f64().unwrap_or(track.duration),
            )
        };
        self.player.play(
            id.into(),
            input,
            headers,
            duration,
            self.store.data.effects.clone(),
        )?;
        // The next songs in the queue get their stream links ready now, so skipping ahead starts at once.
        if let Some(at) = self.store.data.queue.tracks.iter().position(|queued| queued == id) {
            let upcoming: Vec<String> = self.store.data.queue.tracks[at + 1..]
                .iter()
                .take(2)
                .filter_map(|next| self.store.data.tracks.get(next))
                .map(|track| track.source.clone())
                .collect();
            youtube_stream::warm(upcoming);
        }
        self.player.playback_source = provenance;
        self.track_revision = self.track_revision.wrapping_add(1);
        if record_history {
            self.store.data.history.push(PlayEvent {
                track_id: id.into(),
                started_at: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            });
        }
        if self.store.data.history.len() > 10000 {
            self.store.data.history.remove(0);
        }
        self.store.save()
    }
    /// Synchronous command boundary. Call from a dedicated worker, never the UI render thread.
    pub fn dispatch(&mut self, request: &Value) -> Result<Value> {
        let command = text(request, "command")?;
        let p = request.get("params").unwrap_or(&Value::Null);
        if [
            "network.set",
            "cookies.set",
            "cookies.browser",
            "oauth.finish",
            "oauth.adopt",
            "oauth.logout",
            "backup.import",
        ]
        .contains(&command)
        {
            self.search_cache.clear();
        }
        if matches!(
            command,
            "player.request"
                | "player.play"
                | "player.stop"
                | "player.preview"
                | "queue.set"
                | "queue.reorder"
                | "radio.start"
                | "jobs.cancel_all"
                | "network.set"
                | "cookies.browser"
                | "cookies.set"
                | "cookies.clear"
                | "oauth.logout"
                | "backup.import"
        ) {
            self.cancel_autoplay();
        }
        if matches!(
            command,
            "oauth.finish" | "oauth.logout" | "cookies.set" | "cookies.clear"
        ) {
            self.jobs.cancel_operations(&["music.mixes"]);
            self.search_cache.clear();
        }
        if matches!(
            command,
            "player.stop"
                | "network.set"
                | "cookies.set"
                | "cookies.clear"
                | "cookies.browser"
                | "jobs.cancel_all"
        ) {
            self.preview_audio_job = None;
        }
        let mut cancelled_cursor = None;
        if [
            "player.play",
            "player.stop",
            "jobs.cancel_all",
            "player.pause",
            "player.next",
            "player.previous",
            "player.next.request",
            "player.previous.request",
            "radio.start",
            "radio.personal",
            "session.resume",
            "queue.set",
            "player.preview",
            "queue.reorder",
            "library.remove",
        ]
        .contains(&command)
            && let Some(pending) = self.pending_play.take()
        {
            let _ = self.jobs.cancel(&pending.job);
            if ["player.next.request", "player.previous.request"].contains(&command) {
                cancelled_cursor = pending.cursor_before;
            } else if let Some(cursor) = pending.cursor_before {
                self.restore_cursor(cursor);
                self.store.save()?;
            }
        }
        match command {
            "app.install"=>installation::install(&std::env::current_exe()?),
            "support.export"=>self.submit_job(&json!({"operation":command,"params":p})),
            "updates.rollback"=>installation::stage_previous(std::env::current_exe()?.parent().context("Missing application directory")?,&self.store.root.join("updates")),
            "updates.configure"=>{let repo=text(p,"repo")?.trim();if !repo.is_empty(){updates::validate_repo(repo)?;}self.store.data.settings.insert("release_repo".into(),json!(repo));self.store.save()?;Ok(json!({"configured":!repo.is_empty()}))},
            "updates.check"|"updates.stage"=>self.submit_job(&json!({"operation":command,"params":{"repo":self.store.data.settings.get("release_repo")}})),
            "appearance.set"=>{let appearance=appearance::Appearance::from_tokens(p.clone())?;self.store.data.settings.insert("appearance".into(),json!(appearance));self.store.save()?;Ok(json!(appearance))},
            "themes.apply"=>{let theme=extensions::themes(&self.store.root)?.into_iter().find(|t|Some(t.id.as_str())==p["id"].as_str()).context("Theme not found")?;self.dispatch(&json!({"command":"appearance.set","params":theme.tokens}))},
            "network.status"=>Ok(self.client.config.status()),
            "network.set"=>{
                let proxy=p["proxy"].as_str().map(str::trim).filter(|s|!s.is_empty()).map(str::to_owned);
                let config=network::Config{proxy};let client=config.client()?;config.save(&self.store.root)?;
                let resume=if self.player.is_remote(){self.player.track_id.clone().map(|id|(id,self.player.duration,self.player.position(),self.player.paused()))}else{None};
                self.providers.proxy=client.proxy.clone();self.player.proxy=client.proxy.clone();self.client=client;
                if let Some((id,duration,position,paused))=resume{self.player.wait_for_stream(id.clone(),duration,position);self.request_playback(&id,false,position)?;if let Some(pending)=self.pending_play.as_mut(){pending.record_history=false;pending.paused=paused;}}
                Ok(config.status())
            },
            "network.test"=>self.submit_job(&json!({"operation":"network.test","params":{}})),
            "cookies.browser"=>{let browser=p["browser"].as_str().filter(|s|!s.is_empty());ensure!(browser.is_none_or(|s|["chrome","edge","firefox","brave","vivaldi","opera"].contains(&s)),"Unsupported browser");self.providers.browser=browser.map(str::to_owned);self.store.data.settings.insert("cookie_browser".into(),json!(browser));self.store.save()?;Ok(json!({"browser":browser,"scope":"yt-dlp playback only; Music account personalization uses a cookie file"}))},
            "spotify.library"|"spotify.import"=>self.submit_job(&json!({"operation":command,"params":p})),
            "spotify.search" => self.submit_job(&json!({"operation":"spotify.search","params":p})),
            "player.request" => {self.radio=None;self.region_end=None;self.request_playback(text(p,"id")?,true,0.)}
            "player.next.request"|"player.previous.request"=>{
                let cursor=cancelled_cursor.unwrap_or_else(||QueueCursor{index:self.store.data.queue.index,back_stack:self.store.data.queue.back_stack.clone()});
                let id=if command=="player.next.request"{self.advance_queue(false)}else{self.store.data.queue.previous().map(str::to_owned)};
                if let Some(id)=id {let result=self.request_playback(&id,false,0.)?;if let Some(pending)=self.pending_play.as_mut(){pending.cursor_before=Some(cursor);}Ok(result)}else{self.player.stop();Ok(self.player.snapshot())}
            }
            "queue.play"=>{
                self.dispatch(&json!({"command":"queue.set","params":{"tracks":p["tracks"],"index":p["index"],"play":false}}))?;
                let id=self.store.data.queue.current().context("Queue is empty")?.to_owned();self.request_playback(&id,false,0.)
            }
            "session.resume.request"=>{
                let resume=self.store.data.settings.get("resume").context("No saved session")?.clone();
                self.request_playback(text(&resume,"id")?,true,resume["position"].as_f64().unwrap_or(0.))
            }
            "stream.prepare" => {
                let result = self.submit_job(&json!({"operation":"stream.prepare","params":p}))?;
                if p["preview_audio"] == true
                    && let Some(id) = p["track"]["id"].as_str()
                    && let Some(job) = result["job_id"].as_str()
                {
                    self.preview_audio_job = Some((job.into(), id.into()));
                }
                Ok(result)
            },
            "player.toggle" => {
                if self.pending_play.is_some() {
                    Ok(self.snapshot())
                } else if self.player.has_loaded_audio() && !self.player.paused() && !self.player.finished() {
                    self.player.pause();
                    Ok(self.snapshot())
                } else {
                    self.resume_or_start()
                }
            },
            "lastfm.authorize"=>self.integrations.authorize(&self.client,p),
            "lastfm.finish"=>self.integrations.finish(&self.client),
            "lastfm.enable"=>self.integrations.enable(boolean(p,"enabled",true)),
            "lastfm.status"=>Ok(self.integrations.status()),
            "lastfm.disconnect"=>self.integrations.disconnect(),
            "google.import"=>self.submit_job(&json!({"operation":"google.import","params":p})),
            "extensions.status"=>Ok(json!({"themes":extensions::theme_report(&self.store.root)?,"plugins":plugins::report(&self.store.root)?})),
            "themes.import"=>extensions::import_theme(&self.store.root,&PathBuf::from(text(p,"path")?)),
            "plugins.list"=>Ok(json!(plugins::list(&self.store.root)?)),
            "plugins.import"=>Ok(json!(plugins::import(&self.store.root,&PathBuf::from(text(p,"path")?))?)),
            "plugins.enable"=>Ok(json!(plugins::set_enabled(&self.store.root,text(p,"id")?,boolean(p,"enabled",true))?)),
            "plugins.remove"=>{plugins::remove(&self.store.root,text(p,"id")?)?;Ok(json!(true))},
            "plugins.run"=>self.submit_job(&json!({"operation":"plugins.run","params":{"id":text(p,"id")?,"params":{"queue":self.store.data.queue,"input":p["params"]}}})),
            "queue.reorder"=>{
                let order:Vec<usize>=serde_json::from_value(p["order"].clone())?;let length=self.store.data.queue.tracks.len();
                let unique:std::collections::HashSet<_>=order.iter().copied().collect();ensure!(order.len()==length&&unique.len()==length&&order.iter().all(|i|*i<length),"Order must be a permutation of every queue index");
                let current=self.store.data.queue.index;let tracks=order.iter().map(|i|self.store.data.queue.tracks[*i].clone()).collect();self.store.data.queue.tracks=tracks;self.store.data.queue.index=current.and_then(|current|order.iter().position(|i|*i==current));self.store.data.queue.back_stack.clear();self.store.save()?;Ok(json!(self.store.data.queue))
            },
            "session.resume" => {
                let resume = self
                    .store
                    .data
                    .settings
                    .get("resume")
                    .context("No saved playback session")?
                    .clone();
                let id = resume["id"].as_str().context("No saved track")?;
                self.play_id(id)?;
                let position = resume["position"].as_f64().unwrap_or(0.);
                if position > 0. && (self.player.duration <= 0. || position < self.player.duration)
                {
                    self.player.seek(position)?;
                }
                Ok(self.player.snapshot())
            }
            "player.preview" => {
                let id = text(p, "id")?;
                let start = number(p, "start")?;
                let end = number(p, "end")?;
                ensure!(start >= 0. && end > start, "Invalid preview region");
                self.dispatch(&json!({"command":"player.play","params":{"id":id}}))?;
                self.player.seek(start)?;
                self.region_end = Some(end);
                Ok(self.player.snapshot())
            }
            "audio.waveform" => self.submit_job(&json!({"operation":"audio.waveform","params":p})),
            "audio.cover" => media::extract_cover(
                &PathBuf::from(text(p, "input")?),
                &PathBuf::from(text(p, "output")?),
            ),
            "audio.credits" => tools::json(
                "ffprobe",
                [
                    "-v",
                    "error",
                    "-show_format",
                    "-show_streams",
                    "-of",
                    "json",
                    text(p, "path")?,
                ],
            ),
            "collection.save" => {
                let id = text(p, "id")?;
                let saved = self
                    .store
                    .data
                    .settings
                    .entry("saved_collections".into())
                    .or_insert(json!({}));
                saved
                    .as_object_mut()
                    .context("Invalid collection storage")?
                    .insert(id.into(), p.clone());
                self.store.save()?;
                Ok(p.clone())
            }
            "collection.saved" => Ok(self
                .store
                .data
                .settings
                .get("saved_collections")
                .cloned()
                .unwrap_or(json!({}))),
            "collection.unsave" => {
                if let Some(saved) = self
                    .store
                    .data
                    .settings
                    .get_mut("saved_collections")
                    .and_then(Value::as_object_mut)
                {
                    saved.remove(text(p, "id")?);
                }
                self.store.save()?;
                Ok(json!(true))
            }
            "job.cancel" => self.jobs.cancel(text(p, "id")?),
            "jobs.cancel_all" => Ok(self.jobs.cancel_all()),
            "download.bulk" => {
                let ids: Vec<String> = serde_json::from_value(p["ids"].clone())?;
                let mut unique = std::collections::BTreeSet::new();
                let mut pending = Vec::new();
                let mut skipped = 0;
                let format = p["format"].as_str().unwrap_or("opus");
                ensure!(["opus", "mp3", "flac", "wav", "m4a"].contains(&format), "Unsupported download format");
                for id in ids {
                    if !unique.insert(id.clone()) { continue; }
                    let track = self.store.track(&id)?;
                    if track.platform == "local" || track.local_path.as_ref().is_some_and(|p| p.is_file()) { skipped += 1; } else { pending.push(id); }
                }
                ensure!(pending.len() <= self.jobs.room(), "Not enough room in the download queue");
                let mut jobs = Vec::new();
                for id in pending { jobs.push(self.submit_job(&json!({"operation":"download","params":{"id":id,"format":format}}))?); }
                Ok(json!({"jobs":jobs,"skipped":skipped}))
            },
            "music.search"
            | "music.home"
            | "music.charts"
            | "music.browse"
            | "music.mixes"
            | "recommendations.radio" => self.submit_job(&json!({"operation":command,"params":p})),
            "radio.personal"=>{
                let ids:Vec<_>=if p["kind"]=="favorites"{self.store.data.favorites.clone()}else{self.store.data.history.iter().rev().take(20).map(|h|h.track_id.clone()).collect()};
                let ids:Vec<_>=ids.into_iter().filter(|id|self.store.data.tracks.contains_key(id)).collect();ensure!(!ids.is_empty(),"Listen to or like a song first to start your personal radio");
                let id=&ids[rand::random_range(0..ids.len())];self.dispatch(&json!({"command":"radio.start","params":{"id":id}}))
            }
            "radio.start" => {
                let id=text(p,"id")?.to_owned();self.store.track(&id)?;
                let generation=uuid::Uuid::new_v4().to_string();
                self.radio=Some(RadioSession{seed:String::new(),generation:generation.clone(),continuation:None,fetching:true,retry_at:std::time::Instant::now()});
                self.submit_job(&json!({"operation":"radio.prepare","params":{"id":id,"generation":generation}}))
            }
            "queue.append" => {
                let tracks: Vec<String> = serde_json::from_value(p["tracks"].clone())?;
                for id in &tracks {
                    self.store.track(id)?;
                }
                self.store.data.queue.tracks.extend(tracks);
                self.store.save()?;
                Ok(json!(self.store.data.queue))
            }
            "presets.list" => Ok(self
                .store
                .data
                .settings
                .get("audio_presets")
                .cloned()
                .unwrap_or(json!({}))),
            "presets.save" => {
                let id = p["id"]
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                let effects: Effects = serde_json::from_value(p["effects"].clone())?;
                effects.validate()?;
                let entry = json!({"id":id,"name":text(p,"name")?,"effects":effects});
                let presets = self
                    .store
                    .data
                    .settings
                    .entry("audio_presets".into())
                    .or_insert(json!({}));
                presets
                    .as_object_mut()
                    .context("Invalid presets data")?
                    .insert(id.clone(), entry.clone());
                self.store.save()?;
                Ok(entry)
            }
            "presets.delete" => {
                let id = text(p, "id")?;
                if let Some(presets) = self
                    .store
                    .data
                    .settings
                    .get_mut("audio_presets")
                    .and_then(Value::as_object_mut)
                {
                    presets.remove(id);
                }
                self.store.save()?;
                Ok(json!(true))
            }
            "presets.apply" => {
                let id = text(p, "id")?;
                let preset = self
                    .store
                    .data
                    .settings
                    .get("audio_presets")
                    .and_then(|v| v.get(id))
                    .context("Unknown audio preset")?;
                let effects: Effects = serde_json::from_value(preset["effects"].clone())?;
                self.player.effects(effects.clone())?;
                self.store.data.effects = effects;
                self.store.save()?;
                Ok(self.player.snapshot())
            }
            "lyrics.yrc.parse" => Ok(json!(lyrics::parse_yrc(text(p, "text")?))),
            "lyrics.words" => lyrics::words_online(
                &self.client,
                text(p, "title")?,
                text(p, "artist")?,
                p["duration"].as_f64().unwrap_or(0.),
            ),
            "cache.prefetch" => self.submit_job(&json!({"operation":"cache.prefetch","params":p})),
            "oauth.refresh" => {
                let value = self.oauth.refresh(&self.client, text(p, "provider")?)?;
                vault::save(
                    &self.store.root.join("oauth.dpapi"),
                    &self.oauth.private_session(),
                )?;
                Ok(value)
            }
            "discord.connect" => {self.store.data.settings.insert("discord".into(),json!({"enabled":true,"application_id":text(p,"application_id")?}));self.store.save()?;self.discord_update=std::time::Instant::now();self.presence.connect(text(p, "application_id")?)},
            "discord.update" => self.presence.update(p),
            "discord.clear" => self.presence.clear(),
            "discord.status" => Ok(self.presence.status()),
            "discord.disconnect" => {
                self.store.data.settings.insert("discord".into(),json!({"enabled":false}));self.store.save()?;
                self.presence.close();
                Ok(self.presence.status())
            }
            "job.submit" => self.submit_job(p),
            "job.get" => {
                self.poll_jobs();
                self.jobs.get(text(p, "id")?)
            }
            "job.list" => {
                self.poll_jobs();
                Ok(self.jobs.list())
            }
            "capabilities" => Ok(
                json!({"protocol_version":1,"ui":true,"runtime":"rust","external_tools":["ffmpeg","ffprobe","yt-dlp"],"commands":COMMANDS,"parity":"in_progress","unsupported":["legacy_js_plugins","css_rendering"],"distribution":{"installer":true,"update_staging":true,"release_repo_configured":self.store.data.settings.get("release_repo").and_then(Value::as_str).is_some_and(|s|!s.is_empty()),"signed":null,"signature_information":"See the release signing-status.json"},"requires_live_validation":["oauth_accounts","lastfm_account","discord_account","browser_cookie_extraction","release_end_to_end","regional_catalogs"]}),
            ),
            "tools.status" => Ok(tools::status()),
            "tools.install" => match text(p, "name")? {
                "yt-dlp" => tools::install_ytdlp(&self.client),
                "ffmpeg" => tools::install_ffmpeg(&self.client),
                _ => anyhow::bail!("Managed installation supports yt-dlp and ffmpeg"),
            },
            "lastfm" => services::lastfm(&self.client, p),
            "translate" => translation::fetch(&self.client, &self.store.root,p),
            "themes.list" => Ok(json!(extensions::themes(&self.store.root)?)),
            "themes.save" => {
                let theme: extensions::Theme = serde_json::from_value(p.clone())?;
                extensions::save_theme(&self.store.root, &theme)?;
                Ok(json!(theme))
            }
            "themes.delete" => {
                extensions::delete_theme(&self.store.root, text(p, "id")?)?;
                Ok(json!(true))
            }
            "library.get" => Ok(serde_json::to_value(&self.store.data)?),
            "library.scan" => {
                let paths: Vec<PathBuf> = serde_json::from_value(p["paths"].clone())?;
                let result = media::scan(&paths);
                for t in &result.tracks {
                    self.store.data.tracks.insert(t.id.clone(), t.clone());
                }
                self.store.save()?;
                Ok(serde_json::to_value(result)?)
            }
            "library.add" => {
                let track: Track = serde_json::from_value(p["track"].clone())?;
                ensure!(
                    !track.id.is_empty() && !track.title.is_empty(),
                    "Track needs id and title"
                );
                ensure!(
                    track.duration.is_finite() && track.duration >= 0.,
                    "Invalid track duration"
                );
                if track.local_path.is_none() {
                    if track.platform=="spotify" {
                        let id=spotify_public::entity_id(&track.source,"track")?;
                        ensure!(track.id==format!("spotify:{id}"),"Spotify track identity does not match its URL");
                    } else { provider::validate_url(&track.source)?; }
                }
                self.store
                    .data
                    .tracks
                    .insert(track.id.clone(), track.clone());
                self.store.save()?;
                Ok(json!(track))
            }
            "library.remove" => {
                let id = text(p, "id")?;
                if self.player.track_id.as_deref() == Some(id) {
                    self.player.stop();
                }
                self.store.remove(id)?;
                Ok(json!({"removed":id}))
            }
            "library.search" => {
                let q = text(p, "query")?.to_lowercase();
                let mut tracks: Vec<_> = self
                    .store
                    .data
                    .tracks
                    .values()
                    .filter(|t| {
                        format!("{} {} {}", t.title, t.artist, t.album)
                            .to_lowercase()
                            .contains(&q)
                    })
                    .collect();
                match p["sort"].as_str().unwrap_or("title") {
                    "artist" => tracks.sort_by_key(|t| t.artist.to_lowercase()),
                    "duration" => tracks.sort_by(|a, b| a.duration.total_cmp(&b.duration)),
                    _ => tracks.sort_by_key(|t| t.title.to_lowercase()),
                }
                Ok(json!(tracks))
            }
            "favorites.set" => {
                let id = text(p, "id")?;
                self.store.track(id)?;
                self.store.data.favorites.retain(|x| x != id);
                if boolean(p, "enabled", true) {
                    self.store.data.favorites.push(id.into());
                }
                self.store.save()?;
                Ok(json!(self.store.data.favorites))
            }
            "dislikes.set" => {
                let id = text(p, "id")?;
                self.store.track(id)?;
                self.store.data.dislikes.retain(|x| x != id);
                if boolean(p, "enabled", true) {
                    self.store.data.dislikes.push(id.into());
                    // A song cannot be both loved and rejected; it also leaves the upcoming songs and the picks held in reserve.
                    self.store.data.favorites.retain(|x| x != id);
                    self.smart_pool.retain(|t| t.id != id);
                    let current = self.store.data.queue.index;
                    let queue = &mut self.store.data.queue;
                    let mut index = 0;
                    queue.tracks.retain(|t| {
                        let keep = t != id || current == Some(index);
                        index += 1;
                        keep
                    });
                }
                self.store.save()?;
                Ok(json!(self.store.data.dislikes))
            }
            "history.clear" => {
                self.store.data.history.clear();
                self.store.save()?;
                Ok(json!(true))
            }
            "mix.apply" => {
                let tracks = mixes::validate_tracks(&p["tracks"])?;
                let name = text(p,"name")?.trim();
                ensure!(!name.is_empty() && name.chars().count() <= 200, "Invalid mix name");
                let play = boolean(p,"play",false);
                let mut ids = Vec::with_capacity(tracks.len());
                for track in tracks {
                    ids.push(track.id.clone());
                    self.store.data.tracks.entry(track.id.clone()).or_insert(track);
                }
                if play {
                    self.dispatch(&json!({"command":"queue.play","params":{"tracks":ids,"index":0}}))
                } else {
                    self.dispatch(&json!({"command":"playlist.save","params":{"tracks":ids,"name":name}}))
                }
            }
            "playlist.save" => {
                let id = p["id"]
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                let tracks: Vec<String> = serde_json::from_value(p["tracks"].clone())?;
                for id in &tracks {
                    self.store.track(id)?;
                }
                let playlist = Playlist {
                    id: id.clone(),
                    name: text(p, "name")?.into(),
                    tracks,
                };
                self.store.data.playlists.insert(id.clone(), playlist.clone());
                if !self.store.data.playlist_order.contains(&id) {
                    self.store.data.playlist_order.push(id);
                }
                self.store.save()?;
                Ok(json!(playlist))
            }
            "playlist.reorder" => {
                let wanted: Vec<String> = serde_json::from_value(p["ids"].clone())?;
                let mut order = Vec::new();
                for id in wanted.into_iter().chain(self.store.data.ordered_playlists().iter().map(|p| p.id.clone()).collect::<Vec<_>>()) {
                    if self.store.data.playlists.contains_key(&id) && !order.contains(&id) {
                        order.push(id);
                    }
                }
                self.store.data.playlist_order = order.clone();
                self.store.save()?;
                Ok(json!(order))
            }
            "playlist.delete" => {
                let id = text(p, "id")?;
                ensure!(
                    self.store.data.playlists.remove(id).is_some(),
                    "Unknown playlist"
                );
                self.store.data.playlist_order.retain(|x| x != id);
                self.store.save()?;
                Ok(json!(true))
            }
            "queue.set" => {
                self.smart_pool.clear();
                self.smart_picks.clear();
                self.radio = None;
                let tracks: Vec<String> = serde_json::from_value(p["tracks"].clone())?;
                for id in &tracks {
                    self.store.track(id)?;
                }
                let index = p["index"].as_u64().unwrap_or(0) as usize;
                ensure!(
                    tracks.is_empty() || index < tracks.len(),
                    "Queue index out of bounds"
                );
                self.player.stop();
                self.store.data.queue.tracks = tracks;
                self.store.data.queue.index =
                    (!self.store.data.queue.tracks.is_empty()).then_some(index);
                self.store.data.queue.back_stack.clear();
                self.store.save()?;
                if boolean(p, "play", false) {
                    let id = self
                        .store
                        .data
                        .queue
                        .current()
                        .context("Queue is empty")?
                        .to_string();
                    self.play_id(&id)?;
                }
                Ok(json!(self.store.data.queue))
            }
            "queue.mode" => {
                if let Some(mode) = p.get("shuffle_mode") {
                    let mode = mode.as_str().context("Invalid shuffle mode")?;
                    ensure!(["off", "classic", "ordered", "smart"].contains(&mode), "Invalid shuffle mode");
                    self.cancel_autoplay();
                    self.smart_pool.clear();
                    self.store.data.queue.shuffle = mode != "off";
                    self.store.data.settings.insert("shuffle_mode".into(), json!(mode));
                } else if let Some(shuffle) = p["shuffle"].as_bool() {
                    self.cancel_autoplay();
                    self.smart_pool.clear();
                    self.store.data.settings.insert("shuffle_mode".into(), json!(if shuffle {"classic"} else {"off"}));
                }
                if let Some(repeat) = p.get("repeat") {
                    self.store.data.queue.repeat = serde_json::from_value(repeat.clone())?;
                }
                if let Some(shuffle) = p["shuffle"].as_bool() {
                    self.store.data.queue.shuffle = shuffle;
                }
                self.store.save()?;
                Ok(json!(self.store.data.queue))
            }
            "player.play" => {
                self.radio = None;
                self.region_end = None;
                let id = text(p, "id")?.to_string();
                self.store.track(&id)?;
                let index = self
                    .store
                    .data
                    .queue
                    .tracks
                    .iter()
                    .position(|x| x == &id)
                    .unwrap_or_else(|| {
                        self.store.data.queue.tracks.push(id.clone());
                        self.store.data.queue.tracks.len() - 1
                    });
                self.store.data.queue.index = Some(index);
                self.play_id(&id)?;
                Ok(self.player.snapshot())
            }
            "player.pause" => {
                self.player.pause();
                Ok(self.player.snapshot())
            }
            "player.resume" => self.resume_or_start(),
            "player.stop" => {
                self.radio = None;
                self.region_end = None;
                self.player.stop();
                Ok(self.player.snapshot())
            }
            "player.seek" => {
                let seconds = number(p, "seconds")?;
                if self.pending_play.is_none() && self.player.has_loaded_audio() {
                    self.player.seek(seconds)?;
                }
                Ok(self.snapshot())
            }
            "player.next" | "player.previous" => {
                let id = if command == "player.next" {
                    self.advance_queue(false)
                } else {
                    self.store.data.queue.previous().map(str::to_owned)
                };
                if let Some(id) = id {
                    self.play_id(&id)?;
                } else {
                    self.player.stop();
                    self.store.save()?;
                }
                Ok(self.player.snapshot())
            }
            "player.state" => Ok(self.snapshot()),
            "player.analysis" => Ok(self.player.analysis()),
            "player.effects" => {
                let effects: Effects = serde_json::from_value(p.clone())?;
                effects.validate()?;
                self.player.effects(effects.clone())?;
                self.store.data.effects = effects;
                self.store.save()?;
                Ok(self.player.snapshot())
            }
            "player.devices" => Player::devices(),
            "player.device" => {
                self.player.select_device(p["name"].as_str())?;
                self.store.data.settings.insert("audio_device".into(),p["name"].clone());self.store.save()?;
                Ok(self.player.snapshot())
            }
            "player.sleep" => {
                let seconds = number(p, "seconds")?;
                ensure!(
                    (0.0..=86400.).contains(&seconds),
                    "Sleep timer must be 0..86400 seconds"
                );
                self.sleep_deadline = if seconds == 0. {
                    None
                } else {
                    Some(std::time::Instant::now() + Duration::from_secs_f64(seconds))
                };
                Ok(json!({"seconds":seconds}))
            }
            "search" => Ok(json!(self.providers.search(
                text(p, "query")?,
                p["platform"].as_str().unwrap_or("youtube"),
                p["limit"].as_u64().unwrap_or(20) as usize
            )?)),
            "stream.resolve" => self.providers.resolve(self.store.track(text(p, "id")?)?),
            "collection.fetch" => self.providers.collection(
                text(p, "url")?,
                p["limit"].as_u64().unwrap_or(1000) as usize,
            ),
            "collection.import" => {
                let result = self.providers.collection(
                    text(p, "url")?,
                    p["limit"].as_u64().unwrap_or(1000) as usize,
                )?;
                let tracks: Vec<Track> = serde_json::from_value(result["tracks"].clone())?;
                let playlist = Playlist {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: result["name"].as_str().unwrap_or("Imported").into(),
                    tracks: tracks.iter().map(|t| t.id.clone()).collect(),
                };
                for t in tracks {
                    self.store.data.tracks.insert(t.id.clone(), t);
                }
                self.store
                    .data
                    .playlists
                    .insert(playlist.id.clone(), playlist.clone());
                self.store.save()?;
                Ok(json!(playlist))
            }
            "catalog.metadata" => catalog::metadata_with_proxy(&self.client,text(p,"url")?,self.oauth.token("spotify").ok(),self.providers.proxy.as_deref()),
            "catalog.import" => {
                let source=text(p,"url")?;let metadata=catalog::metadata_with_proxy(&self.client,source,self.oauth.token("spotify").ok(),self.providers.proxy.as_deref())?;
                let plan=catalog::import_plan(&metadata,source,boolean(p,"allow_partial",false))?;
                let tracks:Vec<Track>=serde_json::from_value(plan["tracks"].clone())?;
                let playlist=Playlist{id:uuid::Uuid::new_v4().to_string(),name:plan["name"].as_str().unwrap_or("Imported playlist").into(),tracks:tracks.iter().map(|t|t.id.clone()).collect()};
                for track in tracks{self.store.data.tracks.insert(track.id.clone(),track);}
                self.store.data.playlists.insert(playlist.id.clone(),playlist.clone());self.store.save()?;
                let mut result=plan;result["playlist"]=json!(playlist);Ok(result)
            }
            "download" => {
                let id = text(p, "id")?.to_string();
                let track = self.store.track(&id)?.clone();
                ensure!(track.platform != "local", "Track is already local");
                let dir = p["directory"]
                    .as_str()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| self.store.root.join("downloads"));
                let path = self.providers.download(
                    &track,
                    &dir,
                    p["format"].as_str().unwrap_or("opus"),
                )?;
                self.store.data.tracks.get_mut(&id).unwrap().local_path = Some(path.clone());
                self.store.save()?;
                Ok(json!({"path":path}))
            }
            "offline.delete"=>{
                let id=text(p,"id")?.to_owned();let track=self.store.track(&id)?.clone();
                let path=track.local_path.context("Track has no downloaded copy")?;
                ensure!(track.platform!="local","Original local audio files are never deleted");
                if path.is_file(){let base=self.store.root.join("downloads").canonicalize()?;let file=path.canonicalize()?;ensure!(file.file_stem().and_then(|s|s.to_str())==Some(store::key(&id).as_str()),"File is not an app-generated download");ensure!(file.starts_with(&base)&&base.starts_with(self.store.root.canonicalize()?),"Only app-managed downloads can be deleted");if self.player.track_id.as_deref()==Some(&id){self.player.stop();}std::fs::remove_file(file)?;}
                self.store.data.tracks.get_mut(&id).context("Unknown track")?.local_path=None;self.store.save()?;Ok(json!({"deleted":true,"original_files_preserved":true}))
            }
            "offline.detach" => {
                let id = text(p, "id")?;
                let t = self
                    .store
                    .data
                    .tracks
                    .get_mut(id)
                    .context("Unknown track")?;
                ensure!(t.platform != "local", "Cannot detach a local library file");
                t.local_path = None;
                self.store.save()?;
                Ok(json!(true))
            }
            "audio.inspect" => Ok(json!(media::inspect(&PathBuf::from(text(p, "path")?))?)),
            "audio.encode" => media::encode(&serde_json::from_value(p.clone())?),
            "lyrics.get" => lyrics::fetch_source(
                &self.client,
                &self.store.root,
                text(p, "title")?,
                text(p, "artist")?,
                p["duration"].as_f64().unwrap_or(0.),
                boolean(p, "online", true),
                p["source"].as_str().unwrap_or("auto"),
            ),
            "lyrics.save" => lyrics::save(
                &self.store.root,
                text(p, "title")?,
                text(p, "artist")?,
                text(p, "text")?,
            ),
            "lyrics.parse" => Ok(json!(lyrics::parse(text(p, "text")?))),
            "lyrics.active" => {
                let lines: Vec<lyrics::Line> = serde_json::from_value(p["lines"].clone())?;
                Ok(
                    json!({"index":lyrics::active(&lines,number(p,"position")?,p["offset"].as_f64().unwrap_or(0.))}),
                )
            }
            "settings.get" => Ok(json!(self.store.data.settings)),
            "account.music.reset" => {
                // A different account is signing in: keep the old library in a backup file, then start clean.
                let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
                let backups = self.store.root.join("backups");
                std::fs::create_dir_all(&backups)?;
                self.store
                    .export(&backups.join(format!("library-before-account-switch-{stamp}.json")))?;
                let data = &mut self.store.data;
                data.favorites.clear();
                data.playlists.clear();
                data.playlist_order.clear();
                data.history.clear();
                data.tracks
                    .retain(|_, track| track.local_path.is_some() || track.platform == "local");
                self.store.save()?;
                Ok(json!(true))
            }
            "account.music.merge" => {
                let incoming: account_music::Snapshot = serde_json::from_value(p["snapshot"].clone())?;
                incoming.merge_into(&mut self.store.data);
                self.store.save()?;
                Ok(json!(true))
            }
            "settings.set" => {
                let values = p.as_object().context("Settings must be an object")?;
                for (k, v) in values {
                    self.store.data.settings.insert(k.clone(), v.clone());
                }
                self.store.save()?;
                Ok(json!(self.store.data.settings))
            }
            "backup.export" => {
                self.store.export(&PathBuf::from(text(p, "path")?))?;
                Ok(json!(true))
            }
            "backup.import" => {
                self.player.stop();
                self.store.import(&PathBuf::from(text(p, "path")?))?;
                Ok(json!(true))
            }
            "cache.status"=>cache::status(&self.store.root,self.cache_limit()),
            "cache.limit"=>{let mb=p["mb"].as_u64().context("Expected cache limit in MB")?;ensure!((64..=65536).contains(&mb),"Cache limit must be 64..65536 MB");self.store.data.settings.insert("cache_limit_mb".into(),json!(mb));self.prune_cache(self.cache_limit())},
            "cache.clear"=>self.prune_cache(0),
            "oauth.begin" => self.oauth.begin(
                text(p, "provider")?,
                serde_json::from_value(p["config"].clone())?,
            ),
            "oauth.finish" => {
                let value = self.oauth.finish(
                    &self.client,
                    text(p, "provider")?,
                    text(p, "state")?,
                    text(p, "code")?,
                )?;
                vault::save(
                    &self.store.root.join("oauth.dpapi"),
                    &self.oauth.private_session(),
                )?;
                Ok(value)
            }
            "oauth.adopt" => {
                let value = self.oauth.adopt(
                    text(p, "provider")?,
                    text(p, "access_token")?,
                    p["expires_in"].as_u64().unwrap_or(3600),
                )?;
                vault::save(
                    &self.store.root.join("oauth.dpapi"),
                    &self.oauth.private_session(),
                )?;
                Ok(value)
            }
            "oauth.status" => Ok(self.oauth.status(text(p, "provider")?)),
            "oauth.logout" => {
                let provider=text(p,"provider")?;
                ensure!(["google","spotify"].contains(&provider),"Unknown account provider");
                self.jobs.cancel_operations(if provider=="spotify" { &["spotify.library","spotify.import","spotify.search","catalog.import","catalog.metadata"] } else { &["google.library","google.import","music.mixes"] });
                self.oauth.logout(provider);
                vault::save(
                    &self.store.root.join("oauth.dpapi"),
                    &self.oauth.private_session(),
                )?;
                Ok(json!(true))
            }
            "google.library" => oauth::youtube_library(
                &self.client,
                self.oauth.token("google")?,
                text(p, "kind")?,
                p["playlist_id"].as_str(),
            ),
            "cookies.clear" => {
                self.search_cache.clear();self.providers.cookies=None;
                self.store.data.settings.remove("youtube_cookie_path");self.store.save()?;
                Ok(json!({"configured":false,"original_file_preserved":true}))
            }
            "cookies.set" => {
                let path = PathBuf::from(text(p, "path")?);
                ensure!(path.is_file(), "Cookie file missing");
                self.providers.cookies = Some(path.canonicalize()?);
                self.store
                    .data
                    .settings
                    .insert("youtube_cookie_path".into(), json!(self.providers.cookies));
                self.store.save()?;
                Ok(json!({"configured":true,"persistent":true}))
            }
            "diagnostics" => Ok(
                json!({"version":env!("CARGO_PKG_VERSION"),"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"data_dir":self.store.root,"track_count":self.store.data.tracks.len(),"player":self.player.snapshot(),"tools":tools::status()}),
            ),
            _ => anyhow::bail!("Unknown or unimplemented command: {command}"),
        }
    }
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .with_context(|| format!("Missing string: {key}"))
}
fn number(v: &Value, key: &str) -> Result<f64> {
    let n = v[key]
        .as_f64()
        .with_context(|| format!("Missing number: {key}"))?;
    ensure!(n.is_finite(), "Invalid number");
    Ok(n)
}
fn boolean(v: &Value, key: &str, fallback: bool) -> bool {
    v[key].as_bool().unwrap_or(fallback)
}
/// Called once the window is up: open connections and load FFmpeg so the first Play is not the first of everything.
pub fn warm_up() {
    std::thread::spawn(|| {
        youtube_stream::preconnect();
        let _ = tools::run("ffmpeg", ["-version"], Duration::from_secs(10));
    });
}

/// Resolve stream links for tracks the listener is likely to play next.
pub fn warm_streams(sources: Vec<String>) {
    youtube_stream::warm(sources);
}

pub const COMMANDS: &[&str] = &[
    "player.toggle",
    "lastfm.authorize",
    "lastfm.finish",
    "lastfm.enable",
    "lastfm.status",
    "lastfm.disconnect",
    "google.import",
    "plugins.list",
    "extensions.status",
    "themes.import",
    "cookies.clear",
    "plugins.import",
    "plugins.enable",
    "plugins.remove",
    "plugins.run",
    "queue.reorder",
    "session.resume",
    "player.preview",
    "audio.waveform",
    "audio.cover",
    "audio.credits",
    "collection.save",
    "collection.saved",
    "collection.unsave",
    "job.cancel",
    "music.search",
    "music.home",
    "music.charts",
    "music.browse",
    "recommendations.radio",
    "radio.start",
    "queue.append",
    "presets.list",
    "presets.save",
    "presets.delete",
    "presets.apply",
    "lyrics.yrc.parse",
    "lyrics.words",
    "cache.prefetch",
    "oauth.refresh",
    "discord.connect",
    "discord.update",
    "discord.clear",
    "discord.status",
    "discord.disconnect",
    "job.submit",
    "job.get",
    "job.list",
    "tools.install",
    "lastfm",
    "translate",
    "themes.list",
    "themes.save",
    "themes.delete",
    "capabilities",
    "tools.status",
    "library.get",
    "library.scan",
    "library.add",
    "library.remove",
    "library.search",
    "favorites.set",
    "dislikes.set",
    "history.clear",
    "playlist.save",
    "mix.apply",
    "music.mixes",
    "playlist.delete",
    "playlist.reorder",
    "queue.set",
    "queue.mode",
    "player.play",
    "app.install",
    "updates.rollback",
    "support.export",
    "updates.configure",
    "updates.check",
    "updates.stage",
    "appearance.set",
    "themes.apply",
    "network.status",
    "network.set",
    "network.test",
    "cookies.browser",
    "player.request",
    "player.next.request",
    "player.previous.request",
    "queue.play",
    "session.resume.request",
    "cache.limit",
    "stream.prepare",
    "spotify.search",
    "player.pause",
    "player.resume",
    "player.stop",
    "player.seek",
    "player.next",
    "player.previous",
    "player.state",
    "player.analysis",
    "player.effects",
    "player.devices",
    "player.device",
    "player.sleep",
    "search",
    "stream.resolve",
    "collection.fetch",
    "collection.import",
    "catalog.metadata",
    "catalog.import",
    "download",
    "download.bulk",
    "jobs.cancel_all",
    "offline.delete",
    "offline.detach",
    "audio.inspect",
    "audio.encode",
    "lyrics.get",
    "lyrics.save",
    "lyrics.parse",
    "lyrics.active",
    "settings.get",
    "settings.set",
    "backup.export",
    "backup.import",
    "cache.status",
    "cache.clear",
    "oauth.begin",
    "oauth.finish",
    "oauth.adopt",
    "oauth.status",
    "oauth.logout",
    "google.library",
    "cookies.set",
    "diagnostics",
];

#[cfg(test)]
mod radio_tests {
    use super::*;
    fn fake(id: &str) -> Track {
        serde_json::from_value(json!({"id":format!("youtube:{id}"),"title":id,"source":format!("https://www.youtube.com/watch?v={id}"),"platform":"youtube"})).unwrap()
    }
    fn wait(engine: &mut Engine, id: &str) {
        let start = std::time::Instant::now();
        loop {
            engine.poll_jobs();
            if engine.jobs.get(id).unwrap()["state"] != "running"
                && engine.jobs.get(id).unwrap()["state"] != "queued"
            {
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(2));
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    #[ignore = "Fetches radio and starts muted native playback"]
    fn smart_shuffle_fetches_and_plays_a_real_recommendation() {
        let dir = tempfile::tempdir().unwrap();
        let mut engine = Engine::open(dir.path().into()).unwrap();
        let track = fake("dQw4w9WgXcQ");
        let id = track.id.clone();
        for request in [
            json!({"command":"library.add","params":{"track":track}}),
            json!({"command":"queue.mode","params":{"shuffle_mode":"smart"}}),
            json!({"command":"player.effects","params":{"muted":true}}),
            json!({"command":"queue.play","params":{"tracks":[id]}}),
        ] {
            engine.dispatch(&request).unwrap();
        }
        let start = std::time::Instant::now();
        while engine.smart_pool.is_empty() {
            engine.tick().unwrap();
            assert!(
                start.elapsed() < Duration::from_secs(45),
                "No Smart Shuffle picks"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(engine.store.data.queue.tracks, vec![id.clone()]);
        let job = engine
            .dispatch(&json!({"command":"player.next.request"}))
            .unwrap();
        wait(&mut engine, job["job_id"].as_str().unwrap());
        let playing = engine.player.track_id.as_ref().unwrap();
        assert_ne!(playing, &id);
        assert!(engine.smart_picks.contains(playing));
        assert_eq!(engine.store.data.queue.tracks.len(), 2);
        engine.dispatch(&json!({"command":"player.stop"})).unwrap();
    }

    #[test]
    fn smart_shuffle_balances_artists_and_inserts_a_pick_after_three_tracks() {
        let dir = tempfile::tempdir().unwrap();
        let mut engine = Engine::open(dir.path().into()).unwrap();
        for (video, artist) in [
            ("abcdefghijk", "A"),
            ("lmnopqrstuv", "A"),
            ("123456789ab", "B"),
            ("cdefghijklm", "C"),
        ] {
            let mut track = fake(video);
            track.artist = artist.into();
            engine.store.data.queue.tracks.push(track.id.clone());
            engine.store.data.tracks.insert(track.id.clone(), track);
        }
        engine.store.data.queue.index = Some(0);
        engine
            .dispatch(&json!({"command":"queue.mode","params":{"shuffle_mode":"smart"}}))
            .unwrap();
        let recommended = fake("bcdefghijkl");
        engine.smart_pool.push(recommended.clone());
        let first = engine.advance_queue(true).unwrap();
        assert_ne!(engine.store.data.tracks[&first].artist, "A");
        let second = engine.advance_queue(false).unwrap();
        assert_ne!(first, second);
        assert_eq!(engine.advance_queue(true), Some(recommended.id.clone()));
        assert_eq!(engine.store.data.queue.previous(), Some(second.as_str()));
        assert!(engine.smart_picks.contains(&recommended.id));
        engine
            .dispatch(&json!({"command":"queue.mode","params":{"shuffle":true}}))
            .unwrap();
        assert!(!engine.smart_shuffle());
    }

    #[test]
    fn autoplay_appends_unique_tracks_and_ignores_a_replaced_queue() {
        let dir = tempfile::tempdir().unwrap();
        let mut engine = Engine::open(dir.path().into()).unwrap();
        let first = fake("abcdefghijk");
        let next = fake("lmnopqrstuv");
        engine
            .store
            .data
            .tracks
            .insert(first.id.clone(), first.clone());
        engine.store.data.queue.tracks = vec![first.id.clone()];
        engine.store.data.queue.index = Some(0);
        let result = json!({"tracks":[first, next.clone(), next.clone()]});
        let job = engine
            .jobs
            .submit("autoplay.fill".into(), None, move || Ok(result))
            .unwrap();
        let job = job["job_id"].as_str().unwrap().to_owned();
        engine.autoplay = Some(AutoplayRequest {
            smart: false,
            job: job.clone(),
            queue: engine.store.data.queue.tracks.clone(),
        });
        wait(&mut engine, &job);
        assert_eq!(
            engine.store.data.queue.tracks,
            vec![first.id.clone(), next.id.clone()]
        );
        assert_eq!(engine.store.data.queue.index, Some(0));

        let stale = fake("123456789ab");
        let result = json!({"tracks":[stale.clone()]});
        let job = engine
            .jobs
            .submit("autoplay.fill".into(), None, move || Ok(result))
            .unwrap();
        let job = job["job_id"].as_str().unwrap().to_owned();
        engine.autoplay = Some(AutoplayRequest {
            smart: false,
            job: job.clone(),
            queue: engine.store.data.queue.tracks.clone(),
        });
        engine
            .dispatch(&json!({"command":"queue.set","params":{"tracks":[next.id.clone()]}}))
            .unwrap();
        wait(&mut engine, &job);
        assert_eq!(engine.store.data.queue.tracks, vec![next.id]);
        assert!(!engine.store.data.tracks.contains_key(&stale.id));
    }

    #[test]
    fn radio_continuation_appends_without_reordering_or_repeating() {
        let dir = tempfile::tempdir().unwrap();
        let mut e = Engine::open(dir.path().into()).unwrap();
        let seed = fake("abcdefghijk");
        let first = fake("123456789ab");
        let second = fake("z123456789a");
        for t in [&seed, &first] {
            e.store.data.tracks.insert(t.id.clone(), t.clone());
        }
        e.store.data.queue.tracks = vec![seed.id.clone(), first.id.clone()];
        e.store.data.queue.index = Some(1);
        e.radio = Some(RadioSession {
            seed: seed.id.clone(),
            generation: "g".into(),
            continuation: Some("old".into()),
            fetching: true,
            retry_at: std::time::Instant::now(),
        });
        let response = json!({"seed":"abcdefghijk","tracks":[first,second],"generation":"g","append_to_queue":true,"extend":true,"continuation":"new"});
        let job = e
            .jobs
            .submit("recommendations.radio".into(), None, move || Ok(response))
            .unwrap();
        wait(&mut e, job["job_id"].as_str().unwrap());
        assert_eq!(
            e.store.data.queue.tracks,
            vec![
                seed.id,
                "youtube:123456789ab".into(),
                "youtube:z123456789a".into()
            ]
        );
        assert_eq!(e.store.data.queue.index, Some(1));
        assert_eq!(
            e.radio.as_ref().unwrap().continuation.as_deref(),
            Some("new")
        );
    }
    #[test]
    fn stale_radio_result_cannot_replace_user_queue() {
        let dir = tempfile::tempdir().unwrap();
        let mut e = Engine::open(dir.path().into()).unwrap();
        let first = fake("abcdefghijk");
        e.store.data.tracks.insert(first.id.clone(), first.clone());
        e.store.data.queue.tracks = vec![first.id.clone()];
        let response = json!({"seed":"abcdefghijk","tracks":[fake("123456789ab")],"generation":"stale","append_to_queue":true});
        let job = e
            .jobs
            .submit("recommendations.radio".into(), None, move || Ok(response))
            .unwrap();
        wait(&mut e, job["job_id"].as_str().unwrap());
        assert_eq!(e.store.data.queue.tracks, vec![first.id]);
        assert_eq!(e.store.data.tracks.len(), 1);
    }
}

#[cfg(test)]
mod transition_tests {
    use super::*;
    #[test]
    fn sleep_cancels_slow_pending_transition_and_restores_cursor() {
        let root = tempfile::tempdir().unwrap();
        let mut e = Engine::open(root.path().into()).unwrap();
        e.store.data.queue.tracks = vec!["current".into(), "next".into()];
        e.store.data.queue.index = Some(1);
        let job = e
            .jobs
            .submit("playback.prepare".into(), None, || {
                std::thread::sleep(Duration::from_millis(100));
                Ok(json!({}))
            })
            .unwrap();
        e.pending_play = Some(PendingPlayback {
            job: job["job_id"].as_str().unwrap().into(),
            id: "next".into(),
            select_queue: false,
            position: 0.,
            record_history: true,
            paused: false,
            cursor_before: Some(QueueCursor {
                index: Some(0),
                back_stack: vec![],
            }),
        });
        e.dispatch(&json!({"command":"player.sleep","params":{"seconds":0.01}}))
            .unwrap();
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(e.tick().unwrap().unwrap()["event"], "sleep_timer");
        assert!(e.pending_play.is_none());
        assert_eq!(e.store.data.queue.index, Some(0));
        std::thread::sleep(Duration::from_millis(100));
        e.poll_jobs();
        assert_eq!(
            e.jobs.get(job["job_id"].as_str().unwrap()).unwrap()["state"],
            "cancelled"
        );
    }
}
