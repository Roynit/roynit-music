use crate::{
    model::{Effects, Track},
    store, tools,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub fn inspect(path: &Path) -> Result<Track> {
    let path = path.canonicalize().context("Audio file not found")?;
    ensure!(path.is_file(), "Not a regular file");
    let p = path.to_str().context("Path is not UTF-8")?;
    let value = tools::json(
        "ffprobe",
        [
            "-v",
            "error",
            "-show_format",
            "-show_streams",
            "-of",
            "json",
            p,
        ],
    )?;
    ensure!(
        value["streams"]
            .as_array()
            .is_some_and(|s| s.iter().any(|v| v["codec_type"] == "audio")),
        "File has no audio stream"
    );
    let tags = &value["format"]["tags"];
    let tag = |name: &str| {
        tags.as_object()
            .and_then(|m| m.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)))
            .and_then(|(_, v)| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    let title = tag("title");
    Ok(Track {
        id: format!("local:{}", store::file_hash(&path)?),
        title: if title.is_empty() {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        } else {
            title
        },
        artist: tag("artist"),
        album: tag("album"),
        duration: value["format"]["duration"]
            .as_str()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.),
        source: p.into(),
        platform: "local".into(),
        cover: None,
        local_path: Some(path),
        cache_path: None,
    })
}

pub fn waveform(path: &Path, points: usize) -> Result<Value> {
    ensure!(
        (32..=8192).contains(&points),
        "Waveform points must be 32..8192"
    );
    let track = inspect(path)?;
    ensure!(
        track.duration <= 7200.,
        "Waveform analysis currently supports files up to two hours"
    );
    let bytes = tools::run(
        "ffmpeg",
        [
            "-v",
            "error",
            "-nostdin",
            "-i",
            path.to_str().context("Path is not UTF-8")?,
            "-vn",
            "-f",
            "f32le",
            "-acodec",
            "pcm_f32le",
            "-ac",
            "1",
            "-ar",
            "8000",
            "pipe:1",
        ],
        Duration::from_secs(180),
    )?;
    let samples: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|x| f32::from_le_bytes(*x))
        .collect();
    ensure!(!samples.is_empty(), "Decoder returned no samples");
    let size = samples.len().div_ceil(points);
    let mut peak = 0_f32;
    let mut squares = 0_f64;
    let peaks: Vec<_> = samples
        .chunks(size)
        .map(|block| {
            crate::jobs::check_cancelled()?;
            let (mut lo, mut hi) = (0_f32, 0_f32);
            for s in block {
                lo = lo.min(*s);
                hi = hi.max(*s);
                peak = peak.max(s.abs());
                squares += (*s as f64).powi(2);
            }
            Ok(json!([lo, hi]))
        })
        .collect::<Result<_>>()?;
    Ok(
        json!({"track":track,"peaks":peaks,"sample_rate":8000,"peak":peak,"rms":(squares/samples.len() as f64).sqrt(),"frames":samples.len()}),
    )
}

pub fn extract_cover(input: &Path, output: &Path) -> Result<Value> {
    ensure!(input.is_file(), "Input file missing");
    ensure!(!output.exists(), "Cover output already exists");
    let extension = output.extension().and_then(|s| s.to_str()).unwrap_or("png");
    ensure!(
        ["png", "jpg", "jpeg", "webp"].contains(&extension),
        "Cover must be PNG, JPEG or WebP"
    );
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let temp = tempfile::Builder::new()
        .suffix(&format!(".{extension}"))
        .tempfile_in(parent)?;
    tools::run(
        "ffmpeg",
        [
            "-v",
            "error",
            "-y",
            "-i",
            input.to_str().context("Invalid input path")?,
            "-an",
            "-frames:v",
            "1",
            temp.path().to_str().context("Invalid output path")?,
        ],
        Duration::from_secs(30),
    )?;
    temp.persist_noclobber(output).map_err(|e| e.error)?;
    Ok(json!({"path":output}))
}

#[derive(Debug, Serialize)]
pub struct ScanResult {
    pub tracks: Vec<Track>,
    pub errors: Vec<Value>,
}
pub fn scan(paths: &[PathBuf]) -> ScanResult {
    let mut files = Vec::new();
    let mut errors = Vec::new();
    for path in paths {
        if path.is_dir() {
            for entry in walkdir::WalkDir::new(path).follow_links(false) {
                match entry {
                    Ok(e) if e.file_type().is_file() => files.push(e.path().to_path_buf()),
                    Err(e) => errors.push(json!({"path":e.path(),"error":e.to_string()})),
                    _ => {}
                }
            }
        } else {
            files.push(path.clone());
        }
    }
    files.sort();
    files.dedup();
    let mut tracks = Vec::new();
    for path in files {
        let ext = path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        if ![
            "mp3", "flac", "wav", "ogg", "opus", "m4a", "aac", "aiff", "wma", "mp4", "webm",
        ]
        .contains(&ext.as_str())
        {
            continue;
        }
        match inspect(&path) {
            Ok(t) => tracks.push(t),
            Err(e) => errors.push(json!({"path":path,"error":e.to_string()})),
        }
    }
    ScanResult { tracks, errors }
}

