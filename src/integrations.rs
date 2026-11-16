use crate::model::Track;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
pub struct ListeningTracker {
    current: Option<String>,
    pub listened: f64,
    announced: bool,
    scrobbled: bool,
}
impl ListeningTracker {
    pub fn observe(
        &mut self,
        id: Option<&str>,
        duration: f64,
        playing: bool,
        delta: f64,
    ) -> (bool, bool) {
        if self.current.as_deref() != id {
            *self = Self {
                current: id.map(str::to_string),
                ..Default::default()
            };
        }
        if id.is_none() || !playing {
            return (false, false);
        }
        self.listened += delta.clamp(0., 2.);
        let announce = !self.announced;
        self.announced = true;
        let scrobble =
            duration > 30. && self.listened >= ((duration / 2.).min(240.)) && !self.scrobbled;
        if scrobble {
            self.scrobbled = true;
        }
        (announce, scrobble)
    }
}
pub struct Integrations {
    root: PathBuf,
    secrets: Value,
    tracker: ListeningTracker,
    last_tick: Instant,
    started: u64,
    pub last_error: Option<String>,
}
impl Integrations {
    pub fn open(root: PathBuf) -> Self {
        let mut last_error = None;
        let secrets = match crate::vault::load(&root.join("integrations.dpapi")) {
            Ok(Some(v)) => v,
            Ok(None) => json!({}),
            Err(e) => {
                last_error = Some(e.to_string());
                json!({})
            }
        };
        Self {
            root,
            secrets,
            tracker: ListeningTracker::default(),
            last_tick: Instant::now(),
            started: 0,
            last_error,
        }
    }
    fn save(&self) -> Result<()> {
        crate::vault::save(&self.root.join("integrations.dpapi"), &self.secrets)
    }
    pub fn status(&self) -> Value {
        json!({"configured":self.secrets["lastfm"]["api_key"].is_string(),"signed_in":self.secrets["lastfm"]["session_key"].is_string(),"enabled":self.secrets["lastfm"]["enabled"]==true,"persistent":crate::vault::SUPPORTED,"username":self.secrets["lastfm"]["username"],"last_error":self.last_error})
    }
    pub fn authorize(&mut self, client: &reqwest::blocking::Client, p: &Value) -> Result<Value> {
        let key = p["api_key"]
            .as_str()
            .filter(|s| !s.is_empty())
            .context("Last.fm API key is required")?;
        let secret = p["api_secret"]
            .as_str()
            .filter(|s| !s.is_empty())
            .context("Last.fm API secret is required")?;
        let result = crate::services::lastfm(
            client,
            &json!({"operation":"token","api_key":key,"api_secret":secret}),
        )?;
        self.secrets["lastfm"] =
            json!({"api_key":key,"api_secret":secret,"token":result["token"],"enabled":false});
        self.save()?;
        Ok(result)
    }
    pub fn finish(&mut self, client: &reqwest::blocking::Client) -> Result<Value> {
        let mut p = self.secrets["lastfm"].clone();
        ensure!(p["token"].is_string(), "Start Last.fm authorization first");
        p["operation"] = json!("session");
        let result = crate::services::lastfm(client, &p)?;
        ensure!(
            result["session"]["key"].is_string(),
            "Last.fm did not grant a session"
        );
        self.secrets["lastfm"]["session_key"] = result["session"]["key"].clone();
        self.secrets["lastfm"]["username"] = result["session"]["name"].clone();
        self.secrets["lastfm"]["enabled"] = json!(true);
        self.secrets["lastfm"]
            .as_object_mut()
            .unwrap()
            .remove("token");
        self.tracker = ListeningTracker::default();
        self.save()?;
        Ok(self.status())
    }
    pub fn enable(&mut self, enabled: bool) -> Result<Value> {
        ensure!(
            !enabled || self.secrets["lastfm"]["session_key"].is_string(),
            "Authorize Last.fm first"
        );
        if !self.secrets["lastfm"].is_object() {
            self.secrets["lastfm"] = json!({});
        }
        self.secrets["lastfm"]["enabled"] = json!(enabled);
        self.tracker = ListeningTracker::default();
        self.save()?;
        Ok(self.status())
    }
    pub fn disconnect(&mut self) -> Result<Value> {
        self.secrets
            .as_object_mut()
            .context("Invalid integration storage")?
            .remove("lastfm");
        self.tracker = ListeningTracker::default();
        self.save()?;
        Ok(self.status())
    }
    pub fn sample(&mut self, track: Option<&Track>, snapshot: &Value) -> Vec<Value> {
        let delta = self.last_tick.elapsed().as_secs_f64();
        self.last_tick = Instant::now();
        if self.secrets["lastfm"]["enabled"] != true {
            return vec![];
        }
        let playing = snapshot["playing"] == true && snapshot["buffering"] == false;
        let previous = self.tracker.current.clone();
        let (announce, scrobble) = self.tracker.observe(
            track.map(|t| t.id.as_str()),
            track
                .map_or(0., |t| t.duration)
                .max(snapshot["duration"].as_f64().unwrap_or(0.)),
            playing,
            delta,
        );
        if previous != self.tracker.current {
            self.started = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
        }
        let Some(track) = track else {
            return vec![];
        };
        if track.artist.trim().is_empty() {
            return vec![];
        }
        let mut jobs = Vec::new();
        for (operation, send) in [("now_playing", announce), ("scrobble", scrobble)] {
            if send {
                let mut p = self.secrets["lastfm"].clone();
                p["operation"] = json!(operation);
                p["artist"] = json!(track.artist);
                p["track"] = json!(track.title);
                p["album"] = json!(track.album);
                p["timestamp"] = json!(self.started);
                jobs.push(p);
            }
        }
        jobs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scrobble_counts_listening_not_pauses_or_seeks() {
        let mut t = ListeningTracker::default();
        assert_eq!(t.observe(Some("a"), 100., true, 1.), (true, false));
        for _ in 0..48 {
            assert!(!t.observe(Some("a"), 100., true, 1.).1);
        }
        assert!(!t.observe(Some("a"), 100., false, 100.).1);
        assert!(t.observe(Some("a"), 100., true, 1.).1);
        assert!(!t.observe(Some("a"), 100., true, 2.).1);
        assert_eq!(t.observe(Some("b"), 20., true, 2.), (true, false));
        for _ in 0..100 {
            assert!(!t.observe(Some("b"), 20., true, 2.).1);
        }
    }
}
