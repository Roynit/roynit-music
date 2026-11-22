use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{
    ffi::OsStr,
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// What an installer step is doing, so a UI can word it without parsing text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Resolving,
    Downloading,
    Unpacking,
    Verifying,
}
/// Called with the phase, bytes so far and the total when the server reports one.
pub type OnProgress<'a> = &'a mut dyn FnMut(Phase, u64, Option<u64>);

pub fn command(name: &str) -> Command {
    let env_key = format!("RIFF_{}", name.replace('-', "_").to_uppercase());
    let managed = managed_dir().ok().map(|p| {
        p.join(if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.into()
        })
    });
    let executable = std::env::var_os(env_key)
        .or_else(|| managed.filter(|p| p.is_file()).map(|p| p.into_os_string()))
        .unwrap_or_else(|| name.into());
    let mut c = Command::new(executable);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    c
}

pub fn managed_dir() -> Result<std::path::PathBuf> {
    if let Some(dir) = std::env::var_os("RIFF_TOOLS_DIR") {
        return Ok(dir.into());
    }
    Ok(crate::Engine::default_root()?.join("bin"))
}

/// Prepare the managed helper on fresh installations. Keep working copies in place on updates.
pub fn ensure_ytdlp() -> Result<()> {
    if run("yt-dlp", ["--version"], Duration::from_secs(8)).is_ok() {
        return Ok(());
    }
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("Roy-Music/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .build()?;
    install_ytdlp(&client)?;
    run("yt-dlp", ["--version"], Duration::from_secs(8))?;
    Ok(())
}

/// Download an upstream release and verify its SHA-256 manifest before publishing it.
pub fn install_ytdlp(client: &reqwest::blocking::Client) -> Result<Value> {
    install_ytdlp_with(client, &mut |_, _, _| {})
}

/// Stream `url` into a temp file inside `dir`, hashing as it goes, and refuse a wrong digest.
fn download_verified(
    client: &reqwest::blocking::Client,
    url: &str,
    expected: &str,
    dir: &std::path::Path,
    limit: u64,
    label: &str,
    on: OnProgress,
) -> Result<tempfile::NamedTempFile> {
    use sha2::{Digest, Sha256};
    use std::io::Write;
    let mut response = client
        .get(url)
        .timeout(Duration::from_secs(1800))
        .send()?
        .error_for_status()?;
    let total = response.content_length();
    anyhow::ensure!(
        total.is_none_or(|t| t <= limit),
        "{label} download exceeds its size limit"
    );
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    let mut hash = Sha256::new();
    let mut block = [0u8; 65536];
    let mut bytes = 0u64;
    loop {
        let n = response.read(&mut block)?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        anyhow::ensure!(bytes <= limit, "{label} download exceeds its size limit");
        hash.update(&block[..n]);
        file.write_all(&block[..n])?;
        on(Phase::Downloading, bytes, total);
    }
    anyhow::ensure!(
        format!("{:x}", hash.finalize()) == expected,
        "{label} SHA-256 mismatch"
    );
    file.as_file().sync_all()?;
    Ok(file)
}

/// The digest for `name` in a `sha256sum`-style manifest (`<hash>  <name>` per line).
pub fn checksum_for(manifest: &str, name: &str) -> Option<String> {
    manifest.lines().find_map(|row| {
        let mut p = row.split_whitespace();
        let hash = p.next()?;
        let file = p.next()?.trim_start_matches('*');
        (file == name).then_some(hash.to_ascii_lowercase())
    })
}

pub fn install_ytdlp_with(client: &reqwest::blocking::Client, on: OnProgress) -> Result<Value> {
    let asset = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "yt-dlp.exe",
        ("windows", "aarch64") => "yt-dlp_arm64.exe",
        ("linux", "x86_64") => "yt-dlp_linux",
        ("linux", "aarch64") => "yt-dlp_linux_aarch64",
        ("macos", _) => "yt-dlp_macos",
        _ => bail!("No managed yt-dlp build for this platform"),
    };
    let release: Value = client
        .get("https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest")
        .send()?
        .error_for_status()?
        .json()?;
    let tag = release["tag_name"]
        .as_str()
        .context("Missing release tag")?;
    anyhow::ensure!(
        tag.chars()
            .all(|c| c.is_ascii_alphanumeric() || ['.', '-', '_'].contains(&c)),
        "Invalid release tag"
    );
    let base = format!("https://github.com/yt-dlp/yt-dlp/releases/download/{tag}");
    let manifest = client
        .get(format!("{base}/SHA2-256SUMS"))
        .send()?
        .error_for_status()?
        .text()?;
    let expected =
        checksum_for(&manifest, asset).context("No SHA-256 checksum for release asset")?;
    let dir = managed_dir()?;
    std::fs::create_dir_all(&dir)?;
    on(Phase::Downloading, 0, None);
    let file = download_verified(
        client,
        &format!("{base}/{asset}"),
        &expected,
        &dir,
        150 * 1024 * 1024,
        "yt-dlp",
        on,
    )?;
    let bytes = file.as_file().metadata()?.len();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o755))?;
    }
    let target = dir.join(if cfg!(windows) {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    });
    file.persist(&target).map_err(|e| e.error)?;
    Ok(serde_json::json!({"path":target,"version":tag,"sha256":expected,"bytes":bytes}))
}
const FFMPEG_BASE: &str = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest";
const FFMPEG_RELEASE: &str = "https://api.github.com/repos/BtbN/FFmpeg-Builds/releases/tags/latest";