pub fn filter(e: &Effects) -> Result<String> {
    e.validate()?;
    let mut f = vec!["aresample=48000".to_string()];
    let pitch = 2_f64.powf(e.pitch / 12.) * e.speed_pitch;
    if (pitch - 1.).abs() > 0.00001 {
        f.push(format!("asetrate={}", 48000. * pitch));
        f.push("aresample=48000".into());
    }
    let mut tempo = e.speed * e.speed_pitch / pitch;
    while tempo < 0.5 {
        f.push("atempo=0.5".into());
        tempo /= 0.5;
    }
    while tempo > 2. {
        f.push("atempo=2".into());
        tempo /= 2.;
    }
    if (tempo - 1.).abs() > 0.00001 {
        f.push(format!("atempo={tempo}"));
    }
    for (frequency, gain) in [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000]
        .into_iter()
        .zip(e.eq_bands)
    {
        if gain != 0. {
            f.push(format!("equalizer=f={frequency}:t=o:w=1:g={gain}"));
        }
    }
    if e.bass != 0. {
        f.push(format!("bass=g={}", e.bass));
    }
    if e.mid != 0. {
        f.push(format!("equalizer=f=1000:t=o:w=2:g={}", e.mid));
    }
    if e.treble != 0. {
        f.push(format!("treble=g={}", e.treble));
    }
    if e.echo > 0. {
        f.push(format!("aecho=0.8:0.9:250:{}", e.echo / 100. * 0.6));
    }
    if e.reverb > 0. {
        let d = e.reverb / 100. * 0.5;
        f.push(format!(
            "aecho=0.8:0.9:43|79|131:{d}|{}|{}",
            d * 0.7,
            d * 0.4
        ));
    }
    if e.distortion > 0. {
        f.push(format!(
            "asoftclip=type=tanh:threshold=0.8:output=0.8:param={}",
            1. + e.distortion / 10.
        ));
    }
    if e.normalize {
        f.push("dynaudnorm=f=150:g=15".into());
    }
    if e.hq.enabled {
        f.push(format!("bass=g={}", e.hq.bass));
        f.push(format!("equalizer=f=2500:t=o:w=1.5:g={}", e.hq.vocal));
        f.push(format!("treble=f=10000:g={}", e.hq.air));
        f.push("acompressor=threshold=0.125:ratio=2:attack=10:release=150:makeup=1".into());
    }
    f.push("alimiter=limit=0.95:level=false".into());
    Ok(f.join(","))
}

#[derive(Debug, Deserialize)]
pub struct EncodeRequest {
    pub input: PathBuf,
    pub output: PathBuf,
    #[serde(default)]
    pub start: f64,
    pub end: Option<f64>,
    #[serde(default)]
    pub effects: Effects,
    #[serde(default)]
    pub lossless_copy: bool,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
}
pub fn encode(r: &EncodeRequest) -> Result<Value> {
    ensure!(r.input.is_file(), "Input file missing");
    ensure!(
        !r.output.exists(),
        "Output already exists; choose a new file"
    );
    ensure!(r.start.is_finite() && r.start >= 0., "Invalid start time");
    if let Some(end) = r.end {
        ensure!(end.is_finite() && end > r.start, "End must be after start");
    }
    let extension = r
        .output
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("wav");
    ensure!(
        ["mp3", "flac", "opus", "ogg", "wav", "m4a"].contains(&extension),
        "Unsupported output format"
    );
    let parent = r
        .output
        .parent()
        .filter(|x| !x.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let temp = tempfile::Builder::new()
        .suffix(&format!(".{extension}"))
        .tempfile_in(parent)?;
    let mut a = vec![
        "-v".into(),
        "error".into(),
        "-y".into(),
        "-ss".into(),
        r.start.to_string(),
        "-i".into(),
        r.input.to_string_lossy().into_owned(),
    ];
    if let Some(end) = r.end {
        a.extend(["-t".into(), (end - r.start).to_string()]);
    }
    a.extend(["-map".into(), "0:a:0".into(), "-vn".into()]);
    if r.lossless_copy {
        a.extend(["-c:a".into(), "copy".into()]);
    } else {
        a.extend([
            "-af".into(),
            format!(
                "{},volume={}",
                filter(&r.effects)?,
                if r.effects.muted {
                    0.
                } else {
                    r.effects.volume
                }
            ),
        ]);
        let codec = match extension {
            "mp3" => "libmp3lame",
            "flac" => "flac",
            "opus" | "ogg" => "libopus",
            "m4a" => "aac",
            _ => "pcm_s16le",
        };
        a.extend(["-c:a".into(), codec.into()]);
    }
    for (key, value) in [
        ("title", &r.title),
        ("artist", &r.artist),
        ("album", &r.album),
    ] {
        if let Some(value) = value {
            a.extend(["-metadata".into(), format!("{key}={value}")]);
        }
    }
    a.push(temp.path().to_string_lossy().into_owned());
    tools::run("ffmpeg", &a, Duration::from_secs(1800))?;
    temp.persist_noclobber(&r.output).map_err(|e| e.error)?;
    Ok(json!({"path":r.output,"bytes":std::fs::metadata(&r.output)?.len()}))
}
