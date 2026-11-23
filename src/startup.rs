//! Starting with Windows: the "Run" entry that Task Manager's Startup tab lists, and the on/off switch that
//! Task Manager keeps next to it. Switching it off in Task Manager leaves the entry in place but flips that
//! switch, so both have to be read to know whether Roy Music really starts with Windows.
use anyhow::{Result, ensure};
use std::path::Path;

const RUN: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const NAME: &str = "RoyMusic";
/// Task Manager keeps 12 bytes per entry. The first is 02 while it is on and 03 once the user turned it off.
const ON: &str = "020000000000000000000000";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// There is a startup entry for Roy Music.
    pub registered: bool,
    /// The user switched that entry off in Task Manager.
    pub switched_off: bool,
}

impl Status {
    pub fn enabled(self) -> bool {
        self.registered && !self.switched_off
    }
}

/// What Windows runs at sign-in. `--startup` makes the app open quietly in the tray.
pub fn command(exe: &Path) -> String {
    format!("\"{}\" --startup", exe.display())
}

fn reg(args: &[&str]) -> Option<String> {
    let output = crate::tools::command("reg.exe").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The data of one value in `reg query` output (`    NAME    REG_SZ    data`).
fn value_of(output: &str, name: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let rest = line.trim_start().strip_prefix(name)?;
        let rest = rest.strip_prefix(char::is_whitespace)?.trim_start();
        let (_kind, data) = rest.split_once(char::is_whitespace)?;
        Some(data.trim().to_owned())
    })
}

/// A switch value is "off" when the low bit of its first byte is set (03, 07).
fn switched_off(hex: &str) -> bool {
    hex.get(..2)
        .and_then(|byte| u8::from_str_radix(byte, 16).ok())
        .is_some_and(|byte| byte & 1 == 1)
}

fn registered_command() -> Option<String> {
    value_of(&reg(&["query", RUN, "/v", NAME])?, NAME)
}

pub fn status() -> Status {
    Status {
        registered: registered_command().is_some(),
        switched_off: reg(&["query", APPROVED, "/v", NAME])
            .and_then(|output| value_of(&output, NAME))
            .is_some_and(|hex| switched_off(&hex)),
    }
}

/// Turns it on, including the switch Task Manager may have left off.
pub fn enable(exe: &Path) -> Result<()> {
    let done = reg(&["add", RUN, "/v", NAME, "/t", "REG_SZ", "/d", &command(exe), "/f"]).is_some()
        && reg(&["add", APPROVED, "/v", NAME, "/t", "REG_BINARY", "/d", ON, "/f"]).is_some();
    ensure!(done, "Could not turn on starting with Windows");
    Ok(())
}

/// Turns it off and forgets it, so nothing is left in Task Manager's list.
pub fn disable() {
    let _ = reg(&["delete", RUN, "/v", NAME, "/f"]);
    let _ = reg(&["delete", APPROVED, "/v", NAME, "/f"]);
}

/// Points an existing entry at this copy of the app (the program moved or was renamed), leaving the
/// on/off switch alone.
pub fn repair(exe: &Path) {
    if let Some(current) = registered_command()
        && current != command(exe)
    {
        let _ = reg(&["add", RUN, "/v", NAME, "/t", "REG_SZ", "/d", &command(exe), "/f"]);
    }
}

/// Only the installed copy registers itself; a build run from a source folder never does.
pub fn is_installed_copy() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Ok(dir) = crate::installation::install_directory() else {
        return false;
    };
    let same = |a: &Path, b: &Path| matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b);
    // ROY_STARTUP_TEST lets a build run from a source folder try the feature by hand.
    cfg!(windows)
        && (std::env::var_os("ROY_STARTUP_TEST").is_some()
            || (crate::installation::test_root().is_none() && same(&exe, &dir.join(crate::installation::APP_EXE))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_is_read_out_of_reg_output() {
        let run = "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\r\n    RoyMusic    REG_SZ    \"C:\\Users\\me\\Roy Music.exe\" --startup\r\n\r\n";
        assert_eq!(value_of(run, NAME).as_deref(), Some("\"C:\\Users\\me\\Roy Music.exe\" --startup"));
        assert_eq!(value_of(run, "Other"), None);
        assert_eq!(value_of("    RoyMusicOld    REG_SZ    x", NAME), None, "a longer name is not this entry");
    }

    #[test]
    fn task_manager_turns_it_off_with_an_odd_first_byte() {
        assert!(!switched_off("020000000000000000000000"));
        assert!(!switched_off("060000000000000000000000"));
        assert!(switched_off("030000000000000000000000"));
        assert!(switched_off("07AABBCCDDEEFF001122334455"));
        assert!(!switched_off(""));
        let on = Status { registered: true, switched_off: false };
        assert!(on.enabled());
        assert!(!Status { registered: true, switched_off: true }.enabled());
        assert!(!Status { registered: false, switched_off: false }.enabled());
    }

    #[test]
    fn windows_starts_the_app_quietly() {
        assert_eq!(command(Path::new(r"C:\Apps\Roy Music.exe")), "\"C:\\Apps\\Roy Music.exe\" --startup");
    }
}

#[cfg(all(test, windows))]
mod registry_tests {
    use super::*;

    /// Writes to the real Run key (then removes it again), so it only runs when asked: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn the_real_registry_follows_task_manager() {
        let before = status();
        let exe = Path::new(r"C:\Apps\Roy Music.exe");
        disable();
        assert_eq!(status(), Status::default());
        enable(exe).unwrap();
        assert!(status().enabled());
        assert_eq!(registered_command().as_deref(), Some(command(exe).as_str()));
        // What Task Manager does when the person switches it off.
        reg(&["add", APPROVED, "/v", NAME, "/t", "REG_BINARY", "/d", "030000000000000000000000", "/f"]).unwrap();
        assert_eq!(status(), Status { registered: true, switched_off: true });
        // Moving the program re-points the entry without turning it back on.
        repair(Path::new(r"D:\Elsewhere\Roy Music.exe"));
        assert_eq!(registered_command().as_deref(), Some(command(Path::new(r"D:\Elsewhere\Roy Music.exe")).as_str()));
        assert!(!status().enabled());
        enable(exe).unwrap();
        assert!(status().enabled(), "turning it on again clears the Task Manager switch");
        disable();
        assert_eq!(status(), Status::default());
        if before.enabled() {
            let _ = enable(&std::env::current_exe().unwrap());
        }
    }
}