/// Newest released FFmpeg line among the published LGPL "shared" archives, falling back to master.
pub fn pick_ffmpeg_asset<'a>(
    names: impl IntoIterator<Item = &'a str>,
    platform: &str,
) -> Option<String> {
    let line = regex::Regex::new(&format!(
        r"^ffmpeg-n(\d+)\.(\d+)-latest-{platform}-lgpl-shared-(\d+)\.(\d+)\.zip$"
    ))
    .ok()?;
    let master = format!("ffmpeg-master-latest-{platform}-lgpl-shared.zip");
    let mut best: Option<((u32, u32), &str)> = None;
    let mut has_master = false;
    for name in names {
        has_master |= name == master;
        if let Some(c) = line.captures(name) {
            let n = |i: usize| c[i].parse::<u32>().ok();
            if let (Some(a), Some(b), Some(x), Some(y)) = (n(1), n(2), n(3), n(4))
                && (a, b) == (x, y)
                && best.is_none_or(|(v, _)| (a, b) > v)
            {
                best = Some(((a, b), name));
            }
        }
    }
    best.map(|(_, n)| n.to_owned())
        .or(has_master.then_some(master))
}

fn quiet(mut c: Command) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    c.stdin(Stdio::null());
    c
}

/// Pull `bin/*` out of an FFmpeg archive with the bsdtar that ships in Windows 10+.
fn extract_ffmpeg_bin(archive: &std::path::Path, dest: &std::path::Path) -> Result<()> {
    let tar =
        std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap_or("C:\\Windows".into()))
            .join("System32")
            .join("tar.exe");
    anyhow::ensure!(
        tar.is_file(),
        "Windows tar.exe is missing, so the archive cannot be unpacked"
    );
    std::fs::create_dir_all(dest)?;
    let out = quiet(Command::new(tar))
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(dest)
        .args(["--strip-components=2", "*/bin/*"])
        .output()?;
    anyhow::ensure!(
        out.status.success(),
        "Could not unpack FFmpeg: {}",
        String::from_utf8_lossy(&out.stderr)
            .chars()
            .take(400)
            .collect::<String>()
    );
    Ok(())
}

