//! One Roy Music per data folder. A second launch (for example the shortcut, after the app already started
//! with Windows and is waiting in the tray) wakes the first one and quits quietly instead of opening a
//! second window or failing on the locked library.
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Nothing else is running for this data folder; carry on.
    First,
    /// Another copy already owns it.
    Second,
}

/// A short name for a data folder, so copies using different folders (tests, a portable copy) do not meet.
pub fn key_for(root: &Path) -> String {
    let text = root.to_string_lossy().to_lowercase();
    crate::store::key(&text).chars().take(16).collect()
}

#[cfg(windows)]
mod imp {
    use super::Role;
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt, ptr::null, sync::Mutex};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError},
        System::Threading::{
            CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, SetEvent, WaitForSingleObject,
        },
        UI::WindowsAndMessaging::AllowSetForegroundWindow,
    };

    /// Handles that must live as long as the process: closing the mutex would let a second copy in.
    static HELD: Mutex<Vec<usize>> = Mutex::new(Vec::new());

    fn wide(text: &str) -> Vec<u16> {
        OsStr::new(text).encode_wide().chain(Some(0)).collect()
    }
    fn mutex_name(key: &str) -> Vec<u16> {
        wide(&format!("Local\\RoyMusic-{key}"))
    }
    fn event_name(key: &str) -> Vec<u16> {
        wide(&format!("Local\\RoyMusic-wake-{key}"))
    }

    pub fn claim(key: &str) -> Role {
        let name = mutex_name(key);
        // SAFETY: plain Win32 calls with a NUL-terminated name; the last error is read right after the call.
        unsafe {
            let mutex = CreateMutexW(null(), 0, name.as_ptr());
            let exists = GetLastError() == ERROR_ALREADY_EXISTS;
            if mutex.is_null() {
                return Role::First;
            }
            if exists {
                CloseHandle(mutex);
                return Role::Second;
            }
            // Created now, so a second launch that comes a moment later can already wake this one.
            let wake = CreateEventW(null(), 0, 0, event_name(key).as_ptr());
            if let Ok(mut held) = HELD.lock() {
                held.push(mutex as usize);
                if !wake.is_null() {
                    held.push(wake as usize);
                }
            }
            Role::First
        }
    }

    pub fn wake_first(key: &str) {
        // SAFETY: as above. The first copy may bring its window forward because this process was just started
        // by the user, which Windows treats as permission to hand the foreground over.
        unsafe {
            AllowSetForegroundWindow(u32::MAX);
            let wake = OpenEventW(EVENT_MODIFY_STATE, 0, event_name(key).as_ptr());
            if !wake.is_null() {
                SetEvent(wake);
                CloseHandle(wake);
            }
        }
    }

    pub fn on_wake(key: &str, wake: impl Fn() + Send + 'static) {
        // SAFETY: opens the event made by `claim` (or makes it), then waits on it from its own thread.
        let event = unsafe { CreateEventW(null(), 0, 0, event_name(key).as_ptr()) } as usize;
        if event == 0 {
            return;
        }
        std::thread::spawn(move || {
            // SAFETY: the handle stays open for the life of the process.
            while unsafe { WaitForSingleObject(event as _, INFINITE) } == 0 {
                wake();
            }
        });
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Role;
    pub fn claim(_: &str) -> Role {
        Role::First
    }
    pub fn wake_first(_: &str) {}
    pub fn on_wake(_: &str, _: impl Fn() + Send + 'static) {}
}

pub use imp::{claim, on_wake, wake_first};

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{sync::mpsc, time::Duration};

    #[test]
    fn the_second_copy_is_turned_away_and_wakes_the_first() {
        let key = format!("test-{}", std::process::id());
        assert_eq!(claim(&key), Role::First);
        assert_eq!(claim(&key), Role::Second);
        assert_eq!(claim(&format!("{key}-other")), Role::First, "another data folder is independent");
        let (tx, rx) = mpsc::channel();
        on_wake(&key, move || {
            let _ = tx.send(());
        });
        wake_first(&key);
        assert!(rx.recv_timeout(Duration::from_secs(3)).is_ok(), "the first copy was woken");
    }

    #[test]
    fn data_folders_get_their_own_names() {
        assert_ne!(key_for(Path::new(r"C:\a")), key_for(Path::new(r"C:\b")));
        assert_eq!(key_for(Path::new(r"C:\A")), key_for(Path::new(r"c:\a")));
    }
}
