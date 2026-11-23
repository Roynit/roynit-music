use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
/// Setup tests point every location at one scratch folder so a real installation is never touched.
pub fn test_root() -> Option<PathBuf> {
    std::env::var_os("ROY_SETUP_TEST_ROOT").map(PathBuf::from)
}
/// The installed program is "Roy Music.exe" so Windows (volume mixer, Task Manager) shows its real name.
pub const APP_EXE: &str = "Roy Music.exe";
const PREVIOUS_EXE: &str = "Roy Music.previous.exe";
const CLI_EXE: &str = "roy.exe";
/// File names from before the rename; they are cleaned up when the app is installed over an older version.
const OLD_NAMES: [&str; 3] = ["riff-desktop.exe", "riff-desktop.previous.exe", "riff.exe"];
pub fn install_directory() -> Result<PathBuf> {
    if let Some(root) = test_root() {
        return Ok(root.join("app"));
    }
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
    let destination = target.join(APP_EXE);
    if source.canonicalize().ok() != destination.canonicalize().ok() {
        let mut input = std::fs::File::open(source)?;
        let mut temp = tempfile::NamedTempFile::new_in(target)?;
        std::io::copy(&mut input, &mut temp)?;
        temp.as_file().sync_all()?;
        let installed = [destination.clone(), target.join(OLD_NAMES[0])];
        if let Some(old) = installed.iter().find(|p| p.is_file()) {
            std::fs::copy(old, target.join(PREVIOUS_EXE))?;
        }
        temp.persist(&destination)
            .map_err(|e| e.error)
            .context("Close the installed Roy Music window before replacing its executable")?;
    }
    for old in OLD_NAMES {
        let _ = std::fs::remove_file(target.join(old));
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
        .map(|p| p.join(CLI_EXE))
        .filter(|p| p.is_file())
    {
        let to = target.join(CLI_EXE);
        if cli.canonicalize().ok() != to.canonicalize().ok() {
            std::fs::copy(cli, to)?;
        }
    }
    let previous = target.join(PREVIOUS_EXE);
    let previous_sha256 = if previous.is_file() {
        Some(crate::store::file_hash(&previous)?)
    } else {
        None
    };
    let mut manifest = tempfile::NamedTempFile::new_in(target)?;
    manifest.write_all(serde_json::to_string_pretty(&json!({"version":env!("CARGO_PKG_VERSION"),"files":[APP_EXE,CLI_EXE,"LICENSE","install.json"],"user_data_separate":true,"previous_sha256":previous_sha256}))?.as_bytes())?;
    manifest
        .persist(target.join("install.json"))
        .map_err(|e| e.error)?;
    Ok(destination)
}
/// What an installation adds beyond the program files.
#[derive(Default)]
pub struct Options {
    pub desktop_shortcut: bool,
    /// Bytes of the uninstaller to place next to the app; enables the Apps & features entry.
    pub uninstaller: Option<Vec<u8>>,
}
pub fn install(source: &Path) -> Result<Value> {
    install_with(source, &Options::default())
}
pub fn install_with(source: &Path, options: &Options) -> Result<Value> {
    ensure!(
        !source
            .file_name()
            .is_some_and(|s| s.eq_ignore_ascii_case(CLI_EXE)),
        "Use the desktop application to install Roy Music"
    );
    let target = install_directory()?;
    let executable = copy_bundle(source, &target)?;
    if let Some(bytes) = &options.uninstaller {
        std::fs::write(target.join("uninstall.exe"), bytes)?;
    }
    // Shortcuts and the Apps & features entry belong to the real profile, not to a test folder.
    let real = test_root().is_none();
    let mut shortcut_created = false;
    if real {
        let menu =
            PathBuf::from(std::env::var_os("APPDATA").context("Windows Start menu unavailable")?)
                .join("Microsoft/Windows/Start Menu/Programs/Roy Music.lnk");
        shortcut_created = create_shortcut(&menu, &executable).is_ok();
        if options.desktop_shortcut {
            let _ = create_desktop_shortcut(&executable);
        }
        if options.uninstaller.is_some() {
            register_uninstall(&target)?;
        } else {
            let _ = refresh_registered_version();
        }
    }
    Ok(
        json!({"installed":true,"path":executable,"shortcut_created":shortcut_created,"data_preserved":true}),
    )
}
fn create_shortcut(lnk: &Path, target: &Path) -> Result<()> {
    let script = "$s=(New-Object -ComObject WScript.Shell).CreateShortcut($env:RIFF_SHORTCUT); $s.TargetPath=$env:RIFF_INSTALL_TARGET; $s.WorkingDirectory=[IO.Path]::GetDirectoryName($env:RIFF_INSTALL_TARGET); $s.Description='Roy Music by dang'; $s.IconLocation=$env:RIFF_INSTALL_TARGET+',0'; $s.Save()";
    let result = crate::tools::command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("RIFF_SHORTCUT", lnk)
        .env("RIFF_INSTALL_TARGET", target)
        .output()?;
    ensure!(result.status.success(), "Could not create the shortcut");
    Ok(())
}
fn desktop_folder() -> Option<PathBuf> {
    let out = crate::tools::command("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Environment]::GetFolderPath('Desktop')",
        ])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!path.is_empty()).then(|| PathBuf::from(path))
}
fn create_desktop_shortcut(target: &Path) -> Result<()> {
    let folder = desktop_folder().context("Desktop folder unavailable")?;
    create_shortcut(&folder.join("Roy Music.lnk"), target)
}

