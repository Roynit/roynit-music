#[cfg(not(windows))]
fn main() {
    eprintln!("The WebView2 playback audit runs on Windows.");
}

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use riff_core::{
        model::{Effects, Track},
        player::Player,
        provider::Providers,
    };
    use serde_json::{Value, json};
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };
    use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
    use windows::{
        Win32::{
            Foundation::{HWND, RECT},
            System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
            UI::WindowsAndMessaging::*,
        },
        core::{HSTRING, Interface, PCWSTR, PWSTR, w},
    };

    let track: Track = serde_json::from_value(json!({
        "id":"youtube:dQw4w9WgXcQ","title":"Never Gonna Give You Up",
        "platform":"youtube","source":"https://www.youtube.com/watch?v=dQw4w9WgXcQ"
    }))?;
    let started = Instant::now();
    let stream = Providers {
        cookies: None,
        proxy: None,
        browser: None,
    }
    .resolve(&track)?;
    let url = stream["url"].as_str().context("Missing audio URL")?;
    let headers: Vec<(String, String)> = stream["headers"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.into())))
        .collect();
    println!(
        "{}",
        json!({"phase":"resolve","ms":started.elapsed().as_secs_f64()*1000.})
    );

    let mut player = Player::default();
    player.select_device(None)?;
    let started = Instant::now();
    player.play(
        track.id,
        url.into(),
        headers,
        stream["duration"].as_f64().unwrap_or(0.),
        Effects {
            muted: true,
            ..Default::default()
        },
    )?;
    while player.position() < 0.01 {
        ensure!(
            started.elapsed() < Duration::from_secs(15) && player.failure().is_empty(),
            "Native playback failed"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    println!(
        "{}",
        json!({"phase":"native_first_pcm","ms":started.elapsed().as_secs_f64()*1000.})
    );
    player.stop();

    struct Com;
    impl Drop for Com {
        fn drop(&mut self) {
            unsafe { CoUninitialize() }
        }
    }
    struct Window(HWND);
    impl Drop for Window {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }
    let started = Instant::now();
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let _com = Com;
        let window = Window(CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!("Roynit audio benchmark"),
            WS_OVERLAPPED,
            0,
            0,
            320,
            100,
            None,
            None,
            None,
            None,
        )?);
        let profile = tempfile::tempdir()?;
        let profile_path = HSTRING::from(profile.path().as_os_str());
        let options: ICoreWebView2EnvironmentOptions =
            CoreWebView2EnvironmentOptions::default().into();
        options.SetAdditionalBrowserArguments(w!("--autoplay-policy=no-user-gesture-required"))?;
        let (tx, rx) = mpsc::channel();
        CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| {
                CreateCoreWebView2EnvironmentWithOptions(
                    PCWSTR::null(),
                    &profile_path,
                    &options,
                    &handler,
                )
                .map_err(webview2_com::Error::WindowsError)
            }),
            Box::new(move |status, environment| {
                status?;
                let _ = tx.send(environment);
                Ok(())
            }),
        )
        .context("WebView2 Runtime unavailable")?;
        let environment = rx.try_recv()?.context("No WebView2 environment")?;
        let (tx, rx) = mpsc::channel();
        let hwnd = window.0;
        CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| {
                environment
                    .CreateCoreWebView2Controller(hwnd, &handler)
                    .map_err(webview2_com::Error::WindowsError)
            }),
            Box::new(move |status, controller| {
                status?;
                let _ = tx.send(controller);
                Ok(())
            }),
        )?;
        let controller = rx.try_recv()?.context("No WebView2 controller")?;
        let _ = ShowWindow(window.0, SW_SHOWNOACTIVATE);
        controller.SetIsVisible(true)?;
        controller.SetBounds(RECT {
            left: 0,
            top: 0,
            right: 320,
            bottom: 100,
        })?;
        let view = controller.CoreWebView2()?;
        let settings = view.Settings()?;
        settings.SetAreDevToolsEnabled(false)?;
        if let Some(agent) = stream["headers"]["User-Agent"].as_str() {
            settings
                .cast::<ICoreWebView2Settings2>()?
                .SetUserAgent(&HSTRING::from(agent))?;
        }
        let (tx, rx) = mpsc::channel();
        let mut token = 0;
        view.add_WebMessageReceived(
            &WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    let mut ptr = PWSTR::null();
                    args.WebMessageAsJson(&mut ptr)?;
                    let value = CoTaskMemPWSTR::from(ptr);
                    let _ = tx.send(value.to_string());
                }
                Ok(())
            })),
            &mut token,
        )?;
        let html = r#"<!doctype html><meta charset=utf-8><script>
const player = new Audio();
player.muted = true;
document.addEventListener('DOMContentLoaded', () => document.body.append(player));
const send = (data) => chrome.webview.postMessage(data);
chrome.webview.addEventListener('message', ({data}) => {
  const start = performance.now();
  player.onplaying = () => send({event:'playing',ms:performance.now()-start});
  player.onerror = () => send({event:'error',code:player.error?.code});
  player.src = data.url;
  player.play().catch(error => send({event:'error',name:error.name}));
});
send({event:'ready'});
</script>"#;
        view.NavigateToString(&HSTRING::from(html))?;
        let wait = || -> anyhow::Result<Value> {
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                let mut message = MSG::default();
                while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                if let Ok(message) = rx.try_recv() {
                    return Ok(serde_json::from_str(&message)?);
                }
                ensure!(
                    Instant::now() < deadline,
                    "WebView2 playback audit timed out"
                );
                std::thread::sleep(Duration::from_millis(2));
            }
        };
        ensure!(
            wait()?["event"] == "ready",
            "WebView2 failed to initialize audio"
        );
        println!(
            "{}",
            json!({"phase":"webview_ready","ms":started.elapsed().as_secs_f64()*1000.})
        );
        view.PostWebMessageAsJson(&HSTRING::from(json!({"url":url}).to_string()))?;
        let result = wait()?;
        println!(
            "{}",
            json!({"phase":"webview_first_playing","result":result})
        );
        view.remove_WebMessageReceived(token)?;
        controller.Close()?;
        ensure!(
            result["event"] == "playing",
            "WebView2 could not play the audio stream"
        );
    }
    Ok(())
}
