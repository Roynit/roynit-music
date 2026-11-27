//! Voice messages on the PC: record from the microphone and shrink it to small AAC sound with FFmpeg, and play a
//! voice message through the sound card with a progress the chat can draw.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

pub const MAX_SECONDS: i64 = 180;
const BARS: usize = 48;

#[derive(Default)]
struct Shared {
    samples: Mutex<Vec<f32>>,
    /// The loudness of the latest stretches of sound, newest last, to draw as a waveform while recording.
    levels: Mutex<Vec<f32>>,
    paused: AtomicBool,
    stop: AtomicBool,
    rate: AtomicU32,
}

/// A microphone recording in progress. The microphone is read on its own thread.
pub struct Recorder {
    shared: Arc<Shared>,
    worker: Option<std::thread::JoinHandle<()>>,
}

/// What was recorded: mono sound and its sample rate.
pub struct Clip {
    pub samples: Vec<f32>,
    pub rate: u32,
}

impl Recorder {
    pub fn start() -> Result<Self, String> {
        let shared = Arc::new(Shared::default());
        let (tx, rx) = mpsc::channel();
        let inner = shared.clone();
        let worker = std::thread::spawn(move || run(inner, tx));
        // The thread answers as soon as the microphone is open (or could not be).
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self { shared, worker: Some(worker) }),
            Ok(Err(error)) => Err(error),
            Err(_) => Err("The microphone did not answer.".into()),
        }
    }

    pub fn seconds(&self) -> f32 {
        let rate = self.shared.rate.load(Ordering::Relaxed).max(1) as f32;
        self.shared.samples.lock().map_or(0., |s| s.len() as f32 / rate)
    }

    pub fn paused(&self) -> bool {
        self.shared.paused.load(Ordering::Relaxed)
    }

    pub fn toggle_pause(&self) {
        self.shared.paused.fetch_xor(true, Ordering::Relaxed);
    }

    /// The last [`BARS`] loudness values (0 to 1), oldest first.
    pub fn levels(&self) -> Vec<f32> {
        let levels = self.shared.levels.lock().map(|l| l.clone()).unwrap_or_default();
        let mut out = vec![0.; BARS.saturating_sub(levels.len())];
        out.extend(levels.iter().rev().take(BARS).rev());
        out
    }

    /// Stops the microphone and hands over what was recorded.
    pub fn finish(mut self) -> Clip {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        Clip {
            samples: self.shared.samples.lock().map(|mut s| std::mem::take(&mut *s)).unwrap_or_default(),
            rate: self.shared.rate.load(Ordering::Relaxed).max(8000),
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

fn run(shared: Arc<Shared>, ready: mpsc::Sender<Result<(), String>>) {
    if let Err(error) = record(shared, &ready) {
        let _ = ready.send(Err(error));
    }
}

fn record(shared: Arc<Shared>, ready: &mpsc::Sender<Result<(), String>>) -> Result<(), String> {
    let device = cpal::default_host().default_input_device().ok_or("No microphone was found.")?;
    let config = device.default_input_config().map_err(|_| "The microphone cannot be used.".to_owned())?;
    let channels = config.channels().max(1) as usize;
    shared.rate.store(config.sample_rate().0, Ordering::Relaxed);
    let block = (config.sample_rate().0 / 16) as usize;
    let mut sum = 0f32;
    let mut count = 0usize;
    let state = shared.clone();
    let mut take = move |mono: f32| {
        if state.paused.load(Ordering::Relaxed) {
            return;
        }
        if let Ok(mut s) = state.samples.lock() {
            s.push(mono);
        }
        sum += mono * mono;
        count += 1;
        if count >= block {
            let level = (sum / count as f32).sqrt();
            if let Ok(mut l) = state.levels.lock() {
                l.push((level * 6.).clamp(0.05, 1.));
                if l.len() > 256 {
                    l.drain(..128);
                }
            }
            sum = 0.;
            count = 0;
        }
    };
    let error = |_| {};
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(&config.clone().into(), move |data: &[f32], _| data.chunks(channels).for_each(|f| take(f.iter().sum::<f32>() / channels as f32)), error, None),
        cpal::SampleFormat::I16 => device.build_input_stream(&config.clone().into(), move |data: &[i16], _| data.chunks(channels).for_each(|f| take(f.iter().map(|v| *v as f32 / 32768.).sum::<f32>() / channels as f32)), error, None),
        cpal::SampleFormat::U16 => device.build_input_stream(&config.clone().into(), move |data: &[u16], _| data.chunks(channels).for_each(|f| take(f.iter().map(|v| (*v as f32 - 32768.) / 32768.).sum::<f32>() / channels as f32)), error, None),
        _ => return Err("This microphone format is not supported.".into()),
    }
    .map_err(|_| "The microphone is busy or blocked. Check Windows privacy settings for microphones.".to_owned())?;
    stream.play().map_err(|_| "The microphone could not start.".to_owned())?;
    // Open: the caller stops waiting; this thread keeps the stream alive until told to stop.
    let _ = ready.send(Ok(()));
    while !shared.stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(40));
    }
    Ok(())
}

