//! What Roy Music needs on a machine, how to tell what is missing, and how to put it there.
use crate::tools::{self, OnProgress, Phase};
use anyhow::Result;
use std::{path::Path, time::Duration};

/// Free space needed to unpack the media engine and keep a download cache.
pub const MIN_FREE_BYTES: u64 = 400 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Id {
    /// FFmpeg and FFprobe: decode and measure the audio.
    MediaEngine,
    /// yt-dlp: finds playable streams when a direct link is unavailable.
    OnlinePlayback,
    AudioOutput,
    Network,
    DiskSpace,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Present, with a short version or description.
    Ready(String),
    /// Missing, and the installer can fetch it (approximate download size in MB).
    Missing { download_mb: u32 },
    /// Cannot be fixed here; playback or installation may still work.
    Warning(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    pub id: Id,
    pub state: State,
}

impl Id {
    /// Items the installer can install itself.
    pub fn installable(self) -> bool {
        matches!(self, Id::MediaEngine | Id::OnlinePlayback)
    }
}

fn first_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_owned()
}

/// "ffmpeg version n9.0-4-g…" → "9.0"; unknown shapes keep the third word.
fn short_version(line: &str) -> String {
    trim_version(line.split_whitespace().nth(2).unwrap_or(line))
}
fn trim_version(word: &str) -> String {
    let word = word.trim_start_matches('n').trim_start_matches('v');
    word.split(['-', '+']).next().unwrap_or(word).to_owned()
}

pub fn media_engine() -> Check {
    let ffmpeg = tools::run("ffmpeg", ["-version"], Duration::from_secs(10));
    let ffprobe = tools::run("ffprobe", ["-version"], Duration::from_secs(10));
    let state = match (ffmpeg, ffprobe) {
        (Ok(v), Ok(_)) => State::Ready(short_version(&first_line(&v))),
        _ => State::Missing { download_mb: 85 },
    };
    Check {
        id: Id::MediaEngine,
        state,
    }
}

pub fn online_playback() -> Check {
    let state = match tools::run("yt-dlp", ["--version"], Duration::from_secs(10)) {
        Ok(v) => State::Ready(first_line(&v)),
        Err(_) => State::Missing { download_mb: 18 },
    };
    Check {
        id: Id::OnlinePlayback,
        state,
    }
}

pub fn audio_output() -> Check {
    use cpal::traits::{DeviceTrait, HostTrait};
    #[allow(deprecated)]
    let name = cpal::default_host()
        .default_output_device()
        .map(|d| d.name().unwrap_or_else(|_| "Default output".into()));
    let state = match name {
        Some(n) => State::Ready(n),
        None => State::Warning("No speakers or headphones found".into()),
    };
    Check {
        id: Id::AudioOutput,
        state,
    }
}

pub fn network() -> Check {
    use std::net::{TcpStream, ToSocketAddrs};
    let online = "api.github.com:443"
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
        .is_some_and(|addr| TcpStream::connect_timeout(&addr, Duration::from_secs(4)).is_ok());
    let state = if online {
        State::Ready("Connected".into())
    } else {
        State::Warning("Offline: downloads will be skipped".into())
    };
    Check {
        id: Id::Network,
        state,
    }
}

/// Free space where `dir` lives, even when `dir` does not exist yet.
pub fn disk_space(dir: &Path) -> Check {
    let probe = dir.ancestors().find(|p| p.exists()).unwrap_or(dir);
    let state = match fs2::available_space(probe) {
        Ok(free) if free >= MIN_FREE_BYTES => State::Ready(format!("{} free", human_bytes(free))),
        Ok(free) => State::Warning(format!("Only {} free", human_bytes(free))),
        Err(_) => State::Ready("Unknown".into()),
    };
    Check {
        id: Id::DiskSpace,
        state,
    }
}

pub fn human_bytes(bytes: u64) -> String {
    const GB: f64 = 1024. * 1024. * 1024.;
    const MB: f64 = 1024. * 1024.;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else {
        format!("{:.0} MB", b / MB)
    }
}

/// Run every check. Cheap ones first; the network probe is the slowest.
pub fn check_all(install_dir: &Path) -> Vec<Check> {
    vec![
        disk_space(install_dir),
        audio_output(),
        media_engine(),
        online_playback(),
        network(),
    ]
}

/// Fetch and verify one installable component; returns its version.
pub fn install(id: Id, on: OnProgress) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("Roy-Music/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()?;
    let info = match id {
        Id::MediaEngine => tools::install_ffmpeg_with(&client, on)?,
        Id::OnlinePlayback => tools::install_ytdlp_with(&client, on)?,
        _ => anyhow::bail!("This item cannot be installed automatically"),
    };
    on(Phase::Verifying, 0, None);
    // "n9.0.2-61-gd590…" is for logs; people see "9.0.2".
    Ok(trim_version(info["version"].as_str().unwrap_or("")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions_are_shortened_for_display() {
        assert_eq!(
            short_version("ffmpeg version n9.0-12-gabcdef Copyright"),
            "9.0"
        );
        assert_eq!(
            short_version("ffmpeg version 7.1.1-essentials_build"),
            "7.1.1"
        );
        assert_eq!(short_version("2025.01.15"), "2025.01.15");
    }
    #[test]
    fn disk_check_works_for_a_folder_that_does_not_exist_yet() {
        let dir = std::env::temp_dir()
            .join("roy-music-missing")
            .join("deeper");
        assert!(matches!(
            disk_space(&dir).state,
            State::Ready(_) | State::Warning(_)
        ));
    }
    #[test]
    fn only_media_helpers_are_installable() {
        assert!(Id::MediaEngine.installable() && Id::OnlinePlayback.installable());
        assert!(!Id::Network.installable() && !Id::AudioOutput.installable());
    }
}
