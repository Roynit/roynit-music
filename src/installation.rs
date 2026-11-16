use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
pub fn install_directory() -> Result<PathBuf> {
    ensure!(cfg!(windows), "This installer targets Windows");
    Ok(
        PathBuf::from(
            std::env::var_os("LOCALAPPDATA").context("Windows user profile unavailable")?,
        )
        .join("Programs")
        .join("RiffRust"),
    )
}
pub fn copy_bundle(source: &Path, target: &Path) -> Result<PathBuf> {
    ensure!(source.is_file(), "Application executable is missing");
    std::fs::create_dir_all(target)?;
    let destination = target.join("riff-desktop.exe");
    if source.canonicalize().ok() != destination.canonicalize().ok() {
        let mut input = std::fs::File::open(source)?;
        let mut temp = tempfile::NamedTempFile::new_in(target)?;
        std::io::copy(&mut input, &mut temp)?;
        temp.as_file().sync_all()?;
        if destination.is_file() {
            std::fs::copy(&destination, target.join("riff-desktop.previous.exe"))?;
        }
        temp.persist(&destination)
            .map_err(|e| e.error)
            .context("Close the installed Riff window before replacing its executable")?;
    }
    std::fs::write(target.join("LICENSE"), include_str!("../LICENSE"))?;
    std::fs::write(
        target.join("THIRD_PARTY_NOTICES.md"),
        include_str!("../THIRD_PARTY_NOTICES.md"),
    )?;
    std::fs::write(
        target.join("THIRD_PARTY_LICENSES.txt"),
        include_str!("../THIRD_PARTY_LICENSES.txt"),
    )?;
    if let Some(file) = source
        .parent()
        .map(|p| p.join("signing-status.json"))
        .filter(|p| p.is_file())
    {
        std::fs::copy(file, target.join("signing-status.json"))?;
    }
    if let Some(cli) = source
        .parent()
        .map(|p| p.join("riff.exe"))
        .filter(|p| p.is_file())
    {
        let to = target.join("riff.exe");
        if cli.canonicalize().ok() != to.canonicalize().ok() {
            std::fs::copy(cli, to)?;
        }
    }
    let previous = target.join("riff-desktop.previous.exe");
    let previous_sha256 = if previous.is_file() {
        Some(crate::store::file_hash(&previous)?)
    } else {
        None
    };
    let mut manifest = tempfile::NamedTempFile::new_in(target)?;
    manifest.write_all(serde_json::to_string_pretty(&json!({"version":env!("CARGO_PKG_VERSION"),"files":["riff-desktop.exe","riff.exe","LICENSE","install.json"],"user_data_separate":true,"previous_sha256":previous_sha256}))?.as_bytes())?;
    manifest
        .persist(target.join("install.json"))
        .map_err(|e| e.error)?;
    Ok(destination)
}
pub fn install(source: &Path) -> Result<Value> {
    ensure!(
        !source
            .file_name()
            .is_some_and(|s| s.eq_ignore_ascii_case("riff.exe")),
        "Use the desktop application to install Riff"
    );
    let target = install_directory()?;
    let executable = copy_bundle(source, &target)?;
    let menu =
        PathBuf::from(std::env::var_os("APPDATA").context("Windows Start menu unavailable")?)
            .join("Microsoft/Windows/Start Menu/Programs/Roynit Music.lnk");
    let script = "$s=(New-Object -ComObject WScript.Shell).CreateShortcut($env:RIFF_SHORTCUT); $s.TargetPath=$env:RIFF_INSTALL_TARGET; $s.WorkingDirectory=[IO.Path]::GetDirectoryName($env:RIFF_INSTALL_TARGET); $s.Description='Roynit Music by dang'; $s.Save()";
    let result = crate::tools::command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("RIFF_SHORTCUT", &menu)
        .env("RIFF_INSTALL_TARGET", &executable)
        .output()?;
    Ok(
        json!({"installed":true,"path":executable,"shortcut_created":result.status.success(),"data_preserved":true}),
    )
}
pub fn wait_for(pid: u32) -> Result<()> {
    if pid == 0 {
        return Ok(());
    }
    let script = "$p=Get-Process -Id ([int]$env:RIFF_WAIT_PID) -ErrorAction SilentlyContinue; if($p -and -not $p.WaitForExit(60000)){exit 1}";
    let status = crate::tools::command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("RIFF_WAIT_PID", pid.to_string())
        .status()?;
    ensure!(
        status.success(),
        "Riff did not close before the update timeout"
    );
    Ok(())
}

/// Stage a receipt-verified previous version; never overwrite an executable being read.
pub fn stage_previous(directory: &Path, staging: &Path) -> Result<Value> {
    use std::io::Read;
    let previous = directory.join("riff-desktop.previous.exe");
    ensure!(
        std::fs::symlink_metadata(&previous)?.is_file(),
        "No regular previous-version executable"
    );
    ensure!(
        std::fs::metadata(&previous)?.len() <= 256 * 1024 * 1024,
        "Previous version exceeds size limit"
    );
    let receipt = directory.join("install.json");
    ensure!(
        std::fs::metadata(&receipt)?.len() < 65536,
        "Invalid installation receipt"
    );
    let receipt: Value = serde_json::from_reader(std::fs::File::open(receipt)?)?;
    let expected = receipt["previous_sha256"]
        .as_str()
        .context("No verified previous version in this installation")?;
    std::fs::create_dir_all(staging)?;
    let mut staged = tempfile::NamedTempFile::new_in(staging)?;
    let mut input = std::fs::File::open(&previous)?;
    let mut magic = [0; 2];
    input.read_exact(&mut magic)?;
    ensure!(
        &magic == b"MZ",
        "Previous version is not a Windows executable"
    );
    staged.write_all(&magic)?;
    std::io::copy(&mut input, &mut staged)?;
    staged.as_file().sync_all()?;
    ensure!(
        crate::store::file_hash(staged.path())? == expected,
        "Previous executable differs from its installation receipt"
    );
    let path = staging.join(format!("rollback-{}.exe", uuid::Uuid::new_v4()));
    staged.persist(&path).map_err(|e| e.error)?;
    Ok(json!({"staged_path":path,"rollback":true,"sha256_verified":true,"data_preserved":true}))
}