/// Shrinks a recording to AAC in an MP4 file (about 3 KB a second), and says how long it is.
pub fn encode(clip: Clip) -> Result<(Vec<u8>, i64), String> {
    let seconds = (clip.samples.len() as f32 / clip.rate as f32).round() as i64;
    if clip.samples.len() < clip.rate as usize / 2 {
        return Err("That recording is too short.".into());
    }
    let dir = std::env::temp_dir();
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let stamp = format!("{nanos:x}-{}", std::process::id());
    let (raw, out) = (dir.join(format!("roy-rec-{stamp}.raw")), dir.join(format!("roy-rec-{stamp}.m4a")));
    let bytes: Vec<u8> = clip.samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    std::fs::write(&raw, bytes).map_err(|_| "Could not save the recording.".to_owned())?;
    let args: Vec<String> = ["-v", "error", "-y", "-f", "f32le", "-ar", &clip.rate.to_string(), "-ac", "1", "-i", &raw.to_string_lossy(), "-af", "highpass=f=80", "-c:a", "aac", "-b:a", "24k", "-ar", "24000", "-movflags", "+faststart", &out.to_string_lossy()]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let result = riff_core::tools::run("ffmpeg", args, Duration::from_secs(60));
    let data = std::fs::read(&out);
    let _ = std::fs::remove_file(&raw);
    let _ = std::fs::remove_file(&out);
    result.map_err(|_| "FFmpeg could not prepare the recording.".to_owned())?;
    let data = data.map_err(|_| "FFmpeg could not prepare the recording.".to_owned())?;
    if data.len() > 690_000 {
        return Err("That recording is too long.".into());
    }
    Ok((data, seconds.max(1)))
}

/// A stable, natural-looking waveform for a voice message (the server keeps only its length), different for each message.
pub fn shape(seed: i64, bars: usize) -> Vec<f32> {
    let mut x = (seed as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..bars)
        .map(|i| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let wave = ((i as f32 / bars as f32) * std::f32::consts::PI).sin();
            (0.25 + 0.75 * ((x % 1000) as f32 / 1000.) * (0.45 + 0.55 * wave)).clamp(0.15, 1.)
        })
        .collect()
}

struct Now {
    url: String,
    started: Option<Instant>,
    seconds: f32,
    stop: Arc<AtomicBool>,
}

static NOW: Mutex<Option<Now>> = Mutex::new(None);

/// Whether this voice message is playing (or being fetched), and how far along it is (0 to 1).
pub fn state(url: &str) -> Option<f32> {
    let now = NOW.lock().ok()?;
    let now = now.as_ref().filter(|n| n.url == url)?;
    Some(now.started.map_or(0., |s| (s.elapsed().as_secs_f32() / now.seconds.max(0.1)).min(1.)))
}

/// Plays a voice message, or stops it if it is the one playing. Only one plays at a time.
pub fn toggle(client: reqwest::blocking::Client, url: String) {
    let stop = Arc::new(AtomicBool::new(false));
    {
        let Ok(mut now) = NOW.lock() else { return };
        let same = now.as_ref().is_some_and(|n| n.url == url);
        if let Some(old) = now.take() {
            old.stop.store(true, Ordering::Relaxed);
        }
        if same {
            return;
        }
        *now = Some(Now { url: url.clone(), started: None, seconds: 1., stop: stop.clone() });
    }
    std::thread::spawn(move || {
        let finish = |stop: &Arc<AtomicBool>| {
            if let Ok(mut now) = NOW.lock()
                && now.as_ref().is_some_and(|n| Arc::ptr_eq(&n.stop, stop))
            {
                *now = None;
            }
        };
        let samples = (|| {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            url.hash(&mut hasher);
            let path = std::env::temp_dir().join(format!("roy-voice-{:x}.m4a", hasher.finish()));
            if !path.is_file() {
                let bytes = client.get(&url).send().and_then(|r| r.error_for_status()).and_then(|r| r.bytes()).ok()?;
                std::fs::write(&path, &bytes).ok()?;
            }
            let args: Vec<String> = ["-v", "error", "-nostdin", "-i", &path.to_string_lossy(), "-f", "f32le", "-ac", "1", "-ar", "44100", "-"].iter().map(|s| s.to_string()).collect();
            let raw = riff_core::tools::run("ffmpeg", args, Duration::from_secs(60)).ok()?;
            Some(raw.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect::<Vec<f32>>())
        })();
        let Some(samples) = samples.filter(|s| !s.is_empty()) else {
            finish(&stop);
            return;
        };
        let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else {
            finish(&stop);
            return;
        };
        let sink = rodio::Sink::connect_new(stream.mixer());
        if stop.load(Ordering::Relaxed) {
            return;
        }
        if let Ok(mut now) = NOW.lock()
            && let Some(n) = now.as_mut().filter(|n| Arc::ptr_eq(&n.stop, &stop))
        {
            n.seconds = samples.len() as f32 / 44_100.;
            n.started = Some(Instant::now());
        }
        sink.append(rodio::buffer::SamplesBuffer::new(1, 44_100, samples));
        while !sink.empty() && !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(50));
        }
        sink.stop();
        finish(&stop);
    });
}
