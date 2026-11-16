use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{
    ffi::OsStr,
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

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

/// Download an upstream release and verify its SHA-256 manifest before publishing it.
pub fn install_ytdlp(client: &reqwest::blocking::Client) -> Result<Value> {
    use sha2::{Digest, Sha256};
    use std::io::Write;
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
    let expected = manifest
        .lines()
        .find_map(|row| {
            let mut p = row.split_whitespace();
            let hash = p.next()?;
            let name = p.next()?.trim_start_matches('*');
            (name == asset).then_some(hash.to_ascii_lowercase())
        })
        .context("No SHA-256 checksum for release asset")?;
    let dir = managed_dir()?;
    std::fs::create_dir_all(&dir)?;
    let mut response = client
        .get(format!("{base}/{asset}"))
        .timeout(Duration::from_secs(180))
        .send()?
        .error_for_status()?;
    let mut file = tempfile::NamedTempFile::new_in(&dir)?;
    let mut hash = Sha256::new();
    let mut block = [0u8; 65536];
    let mut bytes = 0usize;
    loop {
        let n = response.read(&mut block)?;
        if n == 0 {
            break;
        }
        bytes += n;
        anyhow::ensure!(bytes <= 150 * 1024 * 1024, "Release asset exceeds 150 MB");
        hash.update(&block[..n]);
        file.write_all(&block[..n])?;
    }
    anyhow::ensure!(
        format!("{:x}", hash.finalize()) == expected,
        "yt-dlp SHA-256 mismatch"
    );
    file.as_file().sync_all()?;
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
