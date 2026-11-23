use crate::{media, model::Effects, tools};
use anyhow::{Context, Result, ensure};
use cpal::traits::{DeviceTrait, HostTrait};
use rodio::{OutputStream, OutputStreamBuilder, Sink, Source};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::{BufReader, Read},
    process::{Child, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[derive(Default)]
struct Meter {
    samples: VecDeque<f32>,
    peak: f32,
    buffering: bool,
}
struct PcmSource {
    child: Arc<Mutex<Child>>,
    receiver: mpsc::Receiver<Vec<f32>>,
    block: std::vec::IntoIter<f32>,
    meter: Arc<Mutex<Meter>>,
    clock: Arc<AtomicU64>,
}
impl Iterator for PcmSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let sample = match self.block.next() {
            Some(s) => s,
            None => {
                let (block, buffering) = match self.receiver.try_recv() {
                    Ok(block) => {
                        self.clock.fetch_add(block.len() as u64, Ordering::Relaxed);
                        (block, false)
                    }
                    Err(mpsc::TryRecvError::Empty) => (vec![0.; 2048], true),
                    Err(mpsc::TryRecvError::Disconnected) => return None,
                };
                if let Ok(mut meter) = self.meter.try_lock() {
                    meter.buffering = buffering;
                    for sample in &block {
                        if meter.samples.len() >= 2048 {
                            meter.samples.pop_front();
                        }
                        meter.samples.push_back(*sample);
                        meter.peak = meter.peak.max(sample.abs());
                    }
                }
                self.block = block.into_iter();
                self.block.next()?
            }
        };
        Some(sample)
    }
}
impl Source for PcmSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        48000
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}
impl Drop for PcmSource {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
}

struct PreparedAudio {
    input: String,
    headers: Vec<(String, String)>,
    proxy: Option<String>,
    filter: String,
    expires: Instant,
    source: PcmSource,
    error: Arc<Mutex<String>>,
}

