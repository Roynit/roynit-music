use anyhow::Result;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
pub struct MediaKeys {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl MediaKeys {
    #[cfg(windows)]
    pub fn start(callback: impl Fn(&str) + Send + 'static) -> Result<Self> {
        use windows_sys::Win32::UI::{
            Input::KeyboardAndMouse::{
                MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey, VK_MEDIA_NEXT_TRACK,
                VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK, VK_MEDIA_STOP,
            },
            WindowsAndMessaging::{MSG, PM_REMOVE, PeekMessageW, WM_HOTKEY},
        };
        let stop = Arc::new(AtomicBool::new(false));
        let control = stop.clone();
        let (ready, rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let keys = [
                (1, VK_MEDIA_PLAY_PAUSE, "player.toggle"),
                (2, VK_MEDIA_NEXT_TRACK, "player.next"),
                (3, VK_MEDIA_PREV_TRACK, "player.previous"),
                (4, VK_MEDIA_STOP, "player.stop"),
            ];
            let mut registered = Vec::new();
            for (id, key, _) in keys {
                if unsafe { RegisterHotKey(std::ptr::null_mut(), id, MOD_NOREPEAT, key as u32) }
                    == 0
                {
                    let error = std::io::Error::last_os_error().to_string();
                    for id in registered {
                        unsafe {
                            UnregisterHotKey(std::ptr::null_mut(), id);
                        }
                    }
                    let _ = ready.send(Err(error));
                    return;
                }
                registered.push(id);
            }
            let _ = ready.send(Ok(()));
            while !control.load(Ordering::Relaxed) {
                let mut msg: MSG = unsafe { std::mem::zeroed() };
                while unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0
                {
                    if msg.message == WM_HOTKEY
                        && let Some((_, _, command)) =
                            keys.iter().find(|(id, _, _)| *id as usize == msg.wParam)
                    {
                        callback(command);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            for id in registered {
                unsafe {
                    UnregisterHotKey(std::ptr::null_mut(), id);
                }
            }
        });
        match rx.recv_timeout(std::time::Duration::from_secs(2)) {
            Ok(Ok(())) => Ok(Self {
                stop,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                stop.store(true, Ordering::Relaxed);
                let _ = thread.join();
                anyhow::bail!("Cannot register media keys (another player may own them): {e}")
            }
            Err(e) => {
                stop.store(true, Ordering::Relaxed);
                anyhow::bail!("Media-key initialization failed: {e}")
            }
        }
    }
    #[cfg(not(windows))]
    pub fn start(_callback: impl Fn(&str) + Send + 'static) -> Result<Self> {
        anyhow::bail!("Global media keys are not configured for this platform")
    }
}
impl Drop for MediaKeys {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
