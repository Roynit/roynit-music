use crate::Request;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use riff_core::model::Track;
use serde_json::{Value, json};
use souvlaki::*;
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

pub struct Session {
    controls: MediaControls,
    signature: String,
    updated: Instant,
}
impl Session {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        sender: mpsc::Sender<Request>,
    ) -> Result<Self, String> {
        let hwnd = match cc.window_handle().map_err(|e| e.to_string())?.as_raw() {
            RawWindowHandle::Win32(h) => Some(h.hwnd.get() as *mut std::ffi::c_void),
            _ => None,
        };
        let mut controls = MediaControls::new(PlatformConfig {
            dbus_name: "riff",
            display_name: "Roy Music",
            hwnd,
        })
        .map_err(|e| format!("{e:?}"))?;
        controls
            .attach(move |event| {
                let (command, params) = match event {
                    MediaControlEvent::Play => ("player.resume", json!({})),
                    MediaControlEvent::Pause => ("player.pause", json!({})),
                    MediaControlEvent::Toggle => ("player.toggle", json!({})),
                    MediaControlEvent::Next => ("player.next", json!({})),
                    MediaControlEvent::Previous => ("player.previous", json!({})),
                    MediaControlEvent::Stop => ("player.stop", json!({})),
                    MediaControlEvent::SetPosition(p) => {
                        ("player.seek", json!({"seconds":p.0.as_secs_f64()}))
                    }
                    _ => return,
                };
                let _ = sender.send(Request {
                    tag: "system-media".into(),
                    command: command.into(),
                    params,
                });
            })
            .map_err(|e| format!("{e:?}"))?;
        Ok(Self {
            controls,
            signature: String::new(),
            updated: Instant::now() - Duration::from_secs(2),
        })
    }
    pub fn update(&mut self, track: Option<&Track>, state: &Value) -> Result<(), String> {
        let duration = state["duration"].as_f64().unwrap_or(0.).max(0.);
        let paused =
            state["paused"] != false || state["playing"] != true || state["buffering"] == true;
        let signature = format!("{:?}|{paused}|{duration}", track.map(|t| &t.id));
        if self.signature == signature && self.updated.elapsed() < Duration::from_secs(1) {
            return Ok(());
        }
        if self.signature != signature {
            self.controls
                .set_metadata(MediaMetadata {
                    title: track.map(|t| t.title.as_str()),
                    artist: track.map(|t| t.artist.as_str()),
                    album: track.map(|t| t.album.as_str()),
                    cover_url: track.and_then(|t| t.cover.as_deref()),
                    duration: Some(Duration::from_secs_f64(duration)),
                })
                .map_err(|e| format!("{e:?}"))?;
        }
        let progress = Some(MediaPosition(Duration::from_secs_f64(
            state["position"].as_f64().unwrap_or(0.).max(0.),
        )));
        self.controls
            .set_playback(if track.is_none() {
                MediaPlayback::Stopped
            } else if paused {
                MediaPlayback::Paused { progress }
            } else {
                MediaPlayback::Playing { progress }
            })
            .map_err(|e| format!("{e:?}"))?;
        self.signature = signature;
        self.updated = Instant::now();
        Ok(())
    }
}