/// Put verified FFmpeg and FFprobe (plus their DLLs) in the managed tools folder.
pub fn install_ffmpeg_with(client: &reqwest::blocking::Client, on: OnProgress) -> Result<Value> {
    anyhow::ensure!(cfg!(windows), "Install FFmpeg with your package manager");
    let platform = if std::env::consts::ARCH == "aarch64" {
        "winarm64"
    } else {
        "win64"
    };
    on(Phase::Resolving, 0, None);
    let release: Value = client
        .get(FFMPEG_RELEASE)
        .send()?
        .error_for_status()?
        .json()?;
    let names: Vec<&str> = release["assets"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect())
        .unwrap_or_default();
    let asset = pick_ffmpeg_asset(names, platform)
        .context("No FFmpeg build is published for this platform")?;
    let manifest = client
        .get(format!("{FFMPEG_BASE}/checksums.sha256"))
        .send()?
        .error_for_status()?
        .text()?;
    let expected =
        checksum_for(&manifest, &asset).context("No SHA-256 checksum for the FFmpeg archive")?;
    let dir = managed_dir()?;
    std::fs::create_dir_all(&dir)?;
    let archive = download_verified(
        client,
        &format!("{FFMPEG_BASE}/{asset}"),
        &expected,
        &dir,
        400 * 1024 * 1024,
        "FFmpeg",
        on,
    )?;
    on(Phase::Unpacking, 0, None);
    let staging = tempfile::tempdir_in(&dir)?;
    extract_ffmpeg_bin(archive.path(), staging.path())?;
    for name in ["ffmpeg.exe", "ffprobe.exe"] {
        anyhow::ensure!(
            staging.path().join(name).is_file(),
            "{name} is missing from the FFmpeg archive"
        );
    }
    for entry in std::fs::read_dir(staging.path())? {
        let entry = entry?;
        if entry.file_name().eq_ignore_ascii_case("ffplay.exe") {
            continue;
        }
        let target = dir.join(entry.file_name());
        // Replacing a helper that is playing right now fails on Windows; say so instead of half-installing.
        let _ = std::fs::remove_file(&target);
        std::fs::rename(entry.path(), &target)
            .or_else(|_| std::fs::copy(entry.path(), &target).map(|_| ()))
            .with_context(|| {
                format!(
                    "Close Roy Music, then try again: {} is in use",
                    target.display()
                )
            })?;
    }
    on(Phase::Verifying, 0, None);
    let version = run("ffmpeg", ["-version"], Duration::from_secs(20))?;
    run("ffprobe", ["-version"], Duration::from_secs(20))?;
    let version = String::from_utf8_lossy(&version)
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(2))
        .unwrap_or("")
        .to_owned();
    Ok(serde_json::json!({"path":dir,"version":version,"asset":asset,"sha256":expected}))
}
pub fn install_ffmpeg(client: &reqwest::blocking::Client) -> Result<Value> {
    install_ffmpeg_with(client, &mut |_, _, _| {})
}