const UNINSTALL_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\RoyMusic";
fn reg(args: &[&str]) -> Result<bool> {
    Ok(crate::tools::command("reg.exe")
        .args(args)
        .output()?
        .status
        .success())
}
fn reg_set(name: &str, kind: &str, value: &str) -> Result<()> {
    ensure!(
        reg(&[
            "add",
            UNINSTALL_KEY,
            "/v",
            name,
            "/t",
            kind,
            "/d",
            value,
            "/f"
        ])?,
        "Could not write the uninstall entry ({name})"
    );
    Ok(())
}
/// Make Roy Music appear in Windows Settings → Apps with a working Uninstall button.
pub fn register_uninstall(dir: &Path) -> Result<()> {
    let exe = dir.join(APP_EXE);
    let uninstall = dir.join("uninstall.exe");
    let kb = walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok()?.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum::<u64>()
        / 1024;
    for (name, value) in [
        ("DisplayName", "Roy Music".to_owned()),
        ("DisplayVersion", env!("CARGO_PKG_VERSION").to_owned()),
        ("Publisher", "dang".to_owned()),
        ("InstallLocation", dir.display().to_string()),
        ("DisplayIcon", format!("\"{}\"", exe.display())),
        (
            "UninstallString",
            format!("\"{}\" --uninstall", uninstall.display()),
        ),
        (
            "QuietUninstallString",
            format!("\"{}\" --uninstall --silent", uninstall.display()),
        ),
        (
            "URLInfoAbout",
            "https://roynit.dangz.org/?app=roynit-music".to_owned(),
        ),
    ] {
        reg_set(name, "REG_SZ", &value)?;
    }
    for (name, value) in [
        ("NoModify", 1),
        ("NoRepair", 1),
        ("EstimatedSize", kb as u32),
    ] {
        reg_set(name, "REG_DWORD", &value.to_string())?;
    }
    Ok(())
}
/// An update through the app keeps the existing entry and only advances its version.
fn refresh_registered_version() -> Result<()> {
    if reg(&["query", UNINSTALL_KEY])? {
        reg_set("DisplayVersion", "REG_SZ", env!("CARGO_PKG_VERSION"))?;
    }
    Ok(())
}
/// Version recorded by the installation in `dir`, if there is one.
pub fn installed_version(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("install.json")).ok()?;
    serde_json::from_str::<Value>(&text).ok()?["version"]
        .as_str()
        .map(str::to_owned)
}
/// Process ids of Roy Music running from `dir`.
pub fn running_instances(dir: &Path) -> Vec<u32> {
    let script = "Get-Process -Name 'Roy Music','riff-desktop' -ErrorAction SilentlyContinue | Where-Object { $_.Path -like ($env:RIFF_DIR + '\\*') } | ForEach-Object { $_.Id }";
    let Ok(out) = crate::tools::command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("RIFF_DIR", dir)
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect()
}
/// Ask running copies to close, then end any that stay (the tray keeps a closed window alive).
pub fn close_instances(dir: &Path) -> Result<()> {
    if running_instances(dir).is_empty() {
        return Ok(());
    }
    let script = "Get-Process -Name 'Roy Music','riff-desktop' -ErrorAction SilentlyContinue | Where-Object { $_.Path -like ($env:RIFF_DIR + '\\*') } | ForEach-Object { [void]$_.CloseMainWindow(); if (-not $_.WaitForExit(3000)) { $_.Kill() } }";
    crate::tools::command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("RIFF_DIR", dir)
        .output()?;
    ensure!(
        running_instances(dir).is_empty(),
        "Roy Music is still running. Close it and try again"
    );
    Ok(())
}
/// Remove shortcuts, the Apps entry and the program folder. Library data stays unless asked.
pub fn uninstall(dir: &Path, remove_data: Option<&Path>) -> Result<()> {
    close_instances(dir)?;
    if test_root().is_none() {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            let _ = std::fs::remove_file(
                PathBuf::from(appdata).join("Microsoft/Windows/Start Menu/Programs/Roy Music.lnk"),
            );
        }
        if let Some(folder) = desktop_folder() {
            let _ = std::fs::remove_file(folder.join("Roy Music.lnk"));
        }
        let _ = reg(&["delete", UNINSTALL_KEY, "/f"]);
    }
    if let Some(data) = remove_data {
        let _ = std::fs::remove_dir_all(data);
    }
    let inside = std::env::current_exe()
        .ok()
        .is_some_and(|exe| exe.starts_with(dir));
    if inside {
        // The uninstaller is running from the folder it removes: let this process exit first.
        let script = format!(
            "ping 127.0.0.1 -n 3 >nul & rmdir /s /q \"{}\"",
            dir.display()
        );
        // raw_arg: cmd.exe does not understand the backslash-escaped quotes `args` would add.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            crate::tools::command("cmd.exe")
                .raw_arg(format!("/c {script}"))
                .spawn()?;
        }
    } else if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
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
        "Roy Music did not close before the update timeout"
    );
    Ok(())
}

