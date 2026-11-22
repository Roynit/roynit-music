use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::Path,
};
pub const DEFAULT_REPO: &str = "Roynit/roynit-music";
pub const ASSET: &str = "riff-rust-windows-x64.exe";
pub const MUSIC_ORIGIN: &str = "https://roynit.dangz.org";
pub fn music_release_info(value: &Value, current: &str) -> Result<Value> {
    let tag = value["version"]
        .as_str()
        .context("Missing Roy Music version")?;
    let parsed = version(tag)?;
    let download = format!("{MUSIC_ORIGIN}/get/roynit-music/{tag}");
    let checksum = format!("{MUSIC_ORIGIN}/api/library/roynit-music/checksum/{tag}");
    let hash = value["sha256"]
        .as_str()
        .context("Missing Roy Music checksum")?;
    ensure!(
        hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid Roy Music checksum"
    );
    ensure!(
        value["filename"]
            .as_str()
            .is_some_and(|s| s.ends_with(".exe")),
        "Roy Music update must be an executable"
    );
    Ok(
        json!({"configured":true,"available":parsed>version(current)?,"version":tag,"current":current,"download":download,"checksum":checksum,"notes":value["notes"],"release_url":MUSIC_ORIGIN,"source":"Roynit Cloudflare"}),
    )
}
pub fn validate_repo(repo: &str) -> Result<()> {
    let parts: Vec<_> = repo.split('/').collect();
    ensure!(
        parts.len() == 2
            && parts.iter().all(|p| !p.is_empty()
                && p.len() <= 100
                && p.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))),
        "Use owner/repository"
    );
    ensure!(
        !repo.eq_ignore_ascii_case("rootscripts/riff"),
        "Original Riff releases contain Electron builds; configure a Rust release repository"
    );
    Ok(())
}
fn version(text: &str) -> Result<(u32, u32, u32)> {
    let parts: Vec<_> = text.trim_start_matches('v').split('.').collect();
    ensure!(parts.len() == 3, "Expected stable version x.y.z");
    Ok((parts[0].parse()?, parts[1].parse()?, parts[2].parse()?))
}
pub fn release_info(repo: &str, value: &Value, current: &str) -> Result<Value> {
    validate_repo(repo)?;
    let tag = value["tag_name"]
        .as_str()
        .context("Missing release version")?;
    ensure!(
        value["draft"] != true && value["prerelease"] != true,
        "Only stable published releases are supported"
    );
    let assets = value["assets"]
        .as_array()
        .context("Release has no assets")?;
    let find = |name: &str| -> Result<String> {
        let url = assets
            .iter()
            .find(|a| a["name"] == name)
            .and_then(|a| a["browser_download_url"].as_str())
            .context("Rust Windows executable and SHA-256 asset are required")?;
        ensure!(
            url.starts_with(&format!("https://github.com/{repo}/releases/download/")),
            "Release asset is not from the configured repository"
        );
        Ok(url.into())
    };
    let download = find(ASSET)?;
    let checksum = find(&format!("{ASSET}.sha256"))?;
    Ok(
        json!({"configured":true,"available":version(tag)?>version(current)?,"version":tag,"current":current,"download":download,"checksum":checksum,"notes":value["body"],"release_url":value["html_url"]}),
    )
}
pub fn check(client: &reqwest::blocking::Client, repo: Option<&str>) -> Result<Value> {
    let Some(repo) = repo.filter(|r| !r.is_empty()) else {
        return Ok(
            json!({"configured":false,"available":false,"reason":"Configure a Rust release repository when ready"}),
        );
    };
    validate_repo(repo)?;
    if repo.eq_ignore_ascii_case(DEFAULT_REPO) {
        let response = client
            .get(format!("{MUSIC_ORIGIN}/api/library/roynit-music/update"))
            .send()?;
        if response.status() == reqwest::StatusCode::NOT_FOUND
            || response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE
        {
            return Ok(
                json!({"configured":true,"available":false,"reason":"Roy Music has no published release yet."}),
            );
        }
        return music_release_info(
            &response.error_for_status()?.json()?,
            env!("CARGO_PKG_VERSION"),
        );
    }
    let response = client
        .get(format!(
            "https://api.github.com/repos/{repo}/releases/latest"
        ))
        .header("Accept", "application/vnd.github+json")
        .send()?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(
            json!({"configured":true,"available":false,"reason":"No public release has been published yet, or this repository is not publicly accessible."}),
        );
    }
    let release: Value = response.error_for_status()?.json()?;
    release_info(repo, &release, env!("CARGO_PKG_VERSION"))
}
pub fn verified_file(
    root: &Path,
    version: &str,
    bytes: &[u8],
    checksum: &str,
) -> Result<std::path::PathBuf> {
    let parsed = version.trim_start_matches('v');
    let _ = self::version(parsed)?;
    ensure!(
        bytes.starts_with(b"MZ") && bytes.len() <= 256 * 1024 * 1024,
        "Invalid Windows update executable"
    );
    let expected = checksum
        .split_whitespace()
        .next()
        .context("Empty checksum")?;
    ensure!(
        expected.len() == 64 && expected.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid SHA-256 manifest"
    );
    let actual = format!("{:x}", Sha256::digest(bytes));
    ensure!(
        actual.eq_ignore_ascii_case(expected),
        "Update checksum mismatch; file was not staged"
    );
    let directory = root.join("updates");
    std::fs::create_dir_all(&directory)?;
    let target = directory.join(format!("riff-rust-{parsed}-windows-x64.exe"));
    let mut temp = tempfile::NamedTempFile::new_in(&directory)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(&target).map_err(|e| e.error)?;
    Ok(target)
}
pub fn stage(client: &reqwest::blocking::Client, root: &Path, repo: Option<&str>) -> Result<Value> {
    let mut info = check(client, repo)?;
    ensure!(
        info["available"] == true,
        "No newer Rust release is available"
    );
    crate::jobs::check_cancelled()?;
    let response = client
        .get(info["checksum"].as_str().context("No checksum URL")?)
        .send()?
        .error_for_status()?;
    let mut checksum = String::new();
    response.take(4097).read_to_string(&mut checksum)?;
    ensure!(checksum.len() < 4096, "Checksum manifest too large");
    let response = client
        .get(info["download"].as_str().context("No download URL")?)
        .send()?
        .error_for_status()?;
    ensure!(
        response.url().scheme() == "https",
        "Update download must remain HTTPS"
    );
    let mut bytes = Vec::new();
    let mut reader = response.take(256 * 1024 * 1024 + 1);
    let mut buffer = [0; 65536];
    loop {
        crate::jobs::check_cancelled()?;
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        ensure!(bytes.len() <= 256 * 1024 * 1024, "Update is too large");
    }
    crate::jobs::check_cancelled()?;
    let path = verified_file(
        root,
        info["version"].as_str().context("No version")?,
        &bytes,
        &checksum,
    )?;
    info["staged_path"] = json!(path);
    info["verified"] = json!(true);
    Ok(info)
}