pub fn run<I, S>(name: &str, args: I, timeout: Duration) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    run_with_input(name, args, timeout, None)
}
pub fn run_with_input<I, S>(
    name: &str,
    args: I,
    timeout: Duration,
    input: Option<Vec<u8>>,
) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    crate::jobs::check_cancelled()?;
    let output_limit = if input.is_some() {
        4 * 1024 * 1024
    } else {
        256 * 1024 * 1024
    };
    let mut child = command(name)
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Cannot launch {name}; install it or set its RIFF_* path"))?;
    let guard = match crate::process_guard::ProcessGuard::new(&child) {
        Ok(guard) => guard,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.context("Cannot own helper process tree"));
        }
    };
    let writer = input.map(|bytes| {
        use std::io::Write;
        let mut stdin = child.stdin.take().unwrap();
        std::thread::spawn(move || stdin.write_all(&bytes))
    });
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut b = Vec::new();
        stdout.take(output_limit + 1).read_to_end(&mut b).map(|_| b)
    });
    let err = std::thread::spawn(move || {
        let mut b = Vec::new();
        stderr
            .take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut b)
            .map(|_| b)
    });
    let start = Instant::now();
    let status = loop {
        if crate::jobs::check_cancelled().is_err() {
            guard.terminate();
            let _ = child.kill();
            let _ = child.wait();
            let _ = out.join();
            let _ = err.join();
            bail!("Task cancelled");
        }
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if start.elapsed() > timeout {
            guard.terminate();
            let _ = child.kill();
            let _ = child.wait();
            let _ = out.join();
            let _ = err.join();
            bail!("{name} timed out after {} seconds", timeout.as_secs());
        }
        std::thread::sleep(Duration::from_millis(30));
    };
    drop(guard);
    let bytes = out
        .join()
        .map_err(|_| anyhow::anyhow!("stdout reader failed"))??;
    let errors = err
        .join()
        .map_err(|_| anyhow::anyhow!("stderr reader failed"))??;
    if let Some(writer) = writer {
        writer
            .join()
            .map_err(|_| anyhow::anyhow!("Input writer failed"))??;
    }
    anyhow::ensure!(
        bytes.len() as u64 <= output_limit,
        "Helper output exceeded memory limit"
    );
    if !status.success() {
        bail!(
            "{name}: {}",
            String::from_utf8_lossy(&errors)
                .chars()
                .take(1800)
                .collect::<String>()
        );
    }
    Ok(bytes)
}
pub fn json<I, S>(name: &str, args: I) -> Result<Value>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Ok(serde_json::from_slice(&run(
        name,
        args,
        Duration::from_secs(120),
    )?)?)
}
pub fn status() -> Value {
    let entries: serde_json::Map<String, Value> = ["ffmpeg", "ffprobe", "yt-dlp"].into_iter().map(|name| {
        let flag = if name == "yt-dlp" { "--version" } else { "-version" };
        let result = run(name, [flag], Duration::from_secs(10));
        let value = match result { Ok(b) => serde_json::json!({"available":true,"version":String::from_utf8_lossy(&b).lines().next()}), Err(e) => serde_json::json!({"available":false,"error":e.to_string()}) };
        (name.into(), value)
    }).collect();
    Value::Object(entries)
}

#[cfg(test)]
mod installer_tests {
    use super::*;
    #[test]
    fn newest_release_line_wins_and_master_is_the_fallback() {
        let names = [
            "ffmpeg-master-latest-win64-lgpl-shared.zip",
            "ffmpeg-n8.1-latest-win64-lgpl-shared-8.1.zip",
            "ffmpeg-n9.0-latest-win64-lgpl-shared-9.0.zip",
            "ffmpeg-n9.0-latest-win64-lgpl-9.0.zip",
            "ffmpeg-n10.2-latest-win64-gpl-shared-10.2.zip",
            "ffmpeg-n9.0-latest-winarm64-lgpl-shared-9.0.zip",
        ];
        assert_eq!(
            pick_ffmpeg_asset(names, "win64").as_deref(),
            Some("ffmpeg-n9.0-latest-win64-lgpl-shared-9.0.zip")
        );
        assert_eq!(
            pick_ffmpeg_asset(names, "winarm64").as_deref(),
            Some("ffmpeg-n9.0-latest-winarm64-lgpl-shared-9.0.zip")
        );
        assert_eq!(
            pick_ffmpeg_asset(["ffmpeg-master-latest-win64-lgpl-shared.zip"], "win64").as_deref(),
            Some("ffmpeg-master-latest-win64-lgpl-shared.zip")
        );
        assert!(
            pick_ffmpeg_asset(["ffmpeg-n9.0-latest-win64-lgpl-shared-8.1.zip"], "win64").is_none()
        );
    }
    #[test]
    fn checksum_lookup_matches_whole_file_names_only() {
        let manifest = "AA11  ffmpeg-n9.0-latest-win64-lgpl-9.0.zip\nbb22 *ffmpeg-n9.0-latest-win64-lgpl-shared-9.0.zip\n";
        assert_eq!(
            checksum_for(manifest, "ffmpeg-n9.0-latest-win64-lgpl-shared-9.0.zip").as_deref(),
            Some("bb22")
        );
        assert_eq!(
            checksum_for(manifest, "ffmpeg-n9.0-latest-win64-lgpl-9.0.zip").as_deref(),
            Some("aa11")
        );
        assert!(checksum_for(manifest, "ffmpeg-n9.0").is_none());
    }
}