/// Stage a receipt-verified previous version; never overwrite an executable being read.
pub fn stage_previous(directory: &Path, staging: &Path) -> Result<Value> {
    use std::io::Read;
    let previous = [PREVIOUS_EXE, OLD_NAMES[1]]
        .map(|name| directory.join(name))
        .into_iter()
        .find(|p| p.is_file())
        .unwrap_or_else(|| directory.join(PREVIOUS_EXE));
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

#[cfg(test)]
mod receipt_tests {
    use super::*;
    #[test]
    fn installed_version_comes_from_the_receipt() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(installed_version(dir.path()), None);
        std::fs::write(dir.path().join("install.json"), r#"{"version":"0.1.4"}"#).unwrap();
        assert_eq!(installed_version(dir.path()).as_deref(), Some("0.1.4"));
        std::fs::write(dir.path().join("install.json"), "not json").unwrap();
        assert_eq!(installed_version(dir.path()), None);
    }
    #[test]
    fn uninstall_removes_a_folder_that_is_not_running() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("app");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(APP_EXE), b"x").unwrap();
        // SAFETY: tests in this module are the only ones touching this variable.
        unsafe { std::env::set_var("ROY_SETUP_TEST_ROOT", root.path()) };
        uninstall(&dir, None).unwrap();
        assert!(!dir.exists());
        unsafe { std::env::remove_var("ROY_SETUP_TEST_ROOT") };
    }
}
