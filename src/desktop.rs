#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod system_media;
use eframe::egui::{self, Color32, RichText};
use riff_core::{
    Engine,
    lyrics::Line,
    model::{Effects, Library, Track},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

enum Message {
    Response {
        tag: String,
        result: Result<Value, String>,
    },
    Event(Value),
    Library(Box<Library>),
    Player(Value),
    Fatal(String),
}
struct Request {
    tag: String,
    command: String,
    params: Value,
}
fn worker(
    root: PathBuf,
    requests: mpsc::Receiver<Request>,
    events: mpsc::Sender<Message>,
    shutdown: Arc<AtomicBool>,
) {
    riff_core::jobs::set_thread_cancel_token(shutdown.clone());
    let mut engine = match Engine::open(root) {
        Ok(e) => e,
        Err(e) => {
            let _ = events.send(Message::Fatal(format!("{e:#}")));
            return;
        }
    };
    let _ = events.send(Message::Library(Box::new(engine.store.data.clone())));
    let mut last = Instant::now();
    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        match requests.recv_timeout(Duration::from_millis(30)) {
            Ok(mut r) => {
                r.command = match r.command.as_str() {
                    "player.next" => "player.next.request".into(),
                    "player.previous" => "player.previous.request".into(),
                    "session.resume" => "session.resume.request".into(),
                    _ => r.command,
                };
                if r.command == "queue.set" && r.params["play"] == true {
                    r.command = "queue.play".into();
                }
                if r.command == "app.quit" {
                    break;
                }
                let result = engine
                    .dispatch(&json!({"command":r.command,"params":r.params}))
                    .map_err(|e| format!("{e:#}"));
                if events
                    .send(Message::Response { tag: r.tag, result })
                    .is_err()
                {
                    break;
                }
                let _ = events.send(Message::Library(Box::new(engine.store.data.clone())));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        match engine.tick() {
            Ok(Some(event)) => {
                let _ = events.send(Message::Event(event));
                let _ = events.send(Message::Library(Box::new(engine.store.data.clone())));
            }
            Err(e) => {
                engine.player.stop();
                let _ = events.send(Message::Fatal(format!("{e:#}")));
            }
            _ => {}
        }
        if last.elapsed() > Duration::from_millis(150) {
            let mut state = engine.snapshot();
            if engine
                .store
                .data
                .settings
                .get("appearance")
                .and_then(|a| a["visualizer"].as_str())
                .is_some_and(|v| v != "off")
            {
                state["analysis"] = engine.player.analysis();
            }
            if events.send(Message::Player(state)).is_err() {
                break;
            }
            last = Instant::now();
        }
    }
    let _ = engine.persist_session();
    engine.player.stop();
    let _ = engine.store.save();
}

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Home,
    Search,
    Library,
    History,
    Offline,
    Favorites,
    Playlists,
    Queue,
    Lyrics,
    Downloads,
    Settings,
    Browse,
}
impl Page {
    fn title(self) -> &'static str {
        match self {
            Self::Home => "Listen now",
            Self::Search => "Search",
            Self::Library => "Your library",
            Self::History => "Recently played",
            Self::Offline => "Offline music",
            Self::Favorites => "Favorites",
            Self::Playlists => "Playlists",
            Self::Queue => "Up next",
            Self::Lyrics => "Lyrics",
            Self::Downloads => "Downloads & tasks",
            Self::Settings => "Settings",
            Self::Browse => "YouTube Music collection",
        }
    }
}
type NavigationEntry = (Page, Vec<Track>, Vec<Value>, String, Option<Value>);
struct App {
    media: Option<system_media::Session>,
    tx: mpsc::Sender<Request>,
    rx: mpsc::Receiver<Message>,
    page: Page,
    query: String,
    platform: usize,
    library: Library,
    player: Value,
    tracks: Vec<Track>,
    items: Vec<Value>,
    status: String,
    error: Option<String>,
    job_tags: HashMap<String, String>,
    jobs: HashMap<String, Value>,
    busy: bool,
    lyric_lines: Vec<Line>,
    lyric_text: String,
    lyric_for: Option<String>,
    playlist_name: String,
    selected: Vec<String>,
    active_playlist: Option<String>,
    effects: Effects,
    seek_preview: f64,
    browse_title: String,
    data_root: PathBuf,
    import_url: String,
    oauth_client: String,
    oauth_secret: String,
    oauth_provider: usize,
    search_filter: String,
    search_limit: usize,
    hover_candidate: Option<(Track, Instant)>,
    hover_warmed: Option<String>,
    current_browse: Option<Value>,
    shutdown: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
    follow_lyrics: bool,
    lyric_offset: f64,
    effects_pending: Option<Instant>,
    compact: bool,
    fullscreen: bool,
    results_generation: u64,
    plugins: Vec<Value>,
    lyric_source: String,
    translated: String,
    translation_target: String,
    translation_source: String,
    google_playlists: Vec<Value>,
    spotify_playlists: Vec<Value>,
    _tray: Option<tray_icon::TrayIcon>,
    close_to_tray: bool,
    media_keys: Option<riff_core::media_keys::MediaKeys>,
    native_preferences_loaded: bool,
    lastfm_key: String,
    lastfm_secret: String,
    lastfm_status: Value,
    extension_status: Value,
    account_status: Value,
    login_cancel: Option<Arc<AtomicBool>>,
    login_thread: Option<std::thread::JoinHandle<()>>,
    retry_login: bool,
    login_generation: u64,
    discord_id: String,
    devices: Vec<String>,
    selected_device: String,
    sleep_minutes: u32,
    cache_info: Value,
    cache_limit_mb: u64,
    proxy_input: String,
    network_info: Value,
    cookie_browser: String,
    appearance: riff_core::appearance::Appearance,
    appearance_signature: String,
    themes: Vec<Value>,
    theme_name: String,
    release_repo: String,
    update_info: Value,
    update_checked: bool,
    library_filter: String,
    library_sort: usize,
    rename_playlist: String,
    navigation: Vec<NavigationEntry>,
    lyric_generation: u64,
    translation_generation: u64,
}
impl App {
    fn new(cc: &eframe::CreationContext<'_>, root: PathBuf) -> Self {
        let ctx = &cc.egui_ctx;
        egui_extras::install_image_loaders(ctx);
        let mut fonts = egui::FontDefinitions::default();
        if let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\segoeui.ttf") {
            fonts
                .font_data
                .insert("system".into(), egui::FontData::from_owned(bytes).into());
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "system".into());
        }
        ctx.set_fonts(fonts);
        let mut style = (*ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = Color32::from_rgb(19, 20, 24);
        style.visuals.window_fill = Color32::from_rgb(27, 28, 34);
        style.visuals.selection.bg_fill = Color32::from_rgb(103, 76, 181);
        style.visuals.widgets.active.bg_fill = Color32::from_rgb(112, 80, 197);
        style.spacing.item_spacing = egui::vec2(10., 10.);
        style.spacing.button_padding = egui::vec2(13., 9.);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(15.));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(15.));
        ctx.set_style(style);
        let (tx, requests) = mpsc::channel();
        let (events, rx) = mpsc::channel();
        let data_root = root.clone();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_worker = shutdown.clone();
        let worker = Some(std::thread::spawn(move || {
            worker(root, requests, events, shutdown_worker)
        }));
        let tray = (|| -> anyhow::Result<tray_icon::TrayIcon> {
            use tray_icon::menu::{Menu, MenuEvent, MenuItem};
            let menu = Menu::new();
            for (id, label) in [
                ("riff_show", "Show Riff"),
                ("riff_toggle", "Play / pause"),
                ("riff_next", "Next track"),
                ("riff_quit", "Quit Riff"),
            ] {
                menu.append(&MenuItem::with_id(id, label, true, None))?;
            }
            let context = ctx.clone();
            let sender = tx.clone();
            let quit = shutdown.clone();
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                match event.id.0.as_str() {
                    "riff_show" => {
                        context.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        context.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    "riff_quit" => {
                        quit.store(true, Ordering::Relaxed);
                        context.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    "riff_toggle" | "riff_next" => {
                        let _ = sender.send(Request {
                            tag: "tray".into(),
                            command: if event.id.0 == "riff_toggle" {
                                "player.toggle"
                            } else {
                                "player.next"
                            }
                            .into(),
                            params: json!({}),
                        });
                    }
                    _ => {}
                }
                context.request_repaint();
            }));
            let context = ctx.clone();
            tray_icon::TrayIconEvent::set_event_handler(Some(move |event| {
                if matches!(event, tray_icon::TrayIconEvent::DoubleClick { .. }) {
                    context.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    context.send_viewport_cmd(egui::ViewportCommand::Focus);
                    context.request_repaint();
                }
            }));
            let image = image::load_from_memory(include_bytes!("../assets/icon.png"))?.into_rgba8();
            let (width, height) = image.dimensions();
            let icon = tray_icon::Icon::from_rgba(image.into_raw(), width, height)?;
            Ok(tray_icon::TrayIconBuilder::new()
                .with_tooltip("Roynit Music")
                .with_menu(Box::new(menu))
                .with_icon(icon)
                .build()?)
        })()
        .ok();
        let media = system_media::Session::new(cc, tx.clone());
        let media_error = media.as_ref().err().cloned();
        let mut app = Self {
            media: media.ok(),
            tx,
            rx,
            page: Page::Home,
            query: String::new(),
            platform: 0,
            library: Library::default(),
            player: json!({}),
            tracks: vec![],
            items: vec![],
            status: "Connecting to YouTube Music…".into(),
            error: None,
            job_tags: HashMap::new(),
            jobs: HashMap::new(),
            busy: false,
            lyric_lines: vec![],
            lyric_text: String::new(),
            lyric_for: None,
            playlist_name: String::new(),
            selected: vec![],
            active_playlist: None,
            effects: Effects::default(),
            seek_preview: 0.,
            browse_title: String::new(),
            data_root,
            import_url: String::new(),
            oauth_client: String::new(),
            oauth_secret: String::new(),
            oauth_provider: 0,
            search_filter: "songs".into(),
            search_limit: 10,
            hover_candidate: None,
            hover_warmed: None,
            current_browse: None,
            shutdown,
            worker,
            follow_lyrics: true,
            lyric_offset: 0.,
            effects_pending: None,
            compact: false,
            fullscreen: false,
            results_generation: 0,
            plugins: vec![],
            lyric_source: "auto".into(),
            translated: String::new(),
            translation_target: "vi".into(),
            translation_source: "auto".into(),
            google_playlists: vec![],
            spotify_playlists: vec![],
            _tray: tray,
            close_to_tray: false,
            media_keys: None,
            native_preferences_loaded: false,
            lastfm_key: String::new(),
            lastfm_secret: String::new(),
            lastfm_status: json!({}),
            extension_status: json!({}),
            account_status: json!({}),
            login_cancel: None,
            login_thread: None,
            retry_login: false,
            login_generation: 0,
            discord_id: "1520100510877749358".into(),
            devices: vec![],
            selected_device: String::new(),
            sleep_minutes: 30,
            cache_info: json!({}),
            cache_limit_mb: 1024,
            proxy_input: String::new(),
            network_info: json!({}),
            cookie_browser: String::new(),
            appearance: Default::default(),
            appearance_signature: String::new(),
            themes: vec![],
            theme_name: String::new(),
            release_repo: String::new(),
            update_info: json!({}),
            update_checked: false,
            library_filter: String::new(),
            library_sort: 0,
            rename_playlist: String::new(),
            navigation: vec![],
            lyric_generation: 0,
            translation_generation: 0,
        };
        if let Some(error) = media_error {
            app.error = Some(format!("Windows media session unavailable: {error}"));
        }
        app.send("home", "music.home", json!({}));
        app
    }
    fn send(&mut self, tag: &str, command: &str, params: Value) {
        let tag = if tag == "oauth-authorize" || tag == "auth" {
            format!(
                "{}:{}",
                if tag == "auth" {
                    "oauth-finish"
                } else {
                    "oauth-authorize"
                },
                self.login_generation
            )
        } else if tag == "lyrics" || tag == "translation" {
            let generation = if tag == "lyrics" {
                self.lyric_generation += 1;
                self.lyric_generation
            } else {
                self.translation_generation += 1;
                self.translation_generation
            };
            format!(
                "{tag}@{generation}@{}",
                self.player["track_id"].as_str().unwrap_or("")
            )
        } else if ["search", "home", "browse"].contains(&tag) {
            let old: Vec<_> = self
                .job_tags
                .iter()
                .filter(|(_, tag)| {
                    ["search:", "home:", "browse:"]
                        .iter()
                        .any(|prefix| tag.starts_with(prefix))
                })
                .map(|(id, _)| id.clone())
                .collect();
            for id in old {
                self.send("cancel", "job.cancel", json!({"id":id}));
            }
            self.results_generation += 1;
            format!("{tag}:{}", self.results_generation)
        } else {
            tag.into()
        };
        if self
            .tx
            .send(Request {
                tag,
                command: command.into(),
                params,
            })
            .is_err()
        {
            self.error = Some("Audio engine is no longer running. Restart Riff.".into());
        }
    }
    fn job(&mut self, tag: &str, operation: &str, params: Value) {
        self.busy = true;
        self.send(
            tag,
            "job.submit",
            json!({"operation":operation,"params":params}),
        );
    }
    fn current_tag<'a>(&self, tag: &'a str) -> Option<&'a str> {
        for (prefix, name) in [
            ("oauth-authorize:", "oauth-authorize"),
            ("oauth-finish:", "auth"),
        ] {
            if let Some(number) = tag.strip_prefix(prefix) {
                return (number.parse::<u64>().ok() == Some(self.login_generation)).then_some(name);
            }
        }
        for (prefix, generation, name) in [
            ("lyrics@", self.lyric_generation, "lyrics"),
            ("translation@", self.translation_generation, "translation"),
        ] {
            if let Some(rest) = tag.strip_prefix(prefix) {
                let (number, id) = rest.split_once('@')?;
                return (number.parse::<u64>().ok() == Some(generation)
                    && Some(id) == self.player["track_id"].as_str())
                .then_some(name);
            }
        }
        if let Some((name, number)) = tag.split_once(':')
            && ["search", "home", "browse"].contains(&name)
        {
            return (number.parse::<u64>().ok() == Some(self.results_generation)).then_some(name);
        }
        Some(tag)
    }
    fn complete(&mut self, tag: &str, data: Value) {
        if data["superseded"] == true {
            return;
        }
        if let Some(jobs) = data["jobs"].as_array() {
            for job in jobs {
                self.complete("download", job.clone());
            }
            self.status = format!(
                "Queued {} downloads; {} already offline",
                jobs.len(),
                data["skipped"]
            );
            return;
        }
        if let Some(id) = data["job_id"].as_str() {
            if self.current_tag(tag).is_none() {
                self.send("cancel", "job.cancel", json!({"id":id}));
                return;
            }
            self.job_tags.insert(id.into(), tag.into());
            self.jobs.insert(
                id.into(),
                json!({"id":id,"operation":tag,"state":data["state"]}),
            );
            if tag != "warm" {
                self.busy = true;
            }
            return;
        }
        let Some(tag) = self.current_tag(tag) else {
            return;
        };
        match tag {
            "update" | "update-silent" => {
                self.update_info = data;
                if tag == "update" {
                    self.status = "Update check complete".into();
                }
            }
            "support" => {
                self.status = "Diagnostic report saved without private paths or track titles".into()
            }
            "install" => {
                self.status = format!(
                    "Installed for this Windows user: {}",
                    data["path"].as_str().unwrap_or("")
                );
            }
            "devices" => {
                self.devices = data["devices"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect();
            }
            "cache" => {
                self.cache_info = data;
                self.status = "Offline cache updated".into();
            }
            "network" => {
                self.network_info = data;
                self.proxy_input.clear();
                self.status = "Network settings updated".into();
            }
            "network-test" => {
                self.status = format!("Connection OK ({} ms)", data["elapsed_ms"]);
            }
            "themes" => {
                self.themes = data.as_array().cloned().unwrap_or_default();
            }
            "appearance" => {
                self.status = "Appearance saved".into();
            }
            "sleep" => {
                self.status = if data["seconds"] == 0 {
                    "Sleep timer cancelled".into()
                } else {
                    "Sleep timer set".into()
                };
            }
            "lastfm-status" => self.lastfm_status = data,
            "extensions-status" => self.extension_status = data,
            "account-status" => self.account_status = data,
            "logout" => {
                self.spotify_playlists.clear();
                self.google_playlists.clear();
                self.account_status = json!({"signed_in":false});
                self.status = "Account disconnected; saved library retained".into();
            }
            "cookies-cleared" => {
                self.status = "Cookie file detached; original file retained".into()
            }
            "theme-import" => {
                self.status = data["limitations"]
                    .as_str()
                    .unwrap_or("Theme imported")
                    .into();
                self.send("themes", "themes.list", json!({}));
                self.send("extensions-status", "extensions.status", json!({}));
            }
            "spotify-playlists" => {
                self.spotify_playlists = data["items"].as_array().cloned().unwrap_or_default();
            }
            "google-playlists" => {
                self.google_playlists = data["items"].as_array().cloned().unwrap_or_default()
            }
            "translation" => self.translated = data["text"].as_str().unwrap_or("").into(),
            "plugins" => self.plugins = serde_json::from_value(data).unwrap_or_default(),
            "plugin" => {
                self.status = "Plugin action completed".into();
                self.send("plugins", "plugins.list", json!({}));
            }
            "play" | "toggle" | "next" | "previous" | "playlist-play" | "preview" => {
                self.status = playback_status(&data).into()
            }
            "stop" => self.status = "Playback stopped".into(),
            "authorize" | "oauth-authorize" => {
                if let Some(url) = data["authorization_url"].as_str() {
                    match open::that(url) {
                        Ok(_) => {
                            self.status =
                                "Complete sign-in in your browser, then return to Riff.".into()
                        }
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
            }
            "auth" => {
                self.status = if data["persistent"] == true {
                    "Account connected. Stored securely for this Windows user."
                } else {
                    "Account connected for this app session."
                }
                .into()
            }
            "import" => {
                self.active_playlist = data["playlist"]["id"].as_str().map(str::to_string);
                self.status = if data["matching"] == "deferred_until_playback" {
                    "Playlist imported. Audio matching happens when you play a track."
                } else {
                    "Playlist imported. Review matches before playing."
                }
                .into();
                self.page = Page::Playlists;
            }
            "search" | "home" | "browse" | "recommendations" => {
                if tag == "browse" {
                    self.current_browse =
                        Some(json!({"id":data["browse_id"],"title":self.browse_title}));
                }
                self.tracks = serde_json::from_value(
                    data.get("tracks").cloned().unwrap_or_else(|| data.clone()),
                )
                .unwrap_or_default();
                self.items = data["items"].as_array().cloned().unwrap_or_default();
                self.status = if tag == "recommendations" {
                    format!("{} tracks selected by YouTube Music", self.tracks.len())
                } else {
                    format!("{} tracks", self.tracks.len())
                };
                self.busy = false;
                if let Some(track) = self.tracks.first().cloned() {
                    self.send("warm", "stream.prepare", json!({"track":track}));
                }
            }
            "lyrics" => {
                self.lyric_lines =
                    serde_json::from_value(data["lines"].clone()).unwrap_or_default();
                self.lyric_text = data["text"].as_str().unwrap_or("").into();
                if data["instrumental"] == true {
                    self.lyric_text = "Instrumental — no sung lyrics".into();
                }
            }
            "scan" => {
                let good = data["tracks"].as_array().map_or(0, Vec::len);
                let bad = data["errors"].as_array().map_or(0, Vec::len);
                self.status = format!("Imported {good} tracks - {bad} errors");
                self.page = Page::Library;
            }
            "download" => self.status = "Download finished. Available offline.".into(),
            "encode" => {
                self.status = format!("Saved {}", data["path"].as_str().unwrap_or("audio file"))
            }
            "tools" => self.status = serde_json::to_string_pretty(&data).unwrap_or_default(),
            "radio" => {
                self.status =
                    "YouTube Music radio is ready. Queue follows the provider's order.".into();
                self.page = Page::Queue;
            }
            _ => {}
        }
    }
    fn receive(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            match message {
                Message::Library(data) => {
                    if !self.native_preferences_loaded
                        || data.settings.get("release_repo")
                            != self.library.settings.get("release_repo")
                    {
                        self.release_repo = data
                            .settings
                            .get("release_repo")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .into();
                    }
                    if data.settings.get("appearance") != self.library.settings.get("appearance") {
                        self.appearance = data
                            .settings
                            .get("appearance")
                            .cloned()
                            .and_then(|v| riff_core::appearance::Appearance::from_tokens(v).ok())
                            .unwrap_or_default();
                    }
                    if !self.native_preferences_loaded
                        || data.settings.get("cache_limit_mb")
                            != self.library.settings.get("cache_limit_mb")
                    {
                        self.cache_limit_mb = data
                            .settings
                            .get("cache_limit_mb")
                            .and_then(Value::as_u64)
                            .unwrap_or(1024);
                    }
                    if !self.native_preferences_loaded
                        || data.settings.get("audio_device")
                            != self.library.settings.get("audio_device")
                    {
                        self.selected_device = data
                            .settings
                            .get("audio_device")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .into();
                    }
                    if !self.native_preferences_loaded
                        || data.settings.get("cookie_browser")
                            != self.library.settings.get("cookie_browser")
                    {
                        self.cookie_browser = data
                            .settings
                            .get("cookie_browser")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .into();
                    }
                    if !self.native_preferences_loaded {
                        self.native_preferences_loaded = true;
                        self.close_to_tray = data
                            .settings
                            .get("close_to_tray")
                            .is_some_and(|v| v == true);
                        if data.settings.get("media_keys").is_some_and(|v| v == true) {
                            self.enable_media_keys();
                        }
                    }
                    if self.effects_pending.is_none() {
                        self.effects = data.effects.clone();
                    }
                    self.library = *data;
                    if !self.update_checked {
                        self.update_checked = true;
                        if self.library.settings.get("auto_update_check") != Some(&json!(false)) {
                            self.send("update-silent", "updates.check", json!({}));
                        }
                    }
                }
                Message::Player(state) => {
                    if let Some(tasks) = state["tasks"].as_array() {
                        for task in tasks {
                            if let Some(id) = task["id"].as_str() {
                                let entry =
                                    self.jobs.entry(id.into()).or_insert_with(|| task.clone());
                                entry["state"] = task["state"].clone();
                                entry["elapsed_ms"] = task["elapsed_ms"].clone();
                            }
                        }
                    }
                    if self.status == "Playing"
                        || self.status == "Buffering audio…"
                        || self.status == "Connecting audio…"
                        || self.status == "Reconnecting audio…"
                    {
                        self.status = playback_status(&state).into();
                    }
                    self.player = state;
                }
                Message::Fatal(error) => {
                    self.error = Some(error);
                    self.busy = false;
                }
                Message::Response { tag, result } => match result {
                    Ok(data) => self.complete(&tag, data),
                    Err(error)
                        if tag == "warm"
                            || tag == "update-silent"
                            || tag == "cancel"
                            || self.current_tag(&tag).is_none() =>
                    {
                        let _ = error;
                    }
                    Err(error) => {
                        self.error = Some(error);
                        self.busy = false;
                    }
                },
                Message::Event(event) => {
                    if event["event"] == "job_finished" {
                        let job = event["job"].clone();
                        let id = job["id"].as_str().unwrap_or("").to_string();
                        if let Some(tag) = self.job_tags.remove(&id) {
                            if job["state"] == "completed" {
                                self.complete(&tag, job["data"].clone());
                            } else if tag == "warm" || tag == "update-silent" {
                            } else if self.current_tag(&tag) == Some("lyrics") {
                                self.lyric_text = "Lyrics are not available for this track.".into();
                            } else if job["state"] != "cancelled"
                                && self.current_tag(&tag).is_some()
                            {
                                self.error =
                                    Some(job["error"].as_str().unwrap_or("Task failed").into());
                            }
                        }
                        self.jobs.insert(id, job);
                        self.busy = self.jobs.values().any(|j| {
                            (j["state"] == "running" || j["state"] == "queued")
                                && j["operation"] != "warm"
                                && j["operation"] != "stream.prepare"
                        });
                    } else if event["event"] == "track_changed" {
                        self.player = event["player"].clone();
                        self.status = playback_status(&self.player).into();
                    } else if event["event"] == "playback_recovering" {
                        self.player = event["player"].clone();
                        self.status = "Reconnecting audio…".into();
                    } else if event["event"]
                        .as_str()
                        .is_some_and(|name| name.ends_with("_error"))
                    {
                        self.error =
                            Some(event["error"].as_str().unwrap_or("Playback failed").into());
                        if event["event"] == "playback_error" {
                            self.status = "Playback stopped".into();
                        }
                    }
                }
            }
        }
        let current = self.player["track_id"].as_str().map(str::to_string);
        if current != self.lyric_for {
            let old: Vec<_> = self
                .job_tags
                .iter()
                .filter(|(_, tag)| tag.starts_with("lyrics@"))
                .map(|(id, _)| id.clone())
                .collect();
            for id in old {
                self.send("cancel", "job.cancel", json!({"id":id}));
            }
            self.lyric_for = current.clone();
            self.lyric_lines.clear();
            self.lyric_text.clear();
            self.translated.clear();
            if let Some(track) = current
                .as_ref()
                .and_then(|id| self.library.tracks.get(id))
                .cloned()
            {
                self.job(
                    "lyrics",
                    "lyrics.get",
                    json!({"title":track.title,"artist":track.artist,"duration":track.duration,"source":self.lyric_source}),
                );
            }
        }
    }
    fn play(&mut self, track: &Track) {
        self.send("add", "library.add", json!({"track":track}));
        self.send("play", "player.request", json!({"id":track.id}));
        self.status = format!("Loading {}…", track.title);
    }
    fn radio(&mut self, track: &Track) {
        self.send("add", "library.add", json!({"track":track}));
        self.send("radio", "radio.start", json!({"id":track.id}));
        self.status = "Requesting a radio queue from YouTube Music…".into();
    }
    fn search(&mut self) {
        self.search_limit = if [1, 2].contains(&self.platform) {
            30
        } else {
            10
        };
        self.search_current();
    }
    fn search_current(&mut self) {
        if self.query.trim().is_empty() {
            return;
        }
        self.page = Page::Search;
        self.status = "Searching…".into();
        self.busy = true;
        self.error = None;
        self.tracks.clear();
        self.items.clear();
        if self.platform == 3 {
            self.send(
                "search",
                "spotify.search",
                json!({"query":self.query,"limit":self.search_limit}),
            );
        } else if self.platform == 0 {
            self.send(
                "search",
                "music.search",
                json!({"query":self.query,"filter":self.search_filter}),
            );
        } else {
            self.job("search","search",json!({"query":self.query,"platform":if self.platform==1{"youtube"}else{"soundcloud"},"limit":self.search_limit}));
        }
    }
    fn import(&mut self, folder: bool) {
        let paths = if folder {
            rfd::FileDialog::new().pick_folder().map(|p| vec![p])
        } else {
            rfd::FileDialog::new()
                .add_filter(
                    "Audio",
                    &["mp3", "flac", "wav", "ogg", "opus", "m4a", "aac"],
                )
                .pick_files()
        };
        if let Some(paths) = paths {
            self.job("scan", "library.scan", json!({"paths":paths}));
        }
    }
    fn enable_media_keys(&mut self) {
        let sender = self.tx.clone();
        match riff_core::media_keys::MediaKeys::start(move |command| {
            let _ = sender.send(Request {
                tag: "media-key".into(),
                command: command.into(),
                params: json!({}),
            });
        }) {
            Ok(keys) => self.media_keys = Some(keys),
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    fn login(&mut self) {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        if self.oauth_client.trim().is_empty() {
            self.error = Some("Enter your app's OAuth client ID first.".into());
            return;
        }
        if self
            .login_thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
        {
            if let Some(cancel) = &self.login_cancel {
                cancel.store(true, Ordering::Relaxed);
            }
            self.retry_login = true;
            self.status = "Restarting the sign-in callback…".into();
            return;
        }
        if let Some(thread) = self.login_thread.take() {
            let _ = thread.join();
        }
        self.retry_login = false;
        self.login_generation = self.login_generation.wrapping_add(1);
        let listener = match TcpListener::bind(("127.0.0.1", 8751)) {
            Ok(l) => l,
            Err(e) => {
                self.error = Some(format!("Cannot open sign-in callback on port 8751: {e}"));
                return;
            }
        };
        if let Err(e) = listener.set_nonblocking(true) {
            self.error = Some(e.to_string());
            return;
        }
        let provider = if self.oauth_provider == 0 {
            "google"
        } else {
            "spotify"
        }
        .to_string();
        let callback_provider = provider.clone();
        let tx = self.tx.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        self.login_cancel = Some(cancel.clone());
        let shutdown = self.shutdown.clone();
        let generation = self.login_generation;
        self.login_thread = Some(std::thread::spawn(move || {
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(600)
                && !cancel.load(Ordering::Relaxed)
                && !shutdown.load(Ordering::Relaxed)
            {
                if let Ok((mut stream, _)) = listener.accept() {
                    if stream.set_nonblocking(false).is_err() {
                        continue;
                    }
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let mut bytes = [0; 8192];
                    if let Ok(n) = stream.read(&mut bytes) {
                        let request = String::from_utf8_lossy(&bytes[..n]);
                        let target = request
                            .lines()
                            .next()
                            .and_then(|s| s.split_whitespace().nth(1))
                            .unwrap_or("");
                        if let Ok(url) = url::Url::parse(&format!("http://127.0.0.1:8751{target}"))
                            && url.path() == "/callback"
                        {
                            let pairs = url.query_pairs().collect::<HashMap<_, _>>();
                            let code = pairs.get("code").map(|v| v.to_string());
                            let state = pairs.get("state").map(|v| v.to_string());
                            let body = "Sign-in callback received. You can return to Riff.";
                            let _ = write!(
                                stream,
                                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len()
                            );
                            if let (Some(code), Some(state)) = (code, state)
                                && !cancel.load(Ordering::Relaxed)
                                && !shutdown.load(Ordering::Relaxed)
                            {
                                let _=tx.send(Request{tag:format!("oauth-finish:{generation}"),command:"oauth.finish".into(),params:json!({"provider":callback_provider,"code":code,"state":state})});
                            }
                            return;
                        }
                    }
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                }
                std::thread::sleep(Duration::from_millis(60));
            }
        }));
        self.send("oauth-authorize","oauth.begin",json!({"provider":provider,"config":{"client_id":self.oauth_client,"client_secret":if self.oauth_secret.is_empty(){None}else{Some(self.oauth_secret.clone())},"redirect_uri":"http://127.0.0.1:8751/callback"}}));
    }
    fn rows(&mut self, ui: &mut egui::Ui, mut tracks: Vec<Track>) {
        if matches!(self.page, Page::Library | Page::Favorites | Page::Offline) {
            ui.horizontal(|ui| {
                ui.label("Filter library");
                ui.text_edit_singleline(&mut self.library_filter);
                egui::ComboBox::from_label("Sort library")
                    .selected_text(["Title", "Artist", "Duration"][self.library_sort])
                    .show_ui(ui, |ui| {
                        for (i, label) in ["Title", "Artist", "Duration"].iter().enumerate() {
                            ui.selectable_value(&mut self.library_sort, i, *label);
                        }
                    });
            });
            let filter = self.library_filter.to_lowercase();
            tracks.retain(|t| {
                format!("{} {} {}", t.title, t.artist, t.album)
                    .to_lowercase()
                    .contains(&filter)
            });
            tracks.sort_by(|a, b| match self.library_sort {
                1 => a.artist.to_lowercase().cmp(&b.artist.to_lowercase()),
                2 => a.duration.total_cmp(&b.duration),
                _ => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
            });
        }
        if tracks.is_empty() {
            if self.page == Page::Search && self.busy {
                ui.label("Waiting for search results…");
                return;
            }
            ui.add_space(38.);
            ui.label(RichText::new("Nothing here yet").size(22.));
            ui.label("Search for a song, import your files, or start a radio from a track.");
            return;
        }
        let mut hovered = None;
        for (index, track) in tracks.iter().enumerate() {
            let current = self.player["track_id"] == track.id;
            let row = egui::Frame::new()
                .fill(if current {
                    ui.visuals().selection.bg_fill.gamma_multiply(0.35)
                } else if index % 2 == 0 {
                    ui.visuals().faint_bg_color
                } else {
                    Color32::TRANSPARENT
                })
                .inner_margin(8.)
                .corner_radius(8)
                .show(ui, |ui| {
                    ui.push_id(&track.id, |ui| {
                        ui.horizontal(|ui| {
                            let mut selected = self.selected.contains(&track.id);
                            if ui
                                .checkbox(&mut selected, "")
                                .on_hover_text("Select track")
                                .changed()
                            {
                                if selected {
                                    self.selected.push(track.id.clone());
                                } else {
                                    self.selected.retain(|x| x != &track.id);
                                }
                            }
                            if let Some(cover) = &track.cover {
                                ui.add(
                                    egui::Image::new(cover)
                                        .fit_to_exact_size(egui::vec2(44., 44.))
                                        .corner_radius(6),
                                );
                            } else {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(44., 44.),
                                    egui::Sense::hover(),
                                );
                                ui.painter()
                                    .rect_filled(rect, 6., Color32::from_rgb(70, 56, 101));
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "R",
                                    egui::FontId::proportional(22.),
                                    Color32::WHITE,
                                );
                            }
                            ui.allocate_ui_with_layout(
                                egui::vec2((ui.available_width() - 270.).max(100.), 48.),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.label(RichText::new(&track.title).strong().color(
                                        ui.visuals().text_color(),
                                    ));
                                    ui.label(
                                        RichText::new(format!(
                                            "{}  {}",
                                            track.artist,
                                            if track.local_path.is_some() {
                                                "- Offline"
                                            } else {
                                                ""
                                            }
                                        ))
                                        .small()
                                        .color(ui.visuals().weak_text_color()),
                                    );
                                },
                            );
                            ui.label(time(track.duration));
                            let play = ui
                                .button("Play")
                                .on_hover_text(format!("Play {}", track.title));
                            play.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    true,
                                    format!(
                                        "Play {} by {} [{}]",
                                        track.title, track.artist, track.id
                                    ),
                                )
                            });
                            if play.clicked() {
                                self.play(track);
                            }
                            if matches!(self.page,Page::Queue|Page::Playlists) {
                                if ui.small_button(format!("Up {}",index+1)).clicked() && index>0 {self.move_track(index,index-1);}
                                if ui.small_button(format!("Down {}",index+1)).clicked() && index+1<tracks.len() {self.move_track(index,index+1);}
                            }
                            let favorite = self.library.favorites.contains(&track.id);
                            if ui
                                .selectable_label(favorite, if favorite { "Liked" } else { "Like" })
                                .clicked()
                            {
                                self.send("add", "library.add", json!({"track":track}));
                                self.send(
                                    "favorite",
                                    "favorites.set",
                                    json!({"id":track.id,"enabled":!favorite}),
                                );
                            }
                            let menu = ui.menu_button("More", |ui| {
                                if ui.button("Start YouTube Music radio").clicked()
                                {
                                    self.radio(track);
                                    ui.close();
                                }
                                if !track.artist.is_empty() && ui.button("Find artist").clicked(){self.query=track.artist.clone();self.platform=0;self.search_filter="artists".into();self.search();ui.close();}
                                if !track.album.is_empty() && ui.button("Find album").clicked(){self.query=format!("{} {}",track.artist,track.album);self.platform=0;self.search_filter="albums".into();self.search();ui.close();}
                                if track.platform == "spotify"
                                    && ui.button("Open in Spotify").clicked()
                                {
                                    let _ = open::that(&track.source);
                                    ui.close();
                                }
                                if ui.button("Add to queue").clicked() {
                                    self.send("add", "library.add", json!({"track":track}));
                                    self.send(
                                        "append",
                                        "queue.append",
                                        json!({"tracks":[track.id]}),
                                    );
                                    ui.close();
                                }
                                if track.platform != "local"
                                    && ui.button("Download for offline").clicked()
                                {
                                    self.send("add", "library.add", json!({"track":track}));
                                    self.job(
                                        "download",
                                        "download",
                                        json!({"id":track.id,"format":self.library.settings.get("download_format").and_then(Value::as_str).unwrap_or("opus")}),
                                    );
                                    ui.close();
                                }
                                if track.local_path.is_some()&&track.platform!="local"&&ui.button("Delete downloaded copy").clicked(){self.send("offline","offline.delete",json!({"id":track.id}));ui.close();}
                                if self.page==Page::Playlists && ui.button("Remove from this playlist").clicked(){if let Some(mut list)=self.active_playlist.as_ref().and_then(|id|self.library.playlists.get(id)).cloned(){list.tracks.remove(index);self.send("playlist","playlist.save",json!(list));}ui.close();}
                                if ui.button("Remove from library").clicked() {
                                    self.send("remove", "library.remove", json!({"id":track.id}));
                                    ui.close();
                                }
                            });
                            menu.response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    true,
                                    format!("Options for {} [{}]", track.title, track.id),
                                )
                            });
                        })
                    });
                });
            if ui
                .input(|i| i.pointer.hover_pos())
                .is_some_and(|pos| row.response.rect.contains(pos))
                && track.platform != "local"
                && track.local_path.is_none()
            {
                hovered = Some(track.clone());
            }
        }
        if let Some(track) = hovered {
            if self
                .hover_candidate
                .as_ref()
                .is_none_or(|(old, _)| old.id != track.id)
            {
                self.hover_candidate = Some((track.clone(), Instant::now()));
            }
            let elapsed = self.hover_candidate.as_ref().unwrap().1.elapsed();
            if elapsed >= Duration::from_millis(250)
                && self.hover_warmed.as_deref() != Some(&track.id)
                && !self.job_tags.values().any(|tag| tag == "warm")
                && !self.player["pending_track_id"].is_string()
            {
                self.hover_warmed = Some(track.id.clone());
                self.send("warm", "stream.prepare", json!({"track":track}));
            } else if elapsed < Duration::from_millis(250) {
                ui.ctx()
                    .request_repaint_after(Duration::from_millis(250) - elapsed);
            }
        } else {
            self.hover_candidate = None;
        }
    }
    fn move_track(&mut self, from: usize, to: usize) {
        if self.page == Page::Queue {
            let mut order: Vec<_> = (0..self.library.queue.tracks.len()).collect();
            order.swap(from, to);
            self.send("queue", "queue.reorder", json!({"order":order}));
        } else if let Some(mut list) = self
            .active_playlist
            .as_ref()
            .and_then(|id| self.library.playlists.get(id))
            .cloned()
        {
            list.tracks.swap(from, to);
            self.send("playlist", "playlist.save", json!(list));
        }
    }
    fn catalog(&mut self, ui: &mut egui::Ui) {
        let items = self.items.clone();
        if items.is_empty() {
            return;
        }
        let columns = (ui.available_width() / 190.).floor().max(1.) as usize;
        egui::Grid::new("catalog-grid")
            .num_columns(columns)
            .spacing([12., 12.])
            .show(ui, |ui| {
                for (index, item) in items.iter().take(12).enumerate() {
                    ui.allocate_ui_with_layout(
                        egui::vec2(176., 260.),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            egui::Frame::group(ui.style())
                                .inner_margin(12.)
                                .corner_radius(8)
                                .show(ui, |ui| {
                                    ui.set_width(150.);
                                    ui.set_min_height(235.);
                                    if let Some(cover) = item["cover"].as_str() {
                                        ui.add(
                                            egui::Image::new(cover)
                                                .fit_to_exact_size(egui::vec2(150., 150.))
                                                .corner_radius(6),
                                        );
                                    }
                                    ui.label(
                                        RichText::new(
                                            item["title"].as_str().unwrap_or("Collection"),
                                        )
                                        .strong(),
                                    );
                                    ui.label(item["kind"].as_str().unwrap_or(""));
                                    if ui.button("Open collection").clicked() {
                                        self.navigation.push((
                                            self.page,
                                            self.tracks.clone(),
                                            self.items.clone(),
                                            self.browse_title.clone(),
                                            self.current_browse.clone(),
                                        ));
                                        self.page = Page::Browse;
                                        self.browse_title =
                                            item["title"].as_str().unwrap_or("").into();
                                        self.busy = true;
                                        self.send(
                                            "browse",
                                            "music.browse",
                                            json!({"id":item["id"],"params":item["params"]}),
                                        );
                                    }
                                });
                        },
                    );
                    if (index + 1) % columns == 0 {
                        ui.end_row();
                    }
                }
            });
        ui.add_space(15.);
    }
    fn player_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("player")
            .min_height(105.)
            .show(ctx, |ui| {
                ui.add_space(8.);
                self.visualizer(ui);
                let track = self.player["track_id"]
                    .as_str()
                    .and_then(|id| self.library.tracks.get(id))
                    .cloned();
                if let Some(title) = self.player["playback_source"]["title"].as_str() {
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "Audio: {} - {} (duration checked)",
                            title,
                            time(
                                self.player["playback_source"]["duration"]
                                    .as_f64()
                                    .unwrap_or(0.)
                            )
                        ));
                        if let Some(url) = self.player["playback_source"]["url"].as_str()
                            && ui.small_button("Open recording").clicked()
                        {
                            let _ = open::that(url);
                        }
                    });
                }
                ui.horizontal_wrapped(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(200., 52.),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.label(
                                RichText::new(
                                    track
                                        .as_ref()
                                        .map(|t| t.title.as_str())
                                        .unwrap_or("Choose something to play"),
                                )
                                .strong(),
                            );
                            ui.label(
                                track
                                    .as_ref()
                                    .map(|t| t.artist.as_str())
                                    .unwrap_or("Your music, in one place."),
                            );
                        },
                    );
                    if ui.button("Previous").clicked() {
                        self.send("previous", "player.previous", json!({}));
                    }
                    let paused = self.player["paused"].as_bool().unwrap_or(true);
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(if paused { "Play" } else { "Pause" })
                                    .color(Color32::WHITE),
                            )
                            .fill(Color32::from_rgb(105, 75, 185)),
                        )
                        .clicked()
                    {
                        self.send(
                            "toggle",
                            if paused {
                                "player.resume"
                            } else {
                                "player.pause"
                            },
                            json!({}),
                        );
                    }
                    if ui.button("Next").clicked() {
                        self.send("next", "player.next", json!({}));
                    }
                    if ui.button("Stop").clicked() {
                        self.send("stop", "player.stop", json!({}));
                    }
                    if let Some(track) = track.as_ref()
                        && track.id.starts_with("youtube:")
                        && ui.button("Radio").clicked()
                    {
                        self.radio(track);
                    }
                    let mut muted = self.effects.muted;
                    if ui.checkbox(&mut muted, "Mute").changed() {
                        self.effects.muted = muted;
                        self.send_effects();
                    }
                    let mut shuffle = self.library.queue.shuffle;
                    if ui.checkbox(&mut shuffle, "Shuffle").changed() {
                        self.send("mode", "queue.mode", json!({"shuffle":shuffle}));
                    }
                    let mut repeat = self.library.queue.repeat;
                    egui::ComboBox::from_id_salt("repeat")
                        .selected_text(format!("Repeat: {repeat:?}"))
                        .show_ui(ui, |ui| {
                            for v in [
                                riff_core::model::Repeat::Off,
                                riff_core::model::Repeat::All,
                                riff_core::model::Repeat::One,
                            ] {
                                ui.selectable_value(&mut repeat, v, format!("{v:?}"));
                            }
                        });
                    if repeat != self.library.queue.repeat {
                        self.send("mode", "queue.mode", json!({"repeat":repeat}));
                    }
                    let mut volume = self.effects.volume;
                    if ui
                        .add(
                            egui::Slider::new(&mut volume, 0.0..=1.)
                                .text("Volume")
                                .show_value(false),
                        )
                        .changed()
                    {
                        self.effects.volume = volume;
                        self.send_effects();
                    }
                });
                ui.horizontal(|ui| {
                    let position = self.player["position"].as_f64().unwrap_or(0.);
                    let duration = self.player["duration"].as_f64().unwrap_or(0.);
                    ui.label(time(position));
                    let mut seek = position;
                    ui.spacing_mut().slider_width = (ui.available_width() - 100.).max(150.);
                    let response = ui.add_sized(
                        [ui.available_width() - 100., 22.],
                        egui::Slider::new(&mut seek, 0.0..=duration.max(1.)).show_value(false),
                    );
                    if response.dragged() {
                        self.seek_preview = seek;
                    }
                    if response.drag_stopped() || response.changed() && !response.dragged() {
                        self.send("seek", "player.seek", json!({"seconds":seek}));
                    }
                    ui.label(time(duration));
                    if self.player["buffering"] == true {
                        ui.spinner();
                    }
                });
            });
    }
    fn send_effects(&mut self) {
        self.send("effects", "player.effects", json!(self.effects));
    }
    fn open_settings(&mut self) {
        self.page = Page::Settings;
        for (tag, command) in [
            ("plugins", "plugins.list"),
            ("lastfm-status", "lastfm.status"),
            ("devices", "player.devices"),
            ("cache", "cache.status"),
            ("network", "network.status"),
            ("themes", "themes.list"),
            ("extensions-status", "extensions.status"),
        ] {
            self.send(tag, command, json!({}));
        }
    }
    fn playback_preferences(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("Playback & offline storage",|ui|{
            ui.horizontal(|ui|{
                egui::ComboBox::from_label("Output device").selected_text(if self.selected_device.is_empty(){"System default"}else{&self.selected_device}).show_ui(ui,|ui|{
                    ui.selectable_value(&mut self.selected_device,String::new(),"System default");
                    for name in self.devices.clone(){ui.selectable_value(&mut self.selected_device,name.clone(),name);}
                });
                if ui.button("Use output device").clicked(){self.send("device","player.device",json!({"name":if self.selected_device.is_empty(){None}else{Some(&self.selected_device)}}));}
                if ui.button("Refresh devices").clicked(){self.send("devices","player.devices",json!({}));}
            });
            ui.horizontal(|ui|{
                ui.label("Sleep timer (minutes)");ui.add(egui::DragValue::new(&mut self.sleep_minutes).range(1..=720));
                if ui.button("Start sleep timer").clicked(){self.send("sleep","player.sleep",json!({"seconds":self.sleep_minutes*60}));}
                if ui.button("Cancel sleep timer").clicked(){self.send("sleep","player.sleep",json!({"seconds":0}));}
                if let Some(seconds)=self.player["sleep_remaining"].as_u64(){ui.label(format!("Remaining {}",time(seconds as f64)));}
            });
            ui.horizontal(|ui|{
                let changed=ui.add(egui::Slider::new(&mut self.effects.crossfade_seconds,0.0..=12.).text("Crossfade (seconds)")).changed();
                let normalize=ui.checkbox(&mut self.effects.normalize,"Normalize volume").changed();
                if changed||normalize{self.effects_pending=Some(Instant::now());}
            });
            ui.horizontal(|ui|{
                ui.label("Offline download format");let mut format=self.library.settings.get("download_format").and_then(Value::as_str).unwrap_or("opus").to_owned();let before=format.clone();
                egui::ComboBox::from_label("Audio format").selected_text(&format).show_ui(ui,|ui|{for value in ["opus","mp3","flac","m4a","wav"]{ui.selectable_value(&mut format,value.into(),value);}});
                if format!=before{self.send("preferences","settings.set",json!({"download_format":format}));}
            });
            ui.horizontal(|ui|{
                ui.label("Cache limit (MB)");ui.add(egui::DragValue::new(&mut self.cache_limit_mb).range(64..=65536));
                if ui.button("Apply cache limit").clicked(){self.send("cache","cache.limit",json!({"mb":self.cache_limit_mb}));}
                if ui.button("Refresh cache").clicked(){self.send("cache","cache.status",json!({}));}
                if ui.button("Clear cache").clicked(){self.send("cache","cache.clear",json!({}));}
            });
            ui.label(format!("Cached: {:.1} MB; active audio is protected during cleanup",self.cache_info["bytes"].as_u64().unwrap_or(0)as f64/1048576.));
        });
        ui.collapsing("Appearance & visualizer",|ui|{
            egui::ComboBox::from_label("Theme mode").selected_text(&self.appearance.mode).show_ui(ui,|ui|{ui.selectable_value(&mut self.appearance.mode,"dark".into(),"Dark");ui.selectable_value(&mut self.appearance.mode,"light".into(),"Light");});
            ui.horizontal(|ui|{ui.label("Accent color");ui.color_edit_button_srgb(&mut self.appearance.accent);});
            ui.add(egui::Slider::new(&mut self.appearance.font_size,12.0..=24.).text("Text size"));
            ui.checkbox(&mut self.appearance.reduced_motion,"Reduce motion");
            egui::ComboBox::from_label("Music visualizer").selected_text(&self.appearance.visualizer).show_ui(ui,|ui|{for style in ["off","bars","waveform"]{ui.selectable_value(&mut self.appearance.visualizer,style.into(),style);}});
            if ui.button("Apply appearance").clicked(){self.send("appearance","appearance.set",json!(self.appearance));}
            ui.horizontal(|ui|{
                ui.label("Theme name");ui.text_edit_singleline(&mut self.theme_name);
                if ui.button("Save theme").clicked()&&!self.theme_name.trim().is_empty(){let id=format!("user-{}",uuid::Uuid::new_v4());self.send("theme","themes.save",json!({"id":id,"name":self.theme_name,"tokens":self.appearance}));self.send("themes","themes.list",json!({}));}
                if ui.button("Import theme / original settings").clicked()&&let Some(path)=rfd::FileDialog::new().add_filter("Riff JSON theme or settings",&["json"]).pick_file(){self.send("theme-import","themes.import",json!({"path":path}));}
                if ui.button("Export appearance").clicked()&&let Some(path)=rfd::FileDialog::new().set_file_name("riff-theme.json").save_file(){let value=json!({"id":"exported-theme","name":"My Riff theme","tokens":self.appearance});if let Err(e)=riff_core::store::write_json(&path,&value){self.error=Some(e.to_string());}}
            });
            for theme in self.themes.clone(){ui.horizontal(|ui|{ui.label(theme["name"].as_str().unwrap_or("Theme"));if ui.button(format!("Use {}",theme["name"].as_str().unwrap_or("theme"))).clicked(){self.send("appearance","themes.apply",json!({"id":theme["id"]}));}
if ui.button(format!("Delete theme {}",theme["name"].as_str().unwrap_or("theme"))).clicked(){self.send("theme","themes.delete",json!({"id":theme["id"]}));self.send("themes","themes.list",json!({}));}});}
        });
        ui.collapsing("Network & browser playback cookies",|ui|{
            ui.label(format!("Proxy: {}",self.network_info["proxy"].as_str().unwrap_or("System network settings")));
            ui.horizontal(|ui|{ui.label("Proxy URL (HTTP or SOCKS5)");ui.add(egui::TextEdit::singleline(&mut self.proxy_input).password(true).hint_text("http://127.0.0.1:7890"));});
            ui.horizontal(|ui|{if ui.button("Apply proxy").clicked(){self.send("network","network.set",json!({"proxy":self.proxy_input}));}
if ui.button("Use system network").clicked(){self.send("network","network.set",json!({"proxy":null}));}
if ui.button("Test connection").clicked(){self.send("network-test","network.test",json!({}));}});
            ui.label("Proxy credentials are encrypted for your Windows account. HTTP and SOCKS5 settings also route audio playback.");
            let before=self.cookie_browser.clone();
            egui::ComboBox::from_label("Playback cookie browser").selected_text(if self.cookie_browser.is_empty(){"Disabled"}else{&self.cookie_browser}).show_ui(ui,|ui|{ui.selectable_value(&mut self.cookie_browser,String::new(),"Disabled");for browser in ["chrome","edge","firefox","brave","vivaldi","opera"]{ui.selectable_value(&mut self.cookie_browser,browser.into(),browser);}});
            if before!=self.cookie_browser{self.send("cookies","cookies.browser",json!({"browser":self.cookie_browser}));}
            ui.label("Used by the playback helper only when enabled. Locked/encrypted browser cookies may be unavailable; a Netscape cookie file remains supported below.");
        });
        ui.collapsing("Installation & Rust updates",|ui|{
            if ui.button("Install for this Windows user").clicked(){self.send("install","app.install",json!({}));}
            ui.label("Creates a Start menu shortcut. Library and account data stay in their current location.");
            ui.horizontal(|ui|{ui.label("Rust release repository");ui.text_edit_singleline(&mut self.release_repo);if ui.button("Save release source").clicked(){self.send("preferences","updates.configure",json!({"repo":self.release_repo}));}});
            ui.label("Official update source: Roynit/roynit-music. Clear this field to disable checks.");
            ui.horizontal(|ui|{
                if ui.button("Prepare previous installed version").clicked(){self.send("update","updates.rollback",json!({}));}
                if ui.button("Check for updates").clicked(){self.send("update","updates.check",json!({}));}
                if self.update_info["available"]==true&&ui.button("Download verified update").clicked(){self.send("update","updates.stage",json!({}));}
            });
            let mut automatic=self.library.settings.get("auto_update_check")!=Some(&json!(false));
            if ui.checkbox(&mut automatic,"Check for updates when Roynit starts").changed(){self.send("preferences","settings.set",json!({"auto_update_check":automatic}));}
            if let Some(reason)=self.update_info["reason"].as_str(){ui.label(reason);}
            if self.update_info["configured"]==false{ui.label("No release source configured");}
            if let Some(version)=self.update_info["version"].as_str(){ui.label(format!("Latest Rust release: {version}"));}
            if let Some(path)=self.update_info["staged_path"].as_str().map(str::to_owned){
                ui.label(if self.update_info["rollback"]==true {"Previous version verified against the installation receipt. Applying it will restart Riff and keep library data."}else{"Update downloaded and SHA-256 verified. Applying it will close and restart Riff."});
                if ui.button("Apply update and restart").clicked(){
                    match riff_core::tools::command(&path).args(["--install","--wait-pid",&std::process::id().to_string(),"--launch","--data-dir"]).arg(&self.data_root).spawn(){
                        Ok(_)=>{self.shutdown.store(true,Ordering::Relaxed);ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);}
                        Err(e)=>self.error=Some(e.to_string()),
                    }
                }
            }
        });
    }
    fn apply_appearance(&mut self, ctx: &egui::Context) {
        let signature = serde_json::to_string(&self.appearance).unwrap_or_default();
        if signature == self.appearance_signature {
            return;
        }
        self.appearance_signature = signature;
        let accent = Color32::from_rgb(
            self.appearance.accent[0],
            self.appearance.accent[1],
            self.appearance.accent[2],
        );
        let mut style = (*ctx.style()).clone();
        style.visuals = if self.appearance.mode == "light" {
            egui::Visuals::light()
        } else {
            egui::Visuals::dark()
        };
        style.visuals.selection.bg_fill = accent;
        let [r, g, b] = self.appearance.accent;
        let contrast = if 299 * r as u32 + 587 * g as u32 + 114 * b as u32 > 160_000 {
            Color32::BLACK
        } else {
            Color32::WHITE
        };
        style.visuals.selection.stroke.color = contrast;
        style.visuals.widgets.active.bg_fill = accent;
        style.animation_time = if self.appearance.reduced_motion {
            0.
        } else {
            0.12
        };
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::proportional(self.appearance.font_size),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::proportional(self.appearance.font_size),
        );
        ctx.set_style(style);
    }
    fn visualizer(&self, ui: &mut egui::Ui) {
        if self.appearance.visualizer == "off" || self.player["paused"] != false {
            return;
        }
        let accent = Color32::from_rgb(
            self.appearance.accent[0],
            self.appearance.accent[1],
            self.appearance.accent[2],
        );
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.), egui::Sense::hover());
        if self.appearance.visualizer == "bars" {
            let bins = self.player["analysis"]["spectrum"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let count = bins.len().min(64);
            for (i, bin) in bins.iter().take(count).enumerate() {
                let level = bin.as_f64().unwrap_or(0.).max(0.).sqrt().min(1.) as f32;
                let x = rect.left() + i as f32 / count.max(1) as f32 * rect.width();
                ui.painter().rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(x, rect.bottom() - level * rect.height()),
                        egui::pos2(x + rect.width() / count.max(1) as f32 - 2., rect.bottom()),
                    ),
                    1.,
                    accent,
                );
            }
        } else {
            let samples = self.player["analysis"]["samples"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let count = samples.len().min(256);
            let points: Vec<_> = samples
                .iter()
                .step_by((samples.len() / count.max(1)).max(1))
                .take(count)
                .enumerate()
                .map(|(i, v)| {
                    egui::pos2(
                        rect.left() + i as f32 / count.max(1) as f32 * rect.width(),
                        rect.center().y - v.as_f64().unwrap_or(0.) as f32 * rect.height() * 0.45,
                    )
                })
                .collect();
            if points.len() > 1 {
                ui.painter().add(egui::Shape::line(
                    points,
                    egui::Stroke::new(1.5_f32, accent),
                ));
            }
        }
    }
    fn content(&mut self, ui: &mut egui::Ui) {
        ui.add_space(15.);
        ui.heading(
            RichText::new(if self.page == Page::Browse {
                self.browse_title.as_str()
            } else {
                self.page.title()
            })
            .size(29.),
        );
        ui.add_space(8.);
        match self.page {
            Page::Home | Page::Search | Page::Browse => {
                if self.page == Page::Browse
                    && !self.navigation.is_empty()
                    && ui.button("Back to previous results").clicked()
                    && let Some((page, tracks, items, title, browse)) = self.navigation.pop()
                {
                    self.results_generation += 1;
                    self.page = page;
                    self.tracks = tracks;
                    self.items = items;
                    self.browse_title = title;
                    self.current_browse = browse;
                }

                if self.page == Page::Browse
                    && let Some(collection) = self.current_browse.clone()
                    && ui.button("Save this collection").clicked()
                {
                    self.send("saved", "collection.save", collection);
                }
                if self.page == Page::Home {
                    ui.label("Live selections from YouTube Music. Start a track's radio for its provider-generated queue.");
                    ui.horizontal(|ui| {
                        if ui.button("Radio from favorites").clicked() {
                            self.send("radio", "radio.personal", json!({"kind":"favorites"}));
                        }
                        if ui.button("Radio from listening history").clicked() {
                            self.send("radio", "radio.personal", json!({"kind":"history"}));
                        }
                        if ui.button("Refresh home").clicked() {
                            self.send("home", "music.home", json!({}));
                        }
                        if ui.button("Charts").clicked() {
                            self.send("home", "music.charts", json!({}));
                        }
                    });
                }
                self.catalog(ui);
                self.rows(ui, self.tracks.clone());
                if self.page == Page::Search
                    && [1, 2].contains(&self.platform)
                    && self.search_limit < 100
                    && !self
                        .job_tags
                        .values()
                        .any(|tag| self.current_tag(tag) == Some("search"))
                    && ui.button("Load more search results").clicked()
                {
                    self.search_limit = (self.search_limit + 20).min(100);
                    self.search_current();
                }
            }
            Page::Library => {
                if let Some(saved) = self
                    .library
                    .settings
                    .get("saved_collections")
                    .and_then(Value::as_object)
                    .cloned()
                {
                    ui.collapsing("Saved albums, artists & collections", |ui| {
                        for collection in saved.values() {
                            if ui
                                .button(collection["title"].as_str().unwrap_or("Collection"))
                                .clicked()
                            {
                                self.navigation.push((
                                    self.page,
                                    self.tracks.clone(),
                                    self.items.clone(),
                                    self.browse_title.clone(),
                                    self.current_browse.clone(),
                                ));
                                self.browse_title =
                                    collection["title"].as_str().unwrap_or("").into();
                                self.page = Page::Browse;
                                self.send("browse", "music.browse", json!({"id":collection["id"]}));
                            }
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if ui.button("Import audio files").clicked() {
                        self.import(false);
                    }
                    if ui.button("Scan a folder").clicked() {
                        self.import(true);
                    }
                });
                self.rows(ui, self.library.tracks.values().cloned().collect());
            }
            Page::Favorites => self.rows(
                ui,
                self.library
                    .favorites
                    .iter()
                    .filter_map(|id| self.library.tracks.get(id).cloned())
                    .collect(),
            ),
            Page::History => {
                if ui.button("Clear listening history").clicked() {
                    self.send("history", "history.clear", json!({}));
                }
                let mut seen = std::collections::HashSet::new();
                let tracks = self
                    .library
                    .history
                    .iter()
                    .rev()
                    .filter(|h| seen.insert(h.track_id.clone()))
                    .filter_map(|h| self.library.tracks.get(&h.track_id).cloned())
                    .take(200)
                    .collect();
                self.rows(ui, tracks);
            }
            Page::Offline => {
                self.rows(
                    ui,
                    self.library
                        .tracks
                        .values()
                        .filter(|t| {
                            t.local_path.as_ref().is_some_and(|p| p.is_file())
                                || t.cache_path.as_ref().is_some_and(|p| p.is_file())
                        })
                        .cloned()
                        .collect(),
                );
            }
            Page::Queue => {
                ui.label("Radio keeps the order selected by YouTube Music. Repeat and shuffle remain your choice.");
                self.rows(
                    ui,
                    self.library
                        .queue
                        .tracks
                        .iter()
                        .filter_map(|id| self.library.tracks.get(id).cloned())
                        .collect(),
                );
            }
            Page::Playlists => {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.import_url).hint_text(
                            "Paste a YouTube, SoundCloud, Spotify or Deezer playlist link",
                        ),
                    );
                    if ui.button("Import link").clicked() {
                        let host = url::Url::parse(&self.import_url)
                            .ok()
                            .and_then(|u| u.host_str().map(str::to_string))
                            .unwrap_or_default();
                        let operation = if self.import_url.starts_with("spotify:playlist:")
                            || host.contains("spotify.com")
                            || host.contains("deezer.com")
                            || host.contains("apple.com")
                            || host.contains("yandex.")
                            || host == "vk.com"
                            || host.ends_with(".vk.com")
                            || host == "vk.ru"
                            || host.ends_with(".tiktok.com")
                            || host == "tiktok.com"
                        {
                            "catalog.import"
                        } else {
                            "collection.import"
                        };
                        self.job("import", operation, json!({"url":self.import_url}));
                    }
                });
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.playlist_name);
                    if ui
                        .button(format!(
                            "Create from {} selected tracks",
                            self.selected.len()
                        ))
                        .clicked()
                        && !self.playlist_name.trim().is_empty()
                    {
                        self.send(
                            "playlist",
                            "playlist.save",
                            json!({"name":self.playlist_name,"tracks":self.selected}),
                        );
                        self.playlist_name.clear();
                    }
                });
                let playlists = self.library.playlists.values().cloned().collect::<Vec<_>>();
                ui.horizontal_wrapped(|ui| {
                    for playlist in playlists {
                        if ui
                            .selectable_label(
                                self.active_playlist.as_deref() == Some(&playlist.id),
                                &playlist.name,
                            )
                            .clicked()
                        {
                            self.rename_playlist = playlist.name.clone();
                            self.active_playlist = Some(playlist.id);
                        }
                    }
                });
                if let Some(playlist) = self
                    .active_playlist
                    .as_ref()
                    .and_then(|id| self.library.playlists.get(id))
                    .cloned()
                {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Playlist name");
                        ui.text_edit_singleline(&mut self.rename_playlist);
                        if ui.button("Rename playlist").clicked()
                            && !self.rename_playlist.trim().is_empty()
                        {
                            let mut value = playlist.clone();
                            value.name = self.rename_playlist.clone();
                            self.send("playlist", "playlist.save", json!(value));
                        }
                        if ui.button("Add selected tracks").clicked() {
                            let mut value = playlist.clone();
                            value.tracks.extend(self.selected.clone());
                            self.send("playlist", "playlist.save", json!(value));
                            self.selected.clear();
                        }
                        if ui.button("Download playlist").clicked() {
                            self.send(
                                "download.bulk",
                                "download.bulk",
                                json!({"ids":playlist.tracks,"format":self.library.settings.get("download_format").and_then(Value::as_str).unwrap_or("opus")}),
                            );
                        }
                        if ui.button("Delete playlist").clicked() {
                            self.send("playlist", "playlist.delete", json!({"id":playlist.id}));
                            self.active_playlist = None;
                        }
                    });
                    if ui.button("Play playlist").clicked() {
                        self.send(
                            "playlist-play",
                            "queue.set",
                            json!({"tracks":playlist.tracks,"play":true}),
                        );
                    }
                    self.rows(
                        ui,
                        playlist
                            .tracks
                            .iter()
                            .filter_map(|id| self.library.tracks.get(id).cloned())
                            .collect(),
                    );
                }
            }
            Page::Lyrics => {
                let before = self.lyric_source.clone();
                ui.horizontal(|ui| {
                    ui.label("Lyric source");
                    egui::ComboBox::from_id_salt("lyric-source")
                        .selected_text(&self.lyric_source)
                        .show_ui(ui, |ui| {
                            for source in
                                ["auto", "lrclib", "netease", "kugou", "musixmatch", "genius"]
                            {
                                ui.selectable_value(&mut self.lyric_source, source.into(), source);
                            }
                        });
                    ui.label("Source language");ui.add_sized([60.,25.],egui::TextEdit::singleline(&mut self.translation_source));
                    ui.label("Translate to");
                    ui.add_sized(
                        [60., 25.],
                        egui::TextEdit::singleline(&mut self.translation_target),
                    );
                    if ui.button("Translate").clicked() {
                        let text = if self.lyric_lines.is_empty() {
                            self.lyric_text.clone()
                        } else {
                            self.lyric_lines
                                .iter()
                                .map(|l| l.text.as_str())
                                .collect::<Vec<_>>()
                                .join("\n")
                        };
                        self.job(
                            "translation",
                            "translate",
                            json!({"text":text,"target":self.translation_target,"source":self.translation_source}),
                        );
                    }
                });
                if before != self.lyric_source
                    && let Some(track) = self.player["track_id"]
                        .as_str()
                        .and_then(|id| self.library.tracks.get(id))
                        .cloned()
                {
                    self.job("lyrics","lyrics.get",json!({"title":track.title,"artist":track.artist,"duration":track.duration,"source":self.lyric_source}));
                }
                if !self.translated.is_empty() {
                    ui.collapsing("Translation", |ui| {
                        ui.label(&self.translated);
                    });
                }
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.follow_lyrics, "Follow current line");
                    ui.add(
                        egui::Slider::new(&mut self.lyric_offset, -10.0..=10.)
                            .text("Timing offset (s)"),
                    );
                    if ui.button("Import lyrics file…").clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("Lyrics", &["lrc", "txt"])
                            .pick_file()
                        && let Ok(text) = std::fs::read_to_string(path)
                        && let Some(track) = self.player["track_id"]
                            .as_str()
                            .and_then(|id| self.library.tracks.get(id))
                            .cloned()
                    {
                        self.send(
                            "lyrics",
                            "lyrics.save",
                            json!({"title":track.title,"artist":track.artist,"text":text}),
                        );
                    }
                });
                if ui.input(|i| i.raw_scroll_delta.y != 0.) {
                    self.follow_lyrics = false;
                }
                if self.lyric_lines.is_empty() {
                    ui.label(if self.lyric_text.is_empty() {
                        "No lyrics loaded for the current track."
                    } else {
                        &self.lyric_text
                    });
                } else {
                    let index = riff_core::lyrics::active(
                        &self.lyric_lines,
                        self.player["position"].as_f64().unwrap_or(0.),
                        self.lyric_offset,
                    );
                    let now = self.player["position"].as_f64().unwrap_or(0.) + self.lyric_offset;
                    let mut seek_line = None;
                    for (i, line) in self.lyric_lines.iter().enumerate() {
                        ui.add_space(10.);
                        let active = index == Some(i);
                        let font = egui::FontId::proportional(if active { 26. } else { 21. });
                        let accent = Color32::from_rgb(
                            self.appearance.accent[0],
                            self.appearance.accent[1],
                            self.appearance.accent[2],
                        );
                        let base = if active {
                            ui.visuals().text_color()
                        } else {
                            ui.visuals().weak_text_color()
                        };
                        let mut layout = egui::text::LayoutJob::default();
                        let mut cursor = 0;
                        for word in &line.words {
                            let text = word.text.trim_end();
                            if text.is_empty() {
                                continue;
                            }
                            if let Some(relative) = line.text[cursor..].find(text) {
                                let at = cursor + relative;
                                layout.append(
                                    &line.text[cursor..at],
                                    0.,
                                    egui::TextFormat::simple(font.clone(), base),
                                );
                                let sung = active
                                    && now >= word.time
                                    && word.end.is_none_or(|end| now < end);
                                layout.append(
                                    text,
                                    0.,
                                    egui::TextFormat::simple(
                                        font.clone(),
                                        if sung { accent } else { base },
                                    ),
                                );
                                cursor = at + text.len();
                            }
                        }
                        layout.append(
                            &line.text[cursor..],
                            0.,
                            egui::TextFormat::simple(
                                font,
                                if active && line.words.is_empty() {
                                    accent
                                } else {
                                    base
                                },
                            ),
                        );
                        let response =
                            ui.add(egui::Label::new(layout).wrap().sense(egui::Sense::click()));
                        if response.clicked() {
                            seek_line = Some(line.time);
                        }
                        if self.follow_lyrics && active && self.player["paused"] == false {
                            response.scroll_to_me(Some(egui::Align::Center));
                        }
                    }
                    if let Some(seconds) = seek_line {
                        self.send("seek", "player.seek", json!({"seconds":seconds}));
                    }
                }
            }
            Page::Downloads => {
                if ui.button("Open downloads folder").clicked() {
                    let _ = open::that(self.data_root.join("downloads"));
                }
                if self.jobs.is_empty() {
                    ui.label("No tasks yet. Use a track's More menu to download it.");
                }
                if ui.button("Cancel all pending tasks").clicked() {
                    self.send("cancel", "jobs.cancel_all", json!({}));
                }
                let active = self
                    .jobs
                    .values()
                    .filter(|j| j["state"] == "running" || j["state"] == "queued")
                    .count();
                ui.label(format!("{active} active or queued tasks"));
                let mut tasks = self.jobs.values().cloned().collect::<Vec<_>>();
                tasks.sort_by_key(|j| !(j["state"] == "running" || j["state"] == "queued"));
                for job in tasks.into_iter().take(100) {
                    ui.group(|ui| {
                        ui.label(format!(
                            "{} — {}",
                            job["operation"].as_str().unwrap_or("Task"),
                            job["state"].as_str().unwrap_or("")
                        ));
                        if let Some(error) = job["error"].as_str() {
                            ui.colored_label(Color32::LIGHT_RED, error);
                        }
                        if (job["state"] == "running" || job["state"] == "queued")
                            && ui.button("Cancel task").clicked()
                        {
                            self.send("cancel", "job.cancel", json!({"id":job["id"]}));
                        }
                    });
                }
            }
            Page::Settings => {
                self.playback_preferences(ui);
                let mut media_enabled = self.media_keys.is_some();
                if ui
                    .checkbox(&mut media_enabled, "Use global media keys")
                    .changed()
                {
                    if media_enabled {
                        self.enable_media_keys();
                    } else {
                        self.media_keys.take();
                    }
                    self.send(
                        "preferences",
                        "settings.set",
                        json!({"media_keys":self.media_keys.is_some()}),
                    );
                }
                if self._tray.is_some()
                    && ui
                        .checkbox(
                            &mut self.close_to_tray,
                            "Keep playing in system tray when closing the window",
                        )
                        .changed()
                {
                    self.send(
                        "preferences",
                        "settings.set",
                        json!({"close_to_tray":self.close_to_tray}),
                    );
                }
                ui.horizontal(|ui| {
                    if ui.button("Mini player").clicked() {
                        self.compact = true;
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
                                380., 180.,
                            )));
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                                480., 260.,
                            )));
                    }
                    if ui.button("Fullscreen lyrics").clicked() {
                        self.page = Page::Lyrics;
                        self.fullscreen = true;
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
                    }
                });
                ui.label("Roynit Music by dang. Native interface preview.");
                ui.horizontal(|ui| {
                    if ui.button("Check audio tools").clicked() {
                        self.send("tools", "tools.status", json!({}));
                    }
                    if ui.button("Install / update yt-dlp").clicked() {
                        self.job("tools", "tools.install", json!({"name":"yt-dlp"}));
                    }
                });
                ui.separator();
                ui.heading("YouTube Music session");
                ui.label("Radio works without signing in. To let YouTube use your session, choose your own exported Netscape cookie file. Personalized results remain controlled by YouTube.");
                if ui.button("Choose YouTube cookie file…").clicked()
                    && let Some(path) = rfd::FileDialog::new().pick_file()
                {
                    self.send("cookies", "cookies.set", json!({"path":path}));
                    self.status = "YouTube cookie file configured for this session.".into();
                }
                if ui.button("Stop using cookie file").clicked() {
                    self.send("cookies-cleared", "cookies.clear", json!({}));
                }
                ui.separator();
                ui.heading("Google / Spotify account");
                ui.label("Use your own app client ID. Register callback http://127.0.0.1:8751/callback. Google library sign-in is separate from a YouTube Music cookie session.");
                egui::ComboBox::from_id_salt("oauth-provider")
                    .selected_text(if self.oauth_provider == 0 {
                        "Google"
                    } else {
                        "Spotify"
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.oauth_provider, 0, "Google");
                        ui.selectable_value(&mut self.oauth_provider, 1, "Spotify");
                    });
                ui.horizontal(|ui| {
                    ui.label("Client ID");
                    ui.text_edit_singleline(&mut self.oauth_client);
                });
                ui.horizontal(|ui| {
                    ui.label("Client secret (if required)");
                    ui.add(egui::TextEdit::singleline(&mut self.oauth_secret).password(true));
                });
                if ui.button("Sign in with system browser").clicked() {
                    self.login();
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Import Spotify liked tracks").clicked() {
                        self.send("import", "spotify.import", json!({"kind":"liked"}));
                    }
                    if ui.button("Load my Spotify playlists").clicked() {
                        self.send(
                            "spotify-playlists",
                            "spotify.library",
                            json!({"kind":"playlists"}),
                        );
                    }
                    if ui.button("Import YouTube likes").clicked() {
                        self.send("import", "google.import", json!({"kind":"likes"}));
                    }
                    if ui.button("Load my YouTube playlists").clicked() {
                        self.job(
                            "google-playlists",
                            "google.library",
                            json!({"kind":"playlists"}),
                        );
                    }
                    if ui.button("Check account connection").clicked() {
                        self.send(
                            "account-status",
                            "oauth.status",
                            json!({"provider":if self.oauth_provider==0{"google"}else{"spotify"}}),
                        );
                    }
                    if ui.button("Sign out selected account").clicked() {
                        self.retry_login = false;
                        self.login_generation = self.login_generation.wrapping_add(1);
                        if let Some(cancel) = &self.login_cancel {
                            cancel.store(true, Ordering::Relaxed);
                        }
                        self.send(
                            "logout",
                            "oauth.logout",
                            json!({"provider":if self.oauth_provider==0{"google"}else{"spotify"}}),
                        );
                    }
                    if ui.button("Refresh account session").clicked() {
                        self.send(
                            "auth",
                            "oauth.refresh",
                            json!({"provider":if self.oauth_provider==0{"google"}else{"spotify"}}),
                        );
                    }
                });
                if self.account_status.is_object()
                    && !self.account_status.as_object().unwrap().is_empty()
                {
                    ui.label(format!(
                        "{}: {}",
                        self.account_status["provider"].as_str().unwrap_or(
                            if self.oauth_provider == 0 {
                                "google"
                            } else {
                                "spotify"
                            }
                        ),
                        if self.account_status["signed_in"] == true {
                            "Connected"
                        } else if self.account_status["pending"] == true {
                            "Waiting for sign-in"
                        } else {
                            "Not connected"
                        }
                    ));
                }
                for playlist in self.spotify_playlists.clone() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(playlist["name"].as_str().unwrap_or("Spotify playlist"));
                        if ui
                            .button(format!(
                                "Import {}",
                                playlist["name"].as_str().unwrap_or("Spotify playlist")
                            ))
                            .clicked()
                        {
                            self.job(
                                "import",
                                "catalog.import",
                                json!({"url":playlist["external_urls"]["spotify"]}),
                            );
                        }
                    });
                }
                for playlist in self.google_playlists.clone() {
                    let name = playlist["snippet"]["title"]
                        .as_str()
                        .unwrap_or("YouTube playlist");
                    if ui.button(format!("Import {name}")).clicked() {
                        self.send(
                            "import",
                            "google.import",
                            json!({"kind":"tracks","playlist_id":playlist["id"],"name":name}),
                        );
                    }
                }
                ui.separator();
                ui.heading("Library backup");
                ui.collapsing("Discord Rich Presence", |ui| {
                    ui.label("Share the current track with Discord only when enabled.");
                    ui.horizontal(|ui| {
                        ui.label("Application ID");
                        ui.text_edit_singleline(&mut self.discord_id);
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Connect Discord").clicked() {
                            self.send(
                                "discord",
                                "discord.connect",
                                json!({"application_id":self.discord_id}),
                            );
                        }
                        if ui.button("Disable Discord").clicked() {
                            self.send("discord", "discord.disconnect", json!({}));
                        }
                    });
                });
                ui.collapsing("Last.fm scrobbling",|ui|{ui.label("Uses your own Last.fm application credentials. Scrobbles only after enough actual listening time.");ui.horizontal(|ui|{ui.label("API key");ui.text_edit_singleline(&mut self.lastfm_key);});ui.horizontal(|ui|{ui.label("API secret");ui.add(egui::TextEdit::singleline(&mut self.lastfm_secret).password(true));});ui.horizontal(|ui|{if ui.button("Authorize Last.fm").clicked(){self.send("authorize","lastfm.authorize",json!({"api_key":self.lastfm_key,"api_secret":self.lastfm_secret}));}
if ui.button("Finish Last.fm authorization").clicked(){self.send("lastfm-status","lastfm.finish",json!({}));}
if ui.button("Disable Last.fm").clicked(){self.send("lastfm-status","lastfm.enable",json!({"enabled":false}));}
if ui.button("Disconnect Last.fm account").clicked(){self.send("lastfm-status","lastfm.disconnect",json!({}));}});if self.lastfm_status["signed_in"]==true{ui.label(format!("Connected as {}",self.lastfm_status["username"].as_str().unwrap_or("")));}});
                ui.horizontal(|ui|{
                    if ui.button("Export backup…").clicked()&&let Some(path)=rfd::FileDialog::new().set_file_name("riff-library.json").save_file(){self.send("backup","backup.export",json!({"path":path}));}
                    if ui.button("Import backup…").clicked()
                        && let Some(path)=rfd::FileDialog::new().add_filter("Riff JSON",&["json"]).pick_file()
                        && rfd::MessageDialog::new().set_title("Replace library?").set_description("This replaces the current library with the selected backup. A before-import backup is kept.").set_buttons(rfd::MessageButtons::YesNo).show()==rfd::MessageDialogResult::Yes{self.send("backup","backup.import",json!({"path":path}));}
                });
                if ui
                    .button("Import original Riff account / storage…")
                    .clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("Original Riff account", &["rfacc", "json"])
                        .pick_file()
                {
                    self.send("migration", "legacy.import", json!({"path":path}));
                }
                if ui.button("Export original Riff .rfacc…").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .set_file_name("riff-account.rfacc")
                        .save_file()
                {
                    self.send("export", "account.export", json!({"path":path}));
                }
                if ui.button("Open app data folder").clicked() {
                    let _ = open::that(&self.data_root);
                }
                ui.label(format!("Data: {}", self.data_root.display()));
                ui.separator();
                if ui.button("Save diagnostic report…").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .set_file_name("riff-diagnostics.json")
                        .save_file()
                {
                    self.send("support", "support.export", json!({"path":path}));
                }
                ui.heading("Extension health");
                for area in ["themes", "plugins"] {
                    if let Some(warnings) = self.extension_status[area]["warnings"].as_array() {
                        for warning in warnings {
                            ui.label(format!(
                                "Skipped invalid {area} file {}: {}",
                                warning["file"].as_str().unwrap_or("manifest"),
                                warning["error"].as_str().unwrap_or("Invalid format")
                            ));
                        }
                    }
                }
                ui.heading("Native Rust plugins");
                ui.label("Native plugins run as your Windows user. Enable only code you trust. Original JavaScript plugins need to be rewritten.");
                if ui.button("Import native plugin manifest…").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("Plugin manifest", &["json"])
                        .pick_file()
                {
                    self.send("plugin", "plugins.import", json!({"path":path}));
                }
                for plugin in self.plugins.clone() {
                    ui.horizontal(|ui| {
                        ui.label(plugin["name"].as_str().unwrap_or("Plugin"));
                        let mut enabled = plugin["enabled"].as_bool().unwrap_or(false);
                        if ui.checkbox(&mut enabled, "Enabled").changed() {
                            self.send(
                                "plugin",
                                "plugins.enable",
                                json!({"id":plugin["id"],"enabled":enabled}),
                            );
                        }
                        if enabled && ui.button("Run plugin").clicked() {
                            self.send("plugin", "plugins.run", json!({"id":plugin["id"]}));
                        }
                    });
                }
            }
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested())
            && self.close_to_tray
            && !self.shutdown.load(Ordering::Relaxed)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        // egui exposes UIA ValuePattern, but its text editor does not consume SetValue.
        // Honor that semantic action so assistive tools can edit search without keystrokes.
        ctx.input(|input| {
            for event in &input.events {
                if let egui::Event::AccessKitActionRequest(request) = event
                    && request.target.0 == egui::Id::new("search-query").value()
                    && let Some(egui::accesskit::ActionData::Value(value)) = &request.data
                {
                    self.query = value.to_string();
                }
            }
        });
        self.receive();
        if self.retry_login
            && self
                .login_thread
                .as_ref()
                .is_some_and(|thread| thread.is_finished())
        {
            self.login();
        }
        self.apply_appearance(ctx);
        if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Comma)) {
            self.open_settings();
        }
        if let Some(media) = self.media.as_mut() {
            let track = self.player["track_id"]
                .as_str()
                .and_then(|id| self.library.tracks.get(id));
            if let Err(error) = media.update(track, &self.player) {
                self.error = Some(format!("Windows media session: {error}"));
            }
        }
        if self
            .effects_pending
            .is_some_and(|t| t.elapsed() >= Duration::from_millis(250))
        {
            self.effects_pending = None;
            self.send_effects();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F11)) {
            self.fullscreen = !self.fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.fullscreen {
            self.fullscreen = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
        if !ctx.wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.send(
                "toggle",
                if self.player["paused"].as_bool().unwrap_or(true) {
                    "player.resume"
                } else {
                    "player.pause"
                },
                json!({}),
            );
        }
        ctx.request_repaint_after(Duration::from_millis(80));
        if self.compact {
            egui::CentralPanel::default().show(ctx, |ui| {
                if ui.button("Back to full player").clicked() {
                    self.compact = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
                        1000., 600.,
                    )));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                        1120., 700.,
                    )));
                }
                let track = self.player["track_id"]
                    .as_str()
                    .and_then(|id| self.library.tracks.get(id));
                ui.heading(track.map_or("Riff", |t| t.title.as_str()));
                ui.label(track.map_or("Choose a track in the full player", |t| t.artist.as_str()));
                ui.horizontal(|ui| {
                    if ui
                        .button(if self.player["paused"].as_bool().unwrap_or(true) {
                            "Play"
                        } else {
                            "Pause"
                        })
                        .clicked()
                    {
                        self.send(
                            "toggle",
                            if self.player["paused"].as_bool().unwrap_or(true) {
                                "player.resume"
                            } else {
                                "player.pause"
                            },
                            json!({}),
                        );
                    }
                    if ui.button("Next").clicked() {
                        self.send("next", "player.next", json!({}));
                    }
                    if ui.button("Stop").clicked() {
                        self.send("stop", "player.stop", json!({}));
                    }
                    ui.label(time(self.player["position"].as_f64().unwrap_or(0.)));
                });
            });
            return;
        }
        self.player_bar(ctx);
        egui::TopBottomPanel::top("search_bar").show(ctx, |ui| {
            ui.add_space(7.);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("ROYNIT")
                        .size(24.)
                        .strong()
                        .color(ui.visuals().text_color()),
                );
                ui.label(
                    RichText::new("RUST PREVIEW")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
                ui.add_space(25.);
                let response = ui.add_sized(
                    [ui.available_width() - 340., 34.],
                    egui::TextEdit::singleline(&mut self.query)
                        .id(egui::Id::new("search-query"))
                        .hint_text("Search songs, artists, albums…"),
                );
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.search();
                }
                egui::ComboBox::from_label("Source")
                    .selected_text(["YouTube Music", "YouTube", "SoundCloud", "Spotify"][self.platform])
                    .show_ui(ui, |ui| {
                        for (i, name) in ["YouTube Music", "YouTube", "SoundCloud", "Spotify"]
                            .iter()
                            .enumerate()
                        {
                            ui.selectable_value(&mut self.platform, i, *name);
                        }
                    });
                if ui.button("Search").clicked() {
                    self.search();
                }
            });
            if self.platform == 3 { ui.label("Spotify public search and playlist links - no API key needed. Play uses a matched YouTube Music recording. Sign-in is optional."); }
            if self.platform == 0 {
                ui.horizontal(|ui| {
                    ui.add_space(156.);
                    ui.label("Find");
                    for (kind, label) in [
                        ("songs", "Songs"),
                        ("albums", "Albums"),
                        ("artists", "Artists"),
                        ("playlists", "Playlists"),
                    ] {
                        ui.selectable_value(&mut self.search_filter, kind.into(), label);
                    }
                });
            }
            ui.add_space(7.);
        });
        egui::SidePanel::left("navigation")
            .exact_width(190.)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(24.);
                egui::ScrollArea::vertical()
                    .id_salt("navigation-list")
                    .max_height((ui.available_height() - 55.).max(120.))
                    .show(ui, |ui| {
                        for (page, label) in [
                            (Page::Home, "Listen now"),
                            (Page::Library, "Your library"),
                            (Page::History, "Recently played"),
                            (Page::Offline, "Offline music"),
                            (Page::Favorites, "Favorites"),
                            (Page::Playlists, "Playlists"),
                            (Page::Queue, "Up next"),
                            (Page::Lyrics, "Lyrics"),
                            (Page::Downloads, "Downloads"),
                            (Page::Settings, "Settings"),
                        ] {
                            if ui
                                .add_sized(
                                    [170., 40.],
                                    egui::Button::new(RichText::new(label).color(
                                        if self.page == page {
                                            ui.visuals().selection.stroke.color
                                        } else {
                                            ui.visuals().text_color()
                                        },
                                    ))
                                    .selected(self.page == page),
                                )
                                .clicked()
                            {
                                self.page = page;
                                if page == Page::Home {
                                    self.send("home", "music.home", json!({}));
                                }
                                if page == Page::Settings {
                                    self.open_settings();
                                }
                            }
                        }
                    });
                ui.add_space(25.);
                ui.label(
                    RichText::new(format!(
                        "{} tracks in your library",
                        self.library.tracks.len()
                    ))
                    .small()
                    .color(ui.visuals().weak_text_color()),
                );
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(error) = self.error.clone() {
                egui::Frame::new()
                    .fill(Color32::from_rgb(65, 31, 38))
                    .inner_margin(10.)
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(error).color(Color32::from_rgb(255, 193, 198)));
                            if ui.button("Dismiss error").clicked() {
                                self.error = None;
                            }
                        });
                    });
            }
            ui.horizontal(|ui| {
                if self.busy {
                    ui.spinner();
                }
                ui.label(
                    RichText::new(&self.status)
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            });
            egui::ScrollArea::vertical()
                .id_salt(format!("content-{:?}", self.page as u8))
                .show(ui, |ui| self.content(ui));
        });
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.media_keys.take();
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = self.tx.send(Request {
            tag: "quit".into(),
            command: "app.quit".into(),
            params: json!({}),
        });
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn playback_status(state: &Value) -> &'static str {
    if state["pending_track_id"].is_string() {
        "Reconnecting audio…"
    } else if state["track_id"].is_null() || state["finished"] == true {
        "Playback stopped"
    } else if state["paused"] == true {
        "Playback paused"
    } else if state["buffering"] == true {
        "Buffering audio…"
    } else if state["playing"] != true {
        "Connecting audio…"
    } else {
        "Playing"
    }
}
fn time(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0. {
        return "0:00".into();
    }
    let n = seconds as u64;
    format!("{}:{:02}", n / 60, n % 60)
}
fn main() -> eframe::Result {
    let arguments: Vec<_> = std::env::args().collect();
    if arguments.iter().any(|a| a == "--install") {
        let pid = arguments
            .iter()
            .position(|a| a == "--wait-pid")
            .and_then(|at| arguments.get(at + 1))
            .and_then(|p| p.parse::<u32>().ok())
            .unwrap_or(0);
        let result = (|| -> anyhow::Result<()> {
            riff_core::installation::wait_for(pid)?;
            let installed = riff_core::installation::install(&std::env::current_exe()?)?;
            if arguments.iter().any(|a| a == "--launch") {
                let mut launch =
                    riff_core::tools::command(installed["path"].as_str().unwrap_or(""));
                if let Some(root) = arguments
                    .iter()
                    .position(|a| a == "--data-dir")
                    .and_then(|at| arguments.get(at + 1))
                {
                    launch.arg("--data-dir").arg(root);
                }
                let _ = launch.spawn()?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("Installation failed: {error:#}");
            rfd::MessageDialog::new()
                .set_title("Riff installation failed")
                .set_description(format!("{error:#}"))
                .set_level(rfd::MessageLevel::Error)
                .show();
            std::process::exit(1);
        }
        return Ok(());
    }

    let root = std::env::args()
        .skip_while(|a| a != "--data-dir")
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| Engine::default_root().ok())
        .expect("Cannot find data directory");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1120., 700.])
            .with_position([20., 20.])
            .with_min_inner_size([1000., 600.]),
        ..Default::default()
    };
    eframe::run_native(
        "Roynit Music",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, root)))),
    )
}

#[cfg(test)]
mod playback_status_tests {
    use super::*;
    #[test]
    fn status_never_claims_playing_before_audio_or_after_failure() {
        assert_eq!(
            playback_status(&json!({"track_id":null})),
            "Playback stopped"
        );
        assert_eq!(
            playback_status(
                &json!({"track_id":"a","paused":false,"playing":false,"buffering":true})
            ),
            "Buffering audio…"
        );
        assert_eq!(
            playback_status(&json!({"track_id":"a","paused":false,"playing":false})),
            "Connecting audio…"
        );
        assert_eq!(
            playback_status(&json!({"track_id":"a","paused":false,"playing":true,"finished":true})),
            "Playback stopped"
        );
        assert_eq!(
            playback_status(&json!({"track_id":"a","paused":false,"playing":true})),
            "Playing"
        );
        assert_eq!(
            playback_status(&json!({"pending_track_id":"a"})),
            "Reconnecting audio…"
        );
    }
}