fn open_decoder(
    input: &str,
    headers: &[(String, String)],
    proxy: Option<&str>,
    filter: &str,
    seconds: f64,
) -> Result<(PcmSource, Arc<Mutex<String>>)> {
    let mut command = tools::command("ffmpeg");
    command.args(["-v", "error", "-nostdin"]);
    if input.starts_with("http") {
        if let Some(proxy) = proxy {
            command.args(["-http_proxy", proxy]);
        }
        command.args([
            "-rw_timeout",
            "8000000",
            "-reconnect",
            "1",
            "-reconnect_streamed",
            "1",
            "-reconnect_on_network_error",
            "1",
            "-reconnect_delay_max",
            "3",
        ]);
        if !headers.is_empty() {
            let h: String = headers
                .iter()
                .filter(|(k, v)| !k.contains(['\r', '\n']) && !v.contains(['\r', '\n']))
                .map(|(k, v)| format!("{k}: {v}\r\n"))
                .collect();
            command.args(["-headers", &h]);
        }
    }
    let mut child = command
        .args([
            "-ss",
            &seconds.to_string(),
            "-i",
            input,
            "-vn",
            "-af",
            filter,
            "-f",
            "f32le",
            "-acodec",
            "pcm_f32le",
            "-ar",
            "48000",
            "-ac",
            "2",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Cannot start FFmpeg audio decoder")?;
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut stderr = child.stderr.take().unwrap();
    let child = Arc::new(Mutex::new(child));
    let error = Arc::new(Mutex::new(String::new()));
    // Drain diagnostics continuously with bounded retention; a recovered warning is not failure.
    let diagnostics = std::thread::spawn(move || {
        let mut retained = Vec::new();
        let mut block = [0u8; 4096];
        while let Ok(n) = stderr.read(&mut block) {
            if n == 0 {
                break;
            }
            let room = 16_384usize.saturating_sub(retained.len());
            retained.extend_from_slice(&block[..n.min(room)]);
        }
        String::from_utf8_lossy(&retained).into_owned()
    });
    // About twenty seconds of decoded sound (2048 blocks of 4 KiB = 8 MiB) is kept ahead of the speakers. The
    // old eight blocks held under a tenth of a second, so any network or scheduling hiccup became a freeze.
    let (sender, receiver) = mpsc::sync_channel(2048);
    let decoder_child = child.clone();
    let decoder_error = error.clone();
    std::thread::spawn(move || {
        loop {
            let mut block = vec![0; 4096];
            let mut n = 0;
            while n < block.len() {
                match stdout.read(&mut block[n..]) {
                    Ok(0) | Err(_) => break,
                    Ok(m) => n += m,
                }
            }
            if n == 0 {
                break;
            }
            let samples = block[..n]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|s| f32::from_le_bytes(*s))
                .collect();
            if sender.send(samples).is_err() {
                break;
            }
        }
        // Do not hold the child mutex across wait(): Stop/Drop must be able to kill it.
        let status = loop {
            let result = decoder_child
                .lock()
                .ok()
                .and_then(|mut c| c.try_wait().ok());
            match result {
                Some(Some(status)) => break Some(status),
                Some(None) => std::thread::sleep(Duration::from_millis(10)),
                None => break None,
            }
        };
        let diagnostic = diagnostics.join().unwrap_or_default();
        if let Ok(mut error) = decoder_error.lock() {
            *error = terminal_error(status.is_some_and(|s| s.success()), &diagnostic);
        }
        // Publish terminal state before EOF: Engine must never advance past an unreported failure.
        drop(sender);
    });
    let source = PcmSource {
        child,
        receiver,
        block: Vec::new().into_iter(),
        meter: Arc::new(Mutex::new(Meter {
            buffering: true,
            ..Default::default()
        })),
        clock: Arc::new(AtomicU64::new(0)),
    };
    Ok((source, error))
}

pub struct Player {
    stream: Option<OutputStream>,
    prepared: [Option<PreparedAudio>; 2],
    sink: Option<Sink>,
    outgoing: Option<(Sink, Instant, f64)>,
    input: Option<String>,
    headers: Vec<(String, String)>,
    pub track_id: Option<String>,
    offset: f64,
    pub duration: f64,
    pub playback_source: Value,
    pub proxy: Option<String>,
    effects: Effects,
    meter: Arc<Mutex<Meter>>,
    error: Arc<Mutex<String>>,
    clock: Arc<AtomicU64>,
}
impl Default for Player {
    fn default() -> Self {
        Self {
            stream: None,
            prepared: [None, None],
            sink: None,
            outgoing: None,
            input: None,
            headers: vec![],
            track_id: None,
            offset: 0.,
            duration: 0.,
            playback_source: Value::Null,
            proxy: None,
            effects: Effects::default(),
            meter: Arc::new(Mutex::new(Meter::default())),
            error: Arc::new(Mutex::new(String::new())),
            clock: Arc::new(AtomicU64::new(0)),
        }
    }
}
impl Player {
    pub(crate) fn expire_prepared_audio(&mut self) {
        for slot in &mut self.prepared {
            if slot
                .as_ref()
                .is_some_and(|audio| audio.expires <= Instant::now() || audio.proxy != self.proxy)
            {
                *slot = None;
            }
        }
    }

    pub(crate) fn prepare_audio(
        &mut self,
        input: String,
        headers: Vec<(String, String)>,
        effects: &Effects,
        preview: bool,
    ) -> Result<()> {
        let filter = media::filter(effects)?;
        if self.prepared.iter().flatten().any(|audio| {
            audio.input == input
                && audio.headers == headers
                && audio.proxy == self.proxy
                && audio.filter == filter
                && audio.expires.saturating_duration_since(Instant::now()) > Duration::from_secs(22)
                && audio.error.lock().is_ok_and(|error| error.is_empty())
        }) {
            return Ok(());
        }
        let slot = &mut self.prepared[usize::from(preview)];
        *slot = None;
        let (source, error) = open_decoder(&input, &headers, self.proxy.as_deref(), &filter, 0.)?;
        *slot = Some(PreparedAudio {
            input,
            headers,
            proxy: self.proxy.clone(),
            filter,
            expires: Instant::now() + Duration::from_secs(35),
            source,
            error,
        });
        Ok(())
    }

    pub fn devices() -> Result<Value> {
        let host = cpal::default_host();
        let default = host.default_output_device().and_then(|d| d.name().ok());
        let devices: Vec<_> = host
            .output_devices()?
            .filter_map(|d| d.name().ok())
            .collect();
        Ok(json!({"default":default,"devices":devices}))
    }
    pub fn select_device(&mut self, name: Option<&str>) -> Result<()> {
        let was_paused = self.paused();
        let pos = self.position();
        let stream = if let Some(name) = name {
            let device = cpal::default_host()
                .output_devices()?
                .find(|d| d.name().is_ok_and(|n| n == name))
                .context("Audio output device not found")?;
            OutputStreamBuilder::from_device(device)?.open_stream_or_fallback()?
        } else {
            OutputStreamBuilder::open_default_stream()?
        };
        self.sink.take();
        self.outgoing.take();
        self.stream = Some(stream);
        if self.input.is_some() {
            self.start_at(pos, was_paused, false)?;
        }
        Ok(())
    }
    pub fn play(
        &mut self,
        id: String,
        input: String,
        headers: Vec<(String, String)>,
        duration: f64,
        effects: Effects,
    ) -> Result<()> {
        effects.validate()?;
        if self.stream.is_none() {
            self.select_device(None)?;
        }
        let previous = (
            self.input.clone(),
            self.headers.clone(),
            self.track_id.clone(),
            self.duration,
            self.effects.clone(),
            self.playback_source.clone(),
        );
        self.playback_source = Value::Null;
        self.input = Some(input);
        self.headers = headers;
        self.track_id = Some(id);
        self.duration = duration;
        self.effects = effects;
        if let Err(error) = self.start_at(0., false, true) {
            (
                self.input,
                self.headers,
                self.track_id,
                self.duration,
                self.effects,
                self.playback_source,
            ) = previous;
            return Err(error);
        }
        Ok(())
    }
    fn start_at(&mut self, seconds: f64, paused: bool, crossfade: bool) -> Result<()> {
        if !crossfade {
            self.outgoing.take();
        }
        let input = self.input.as_ref().context("Nothing loaded")?;
        let filter = media::filter(&self.effects)?;
        let ready = if seconds == 0. {
            self.prepared
                .iter_mut()
                .find(|slot| {
                    slot.as_ref().is_some_and(|audio| {
                        audio.expires > Instant::now()
                            && audio.input == *input
                            && audio.headers == self.headers
                            && audio.proxy == self.proxy
                            && audio.filter == filter
                            && audio.error.lock().is_ok_and(|error| error.is_empty())
                    })
                })
                .and_then(Option::take)
        } else {
            None
        };
        let (source, error) = match ready {
            Some(audio) => (audio.source, audio.error),
            _ => open_decoder(
                input,
                &self.headers,
                self.proxy.as_deref(),
                &filter,
                seconds,
            )?,
        };
        self.meter = source.meter.clone();
        self.clock = source.clock.clone();
        self.error = error;
        let sink = Sink::connect_new(self.stream.as_ref().context("No audio output")?.mixer());
        sink.pause();
        sink.set_volume(if self.effects.muted {
            0.
        } else {
            self.effects.volume
        });
        sink.append(source);
        if let Some(old) = self.sink.take()
            && crossfade
            && self.effects.crossfade_seconds > 0.
            && !old.is_paused()
            && !old.empty()
        {
            self.outgoing = Some((old, Instant::now(), self.effects.crossfade_seconds));
            sink.set_volume(0.);
        }
        if !paused {
            sink.play();
        }
        self.sink = Some(sink);
        self.offset = seconds;
        Ok(())
    }
    pub fn pause(&mut self) {
        if let Some(s) = &self.sink {
            s.pause();
        }
        self.outgoing.take();
        if let Some(s) = &self.sink {
            s.set_volume(if self.effects.muted {
                0.
            } else {
                self.effects.volume
            });
        }
    }
    pub fn resume(&self) -> Result<()> {
        self.sink.as_ref().context("Nothing loaded")?.play();
        Ok(())
    }
    pub(crate) fn has_loaded_audio(&self) -> bool {
        self.sink.is_some()
    }
    pub fn stop(&mut self) {
        self.prepared = [None, None];
        self.sink.take();
        self.outgoing.take();
        self.input = None;
        self.track_id = None;
        self.offset = 0.;
        self.duration = 0.;
        self.playback_source = Value::Null;
        self.meter = Arc::new(Mutex::new(Meter::default()));
        self.error = Arc::new(Mutex::new(String::new()));
    }
    pub fn seek(&mut self, seconds: f64) -> Result<()> {
        ensure!(
            seconds.is_finite()
                && seconds >= 0.
                && (self.duration <= 0. || seconds <= self.duration),
            "Seek outside track"
        );
        self.start_at(seconds, self.paused(), false)
    }
    pub fn effects(&mut self, effects: Effects) -> Result<()> {
        effects.validate()?;
        let position = self.position();
        let mut old = self.effects.clone();
        old.volume = effects.volume;
        old.muted = effects.muted;
        old.crossfade_seconds = effects.crossfade_seconds;
        let restart = serde_json::to_value(old)? != serde_json::to_value(&effects)?;
        self.effects = effects;
        if let Some(s) = &self.sink {
            s.set_volume(if self.effects.muted {
                0.
            } else {
                self.effects.volume
            });
        }
        if restart && self.input.is_some() {
            self.start_at(position, self.paused(), false)?;
        }
        Ok(())
    }
    pub fn is_remote(&self) -> bool {
        self.input
            .as_deref()
            .is_some_and(|input| input.starts_with("http://") || input.starts_with("https://"))
    }
    pub fn wait_for_stream(&mut self, id: String, duration: f64, position: f64) {
        self.stop();
        self.track_id = Some(id);
        self.duration = duration;
        self.offset = position;
    }
    pub fn position(&self) -> f64 {
        self.offset
            + if self.sink.is_some() {
                self.clock.load(Ordering::Relaxed) as f64 / 96000.
                    * self.effects.speed
                    * self.effects.speed_pitch
            } else {
                0.
            }
    }
    pub fn paused(&self) -> bool {
        self.sink.as_ref().is_none_or(Sink::is_paused)
    }
    pub fn finished(&self) -> bool {
        self.sink.as_ref().is_some_and(Sink::empty)
    }
    pub fn should_crossfade(&self) -> bool {
        self.effects.crossfade_seconds > 0.
            && !self.paused()
            && self.duration > 0.
            && self.duration - self.position()
                <= self.effects.crossfade_seconds * self.effects.speed * self.effects.speed_pitch
            && self.outgoing.is_none()
    }
    pub fn tick(&mut self) {
        if let Some((old, start, duration)) = &self.outgoing {
            let fraction = (start.elapsed().as_secs_f64() / duration).clamp(0., 1.) as f32;
            let volume = if self.effects.muted {
                0.
            } else {
                self.effects.volume
            };
            old.set_volume(volume * (1. - fraction));
            if let Some(s) = &self.sink {
                s.set_volume(volume * fraction);
            }
            if fraction >= 1. {
                self.outgoing.take();
            }
        }
    }
    pub fn failure(&self) -> String {
        let error = self.error.lock().map(|e| e.clone()).unwrap_or_default();
        if !error.is_empty() {
            return error;
        }
        if self.finished()
            && self.is_remote()
            && (self.position() < 0.05
                || (self.duration > 0. && self.position() + 3. < self.duration))
        {
            return "The audio connection ended before the track finished. Please try playing this track again.".into();
        }
        String::new()
    }
    pub fn snapshot(&self) -> Value {
        json!({"track_id":self.track_id,"playback_source":self.playback_source,"position":self.position(),"duration":self.duration,"paused":self.paused(),"finished":self.finished(),"buffering":self.meter.lock().map(|m|m.buffering).unwrap_or(false),"playing":!self.paused() && !self.finished() && self.clock.load(Ordering::Relaxed)>0,"effects":self.effects,"decoder_error":self.failure()})
    }
    pub fn analysis(&self) -> Value {
        let Ok(mut meter) = self.meter.lock() else {
            return json!({"samples":[],"spectrum":[]});
        };
        let mono: Vec<f32> = meter
            .samples
            .iter()
            .copied()
            .collect::<Vec<_>>()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|s| (s[0] + s[1]) * 0.5)
            .collect();
        let n = mono.len();
        let rms = if n > 0 {
            (mono.iter().map(|x| x * x).sum::<f32>() / n as f32).sqrt()
        } else {
            0.
        };
        let spectrum: Vec<f32> = (0..64)
            .map(|k| {
                if n == 0 {
                    return 0.;
                }
                let (mut real, mut imag) = (0_f32, 0_f32);
                for (i, sample) in mono.iter().enumerate() {
                    let window = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos();
                    let a = std::f32::consts::TAU * (k * 4) as f32 * i as f32 / n as f32;
                    real += sample * window * a.cos();
                    imag -= sample * window * a.sin();
                }
                (real * real + imag * imag).sqrt() / n as f32
            })
            .collect();
        let peak = meter.peak;
        meter.peak = 0.;
        json!({"samples":mono,"spectrum":spectrum,"rms":rms,"peak":peak,"sample_rate":48000,"fft_size":n,"bin_stride":4})
    }
}

fn terminal_error(success: bool, diagnostic: &str) -> String {
    if success {
        return String::new();
    }
    let text = diagnostic.to_ascii_lowercase();
    if [
        "[tls @",
        "[http @",
        "[https @",
        "connection reset",
        "connection refused",
        "connection timed out",
        "network is unreachable",
        "server returned",
        "session has been invalidated",
    ]
    .iter()
    .any(|s| text.contains(s))
    {
        "The audio connection was interrupted. Please try playing this track again.".into()
    } else {
        "The audio decoder could not read this track. Please retry or choose another source.".into()
    }
}
#[cfg(test)]
mod failure_tests {
    use super::*;
    #[test]
    fn recovered_tls_diagnostics_are_not_a_playback_failure() {
        let log = "[tls @ 000123] IO error -10054 The specified session has been invalidated https://private.test/?token=secret";
        assert_eq!(terminal_error(true, log), "");
        let error = terminal_error(false, log);
        assert!(error.contains("connection was interrupted"));
        assert!(!error.contains("secret"));
        assert!(!error.contains("000123"));
        assert!(
            terminal_error(
                false,
                "Error opening input file C:/qa-network-failure/invalid.wav: Invalid data found"
            )
            .contains("decoder could not read")
        );
    }
}
