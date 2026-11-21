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
    live_search_at: Option<Instant>,
    mixes: Vec<riff_core::mixes::Mix>,
    suggested: Vec<Track>,
    mix_signature: Option<(usize, Option<u64>, Vec<String>)>,
    mix_loading: bool,
    mix_error: bool,
    mix_generation: u64,
    youtube_mix_signal: Value,
    mix_warnings: Vec<String>,
    hover_candidate: Option<(Track, Instant)>,
    hover_warmed: Option<String>,
    first_visible_warmed: Option<String>,
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
    cover_tints: Arc<std::sync::Mutex<HashMap<String, Option<Color32>>>>,
    artwork_for: Option<String>,
    artwork_since: Instant,
    side_panel: Option<Page>,
    narrow_open: bool,
    settings_group: usize,
    settings_query: String,
    settings_panel_stash: Option<Option<Page>>,
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
        style.visuals = graphite_visuals();
        style.visuals.selection.bg_fill = IRIS;
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
            live_search_at: None,
            mixes: Vec::new(),
            suggested: Vec::new(),
            mix_signature: None,
            mix_loading: false,
            mix_error: false,
            mix_generation: 0,
            youtube_mix_signal: Value::Null,
            mix_warnings: Vec::new(),
            hover_candidate: None,
            hover_warmed: None,
            first_visible_warmed: None,
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
            cover_tints: Default::default(),
            artwork_for: None,
            artwork_since: Instant::now(),
            side_panel: Some(Page::Queue),
            narrow_open: false,
            settings_group: 0,
            settings_query: String::new(),
            settings_panel_stash: None,
        };
        if let Some(error) = media_error {
            app.error = Some(format!("Windows media session unavailable: {error}"));
        }
        app.send("home", "music.home", json!({}));
        // UI checks open a tab directly (e.g. "Settings" or "search:den vau") without driving the desktop.
        if let Ok(start) = std::env::var("ROYNIT_START_PAGE") {
            if let Some(query) = start.strip_prefix("search:") {
                app.query = query.into();
                app.search();
            } else if let Some(query) = start.strip_prefix("settings:") {
                app.open_settings();
                app.settings_query = query.into();
            } else if start == "mini" {
                Self::set_mini(ctx, &mut app.compact, true);
            } else if let Some(page) = [
                Page::Library,
                Page::History,
                Page::Offline,
                Page::Favorites,
                Page::Playlists,
                Page::Queue,
                Page::Lyrics,
                Page::Downloads,
                Page::Settings,
            ]
            .into_iter()
            .find(|p| p.title().eq_ignore_ascii_case(&start))
            {
                app.page = page;
                if page == Page::Settings {
                    app.open_settings();
                }
            }
        }
        app
    }
    fn send(&mut self, tag: &str, command: &str, params: Value) {
        let tag = if tag == "mixes" {
            format!("mixes:{}", self.mix_generation)
        } else if tag == "oauth-authorize" || tag == "auth" {
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
        if let Some(number) = tag.strip_prefix("mixes:") {
            return (number.parse::<u64>().ok() == Some(self.mix_generation)).then_some("mixes");
        }
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
            if tag != "warm" && !tag.starts_with("mixes:") {
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
                self.reset_mixes();
                self.spotify_playlists.clear();
                self.google_playlists.clear();
                self.account_status = json!({"signed_in":false});
                self.status = "Account disconnected; saved library retained".into();
            }
            "cookies" => {
                self.reset_mixes();
                self.load_mixes_if_needed();
            }
            "cookies-cleared" => {
                self.reset_mixes();
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
                if data["provider"] == "google" || self.oauth_provider == 0 {
                    self.reset_mixes();
                    self.load_mixes_if_needed();
                }
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
            "mixes" => {
                self.mix_loading = false;
                self.mix_error = false;
                self.youtube_mix_signal = data["youtube_signal"].clone();
                self.mix_warnings =
                    serde_json::from_value(data["warnings"].clone()).unwrap_or_default();
                if data["youtube_oauth"] == true {
                    self.google_playlists =
                        data["playlists"].as_array().cloned().unwrap_or_default();
                }
                self.mixes = serde_json::from_value(data["mixes"].clone()).unwrap_or_default();
                self.suggested = serde_json::from_value(data["picks"].clone()).unwrap_or_default();
            }
            "mix-saved" => self.status = "Mix saved to your playlists".into(),
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
                    Err(_) if self.current_tag(&tag) == Some("mixes") => {
                        self.mix_loading = false;
                        self.mix_error = true;
                    }
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
                            } else if self.current_tag(&tag) == Some("mixes") {
                                self.mix_loading = false;
                                self.mix_error = true;
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
                                && j["operation"] != "autoplay.fill"
                                && j["operation"] != "music.mixes"
                                && !j["operation"]
                                    .as_str()
                                    .is_some_and(|op| op.starts_with("mixes:"))
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
    fn schedule_live_search(&mut self) {
        self.live_search_at = None;
        if self.page != Page::Search && self.query.trim().chars().count() < 2 {
            return;
        }
        self.results_generation += 1;
        self.tracks.clear();
        self.items.clear();
        self.error = None;
        if self.query.trim().chars().count() >= 2 {
            self.page = Page::Search;
            self.busy = true;
            self.status = "Searching…".into();
            self.live_search_at = Some(Instant::now() + Duration::from_millis(300));
        } else {
            self.busy = false;
            self.status.clear();
        }
    }
    fn search(&mut self) {
        self.live_search_at = None;
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
                ui.add(
                    egui::TextEdit::singleline(&mut self.library_filter)
                        .hint_text("Filter library")
                        .desired_width(220.),
                );
                egui::ComboBox::from_id_salt("Sort library")
                    .selected_text(format!(
                        "Sort: {}",
                        ["Title", "Artist", "Duration"][self.library_sort]
                    ))
                    .show_ui(ui, |ui| {
                        for (i, label) in ["Title", "Artist", "Duration"].iter().enumerate() {
                            ui.selectable_value(&mut self.library_sort, i, *label);
                        }
                    });
                if self.page == Page::Library {
                    self.import_menu(ui);
                }
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
            self.empty_state(ui);
            return;
        }
        if !self.selected.is_empty() {
            self.selection_bar(ui);
        }
        let accent = ui.visuals().selection.bg_fill;
        let reorder = matches!(self.page, Page::Queue | Page::Playlists);
        let selecting = !self.selected.is_empty();
        let mut hovered = None;
        let mut first_visible = None;
        ui.spacing_mut().item_spacing.y = 2.;
        for (index, track) in tracks.iter().enumerate() {
            let current = self.player["track_id"] == track.id;
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 48.), egui::Sense::click());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    format!("Play {} by {} [{}]", track.title, track.artist, track.id),
                )
            });
            let pointer_inside = ui.rect_contains_pointer(rect);
            if ui.is_rect_visible(rect) {
                let fill = if current {
                    accent.gamma_multiply(0.14)
                } else if pointer_inside || response.has_focus() {
                    Color32::from_white_alpha(8)
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter().rect_filled(rect, 10., fill);
                if response.has_focus() {
                    ui.painter().rect_stroke(
                        rect,
                        10.,
                        egui::Stroke::new(1.0_f32, accent),
                        egui::StrokeKind::Inside,
                    );
                }
                let inner = rect.shrink2(egui::vec2(8., 0.));
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .id_salt(("row", &track.id, index))
                        .max_rect(inner)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 10.;
                        let lead = egui::vec2(30., 48.);
                        let is_selected = self.selected.contains(&track.id);
                        ui.allocate_ui_with_layout(
                            lead,
                            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                // Ctrl+click or Ctrl+Enter starts a selection; checkboxes then stay visible.
                                if selecting || is_selected {
                                    let mut selected = is_selected;
                                    let check = ui.checkbox(&mut selected, "");
                                    check.widget_info(|| {
                                        egui::WidgetInfo::selected(
                                            egui::WidgetType::Checkbox,
                                            true,
                                            selected,
                                            format!("Select {}", track.title),
                                        )
                                    });
                                    if check.changed() {
                                        if selected {
                                            self.selected.push(track.id.clone());
                                        } else {
                                            self.selected.retain(|x| x != &track.id);
                                        }
                                    }
                                } else if pointer_inside || response.has_focus() {
                                    if Self::icon_button(
                                        ui,
                                        "▶",
                                        &format!("Play {}", track.title),
                                        current,
                                    )
                                    .on_hover_text("Ctrl+click a row to select")
                                    .clicked()
                                    {
                                        self.play(track);
                                    }
                                } else if current {
                                    ui.label(RichText::new("▶").color(accent));
                                } else {
                                    ui.label(
                                        RichText::new(format!("{}", index + 1))
                                            .monospace()
                                            .size(12.)
                                            .color(MUTED),
                                    );
                                }
                            },
                        );
                        self.artwork(ui, track.cover.as_deref(), 36., 1.);
                        let rest = ui.available_width();
                        let fixed = 150.;
                        // Reserve the album column on wide rows even when empty so time and actions line up.
                        let show_album = rest > 560.;
                        let album = if track.album != track.title {
                            track.album.as_str()
                        } else {
                            ""
                        };
                        let album_width = if show_album { rest * 0.28 } else { 0. };
                        let title_width = (rest - fixed - album_width).max(80.);
                        ui.allocate_ui_with_layout(
                            egui::vec2(title_width, 48.),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(title_width);
                                ui.spacing_mut().item_spacing.y = 1.;
                                ui.add_space(7.);
                                ui.add(
                                    egui::Label::new(RichText::new(&track.title).color(
                                        if current {
                                            accent
                                        } else {
                                            ui.visuals().text_color()
                                        },
                                    ))
                                    .truncate()
                                    .selectable(false),
                                );
                                let artist = if track.platform == "local" {
                                    format!("{} · Local file", track.artist)
                                } else if track.local_path.is_some() {
                                    format!("{} · Downloaded", track.artist)
                                } else {
                                    track.artist.clone()
                                };
                                ui.add(
                                    egui::Label::new(RichText::new(artist).small().color(MUTED))
                                        .truncate()
                                        .selectable(false),
                                );
                            },
                        );
                        if show_album {
                            ui.allocate_ui_with_layout(
                                egui::vec2(album_width, 48.),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.set_width(album_width);
                                    ui.add(
                                        egui::Label::new(RichText::new(album).small().color(MUTED))
                                            .truncate()
                                            .selectable(false),
                                    );
                                },
                            );
                        }
                        ui.label(
                            RichText::new(time(track.duration))
                                .monospace()
                                .size(12.)
                                .color(MUTED),
                        );
                        self.like_button(ui, track);
                        self.track_menu(ui, track, index, tracks.len(), reorder);
                    },
                );
            }
            if response.clicked() && ui.input(|i| i.modifiers.command) {
                if let Some(at) = self.selected.iter().position(|id| id == &track.id) {
                    self.selected.remove(at);
                } else {
                    self.selected.push(track.id.clone());
                }
            } else if response.clicked() && !current {
                self.play(track);
            }
            if ui.is_rect_visible(rect)
                && track.platform != "local"
                && track.local_path.is_none()
                && !current
            {
                first_visible.get_or_insert(track);
                if pointer_inside {
                    hovered = Some(track.clone());
                }
            }
        }
        if let Some(track) = hovered {
            let warm_delay = Duration::from_millis(80);
            if self
                .hover_candidate
                .as_ref()
                .is_none_or(|(old, _)| old.id != track.id)
            {
                self.hover_candidate = Some((track.clone(), Instant::now()));
            }
            let elapsed = self.hover_candidate.as_ref().unwrap().1.elapsed();
            if elapsed >= warm_delay
                && self.hover_warmed.as_deref() != Some(&track.id)
                && !self.job_tags.values().any(|tag| tag == "warm")
                && !self.player["pending_track_id"].is_string()
            {
                self.hover_warmed = Some(track.id.clone());
                self.send(
                    "warm",
                    "stream.prepare",
                    json!({"track":track,"preview_audio":true}),
                );
            } else if elapsed < warm_delay {
                ui.ctx().request_repaint_after(warm_delay - elapsed);
            }
        } else {
            self.hover_candidate = None;
            if let Some(track) = first_visible
                && self.first_visible_warmed.as_deref() != Some(&track.id)
                && self.hover_warmed.as_deref() != Some(&track.id)
                && !self.job_tags.values().any(|tag| tag == "warm")
                && !self.player["pending_track_id"].is_string()
            {
                self.first_visible_warmed = Some(track.id.clone());
                self.send(
                    "warm",
                    "stream.prepare",
                    json!({"track":track,"preview_audio":true}),
                );
            }
        }
    }
    /// Shared bulk actions once tracks are selected.
    fn selection_bar(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(SURFACE)
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(12, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{} selected", self.selected.len())).strong());
                    ui.add_space(8.);
                    let ids = self.selected.clone();
                    if ui.button("Add to queue").clicked() {
                        self.send("append", "queue.append", json!({"tracks":ids}));
                    }
                    ui.menu_button("Add to playlist", |ui| {
                        for playlist in self.library.playlists.values().cloned().collect::<Vec<_>>() {
                            if ui.button(&playlist.name).clicked() {
                                let mut value = playlist.clone();
                                value.tracks.extend(ids.iter().cloned());
                                self.send("playlist", "playlist.save", json!(value));
                                ui.close();
                            }
                        }
                    });
                    if ui.button("Download").clicked() {
                        self.send(
                            "download.bulk",
                            "download.bulk",
                            json!({"ids":ids,"format":self.library.settings.get("download_format").and_then(Value::as_str).unwrap_or("opus")}),
                        );
                    }
                    if ui.button("Clear").clicked() {
                        self.selected.clear();
                    }
                });
            });
        ui.add_space(4.);
    }
    fn empty_state(&mut self, ui: &mut egui::Ui) {
        ui.add_space(24.);
        if self.page == Page::Search && self.busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Searching…").size(18.));
            });
        } else if self.page == Page::Search && self.error.is_some() {
            ui.label(RichText::new("Couldn't load results").size(20.));
            if ui.button("Retry").clicked() {
                self.search_current();
            }
        } else if self.page == Page::Search && !self.query.trim().is_empty() {
            ui.label(RichText::new("No results").size(20.));
        } else {
            let message = match self.page {
                Page::Search => "Search for music",
                Page::Library => "Library is empty",
                Page::Favorites => "No favorites yet",
                Page::History => "Nothing played yet",
                Page::Offline => "Nothing downloaded",
                Page::Queue => "Queue is empty",
                Page::Playlists => "This playlist is empty",
                _ => "Nothing here yet",
            };
            ui.label(RichText::new(message).size(20.));
            if self.page == Page::Library {
                self.import_menu(ui);
            }
        }
    }
    fn import_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Import", |ui| {
            if ui.button("Audio files…").clicked() {
                self.import(false);
                ui.close();
            }
            if ui.button("Scan a folder…").clicked() {
                self.import(true);
                ui.close();
            }
        });
    }
    /// Album / artist / playlist header; returns true when Back was pressed.
    fn browse_header(&mut self, ui: &mut egui::Ui) -> bool {
        let back = !self.navigation.is_empty() && ui.small_button("← Back").clicked();
        let cover = self
            .current_browse
            .as_ref()
            .and_then(|c| c["cover"].as_str().map(str::to_owned))
            .or_else(|| self.tracks.iter().find_map(|t| t.cover.clone()));
        ui.horizontal(|ui| {
            self.artwork(ui, cover.as_deref(), 112., 1.);
            ui.vertical(|ui| {
                ui.add_space(10.);
                if let Some(kind) = self
                    .current_browse
                    .as_ref()
                    .and_then(|c| c["kind"].as_str())
                {
                    ui.label(RichText::new(kind).small().color(MUTED));
                }
                ui.add(
                    egui::Label::new(RichText::new(&self.browse_title).size(26.).strong())
                        .truncate(),
                );
                if !self.tracks.is_empty() {
                    ui.label(
                        RichText::new(format!("{} songs", self.tracks.len()))
                            .small()
                            .color(MUTED),
                    );
                }
                ui.horizontal(|ui| {
                    let accent = ui.visuals().selection.bg_fill;
                    let on_accent = ui.visuals().selection.stroke.color;
                    if !self.tracks.is_empty()
                        && ui
                            .add(
                                egui::Button::new(
                                    RichText::new("▶  Play").strong().color(on_accent),
                                )
                                .fill(accent)
                                .corner_radius(16),
                            )
                            .clicked()
                    {
                        self.send(
                            "play",
                            "mix.apply",
                            json!({"name":self.browse_title,"tracks":self.tracks,"play":true}),
                        );
                    }
                    if let Some(collection) = self.current_browse.clone()
                        && ui
                            .button("Save")
                            .on_hover_text("Save to your library")
                            .clicked()
                    {
                        self.send("saved", "collection.save", collection);
                    }
                });
            });
        });
        ui.add_space(8.);
        back
    }
    fn track_menu(
        &mut self,
        ui: &mut egui::Ui,
        track: &Track,
        index: usize,
        count: usize,
        reorder: bool,
    ) {
        ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        let menu = ui.menu_button(RichText::new("•••").size(14.).color(MUTED), |ui| {
            if reorder {
                if index > 0 && ui.button("Move up").clicked() {
                    self.move_track(index, index - 1);
                    ui.close();
                }
                if index + 1 < count && ui.button("Move down").clicked() {
                    self.move_track(index, index + 1);
                    ui.close();
                }
                ui.separator();
            }
            if ui.button("Start YouTube Music radio").clicked() {
                self.radio(track);
                ui.close();
            }
            if !track.artist.is_empty() && ui.button("Find artist").clicked() {
                self.query = track.artist.clone();
                self.platform = 0;
                self.search_filter = "artists".into();
                self.search();
                ui.close();
            }
            if !track.album.is_empty() && ui.button("Find album").clicked() {
                self.query = format!("{} {}", track.artist, track.album);
                self.platform = 0;
                self.search_filter = "albums".into();
                self.search();
                ui.close();
            }
            if track.platform == "spotify" && ui.button("Open in Spotify").clicked() {
                let _ = open::that(&track.source);
                ui.close();
            }
            if ui.button("Add to queue").clicked() {
                self.send("add", "library.add", json!({"track":track}));
                self.send("append", "queue.append", json!({"tracks":[track.id]}));
                ui.close();
            }
            if track.platform != "local" && ui.button("Download for offline").clicked() {
                self.send("add", "library.add", json!({"track":track}));
                self.job(
                    "download",
                    "download",
                    json!({"id":track.id,"format":self.library.settings.get("download_format").and_then(Value::as_str).unwrap_or("opus")}),
                );
                ui.close();
            }
            if track.local_path.is_some()
                && track.platform != "local"
                && ui.button("Delete downloaded copy").clicked()
            {
                self.send("offline", "offline.delete", json!({"id":track.id}));
                ui.close();
            }
            if self.page == Page::Playlists && ui.button("Remove from this playlist").clicked() {
                if let Some(mut list) = self
                    .active_playlist
                    .as_ref()
                    .and_then(|id| self.library.playlists.get(id))
                    .cloned()
                {
                    list.tracks.remove(index);
                    self.send("playlist", "playlist.save", json!(list));
                }
                ui.close();
            }
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
    /// Square artwork card with the title over the cover; returns the card rect and whether
    /// it was opened (clicked) or its Play button was pressed.
    fn cover_card(
        &self,
        ui: &mut egui::Ui,
        side: f32,
        cover: Option<&str>,
        title: &str,
        subtitle: &str,
        with_play: bool,
    ) -> (egui::Rect, bool, bool) {
        let (rect, open) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
        open.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Open {title}"))
        });
        if !ui.is_rect_visible(rect) {
            return (rect, open.clicked(), false);
        }
        let radius = 14.;
        if let Some(cover) = cover {
            // Centre-crop non-square thumbnails so every card stays square and never overlaps.
            let image = egui::Image::new(cover);
            let uv = image
                .load_for_size(ui.ctx(), rect.size())
                .ok()
                .and_then(|poll| poll.size())
                .map_or(
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                    |size| {
                        let aspect = size.x / size.y.max(1.);
                        let (x, y) = if aspect > 1. {
                            ((1. - 1. / aspect) / 2., 0.)
                        } else {
                            (0., (1. - aspect) / 2.)
                        };
                        egui::Rect::from_min_max(egui::pos2(x, y), egui::pos2(1. - x, 1. - y))
                    },
                );
            image
                .uv(uv)
                .fit_to_exact_size(rect.size())
                .maintain_aspect_ratio(false)
                .corner_radius(radius)
                .paint_at(ui, rect);
        } else {
            ui.painter()
                .rect_filled(rect, radius, IRIS.gamma_multiply(0.28));
        }
        // Twenty faint overlapping bands give a smooth bottom scrim that keeps white text readable.
        let bottom = egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: radius as u8,
            se: radius as u8,
        };
        for step in 0..20 {
            let top = rect.top() + rect.height() * (0.4 + step as f32 * 0.03);
            ui.painter().rect_filled(
                egui::Rect::from_min_max(egui::pos2(rect.left(), top), rect.max),
                bottom,
                Color32::from_black_alpha(11),
            );
        }
        if open.hovered() || open.has_focus() {
            ui.painter().rect_stroke(
                rect,
                radius,
                egui::Stroke::new(1.5_f32, IRIS.gamma_multiply(0.8)),
                egui::StrokeKind::Inside,
            );
        }
        let play_space = if with_play { 46. } else { 0. };
        let text = egui::Rect::from_min_max(
            egui::pos2(rect.left() + 14., rect.bottom() - 56.),
            egui::pos2(rect.right() - 12. - play_space, rect.bottom() - 10.),
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(text)
                .layout(egui::Layout::top_down(egui::Align::Min)),
            |ui| {
                ui.spacing_mut().item_spacing.y = 1.;
                ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .strong()
                            .size(16.)
                            .color(Color32::WHITE),
                    )
                    .truncate()
                    .selectable(false),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(subtitle)
                            .small()
                            .color(Color32::from_white_alpha(200)),
                    )
                    .truncate()
                    .selectable(false),
                );
            },
        );
        let mut play = false;
        if with_play {
            let button = egui::Rect::from_center_size(
                egui::pos2(rect.right() - 30., rect.bottom() - 32.),
                egui::vec2(36., 36.),
            );
            let response = ui.put(
                button,
                egui::Button::new(
                    RichText::new("▶")
                        .size(14.)
                        .color(ui.visuals().selection.stroke.color),
                )
                .fill(ui.visuals().selection.bg_fill)
                .corner_radius(18),
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Play {title}"))
            });
            play = response.on_hover_text(format!("Play {title}")).clicked();
        }
        (rect, open.clicked(), play)
    }
    /// Card side for `count` cards in the space left, keeping at most `max_columns` per row.
    fn card_layout(available: f32, count: usize, max_columns: usize) -> (usize, f32) {
        let gap = 14.;
        let columns = (((available + gap) / (150. + gap)).floor() as usize)
            .clamp(1, max_columns.max(1))
            .min(count.max(1));
        let side = ((available - gap * (columns as f32 - 1.)) / columns as f32).min(240.);
        (columns, side)
    }
    fn catalog(&mut self, ui: &mut egui::Ui) {
        let items: Vec<Value> = self.items.iter().take(12).cloned().collect();
        if items.is_empty() {
            return;
        }
        let (columns, side) = Self::card_layout(ui.available_width() - 12., items.len(), 6);
        for chunk in items.chunks(columns) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.;
                for item in chunk {
                    let (_, open, _) = self.cover_card(
                        ui,
                        side,
                        item["cover"].as_str(),
                        item["title"].as_str().unwrap_or("Collection"),
                        item["kind"].as_str().unwrap_or(""),
                        false,
                    );
                    if open {
                        self.navigation.push((
                            self.page,
                            self.tracks.clone(),
                            self.items.clone(),
                            self.browse_title.clone(),
                            self.current_browse.clone(),
                        ));
                        self.page = Page::Browse;
                        self.browse_title = item["title"].as_str().unwrap_or("").into();
                        self.busy = true;
                        self.send(
                            "browse",
                            "music.browse",
                            json!({"id":item["id"],"params":item["params"]}),
                        );
                    }
                }
            });
            ui.add_space(6.);
        }
        ui.add_space(10.);
    }
    /// Glow colour for a cover; decoded once per URI on a helper thread, never on the audio path.
    fn cover_glow(&self, ctx: &egui::Context, uri: &str) -> Option<Color32> {
        let mut tints = self.cover_tints.lock().ok()?;
        if let Some(tint) = tints.get(uri) {
            return *tint;
        }
        if let Ok(egui::load::BytesPoll::Ready { bytes, .. }) = ctx.try_load_bytes(uri) {
            tints.insert(uri.to_owned(), None);
            let (tints, uri, bytes, ctx) = (
                self.cover_tints.clone(),
                uri.to_owned(),
                bytes.to_vec(),
                ctx.clone(),
            );
            std::thread::spawn(move || {
                let tint = cover_tint(&bytes);
                if let Ok(mut map) = tints.lock() {
                    map.insert(uri, tint);
                }
                ctx.request_repaint();
            });
        }
        None
    }
    fn artwork(&self, ui: &mut egui::Ui, cover: Option<&str>, size: f32, opacity: f32) {
        if let Some(cover) = cover {
            ui.add(
                egui::Image::new(cover)
                    .fit_to_exact_size(egui::vec2(size, size))
                    .corner_radius((size / 7.) as u8)
                    .tint(Color32::WHITE.gamma_multiply(opacity)),
            );
        } else {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, size / 7., IRIS.gamma_multiply(0.28 * opacity));
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "R",
                egui::FontId::proportional(size * 0.45),
                TEXT.gamma_multiply(opacity),
            );
        }
    }
    fn player_bar(&mut self, ctx: &egui::Context) {
        let track = self.player["track_id"]
            .as_str()
            .and_then(|id| self.library.tracks.get(id))
            .cloned();
        let cover = track.as_ref().and_then(|t| t.cover.clone());
        let motion = if self.appearance.reduced_motion {
            0.
        } else {
            ARTWORK_FADE
        };
        let target = cover
            .as_deref()
            .and_then(|uri| self.cover_glow(ctx, uri))
            .unwrap_or(IRIS);
        let channel = |name: &str, value: u8| {
            ctx.animate_value_with_time(egui::Id::new(("player-glow", name)), value as f32, motion)
                as u8
        };
        let glow = Color32::from_rgb(
            channel("r", target.r()),
            channel("g", target.g()),
            channel("b", target.b()),
        );
        let id = self.player["track_id"].as_str().map(str::to_owned);
        if id != self.artwork_for {
            self.artwork_for = id;
            self.artwork_since = Instant::now();
        }
        let fade = if motion == 0. {
            1.
        } else {
            (self.artwork_since.elapsed().as_secs_f32() / motion).min(1.)
        };
        if fade < 1. {
            ctx.request_repaint();
        }
        let light = self.appearance.mode == "light";
        let base = if light {
            ctx.style().visuals.window_fill
        } else {
            SURFACE
        };
        let glass = base
            .lerp_to_gamma(glow, if track.is_some() { 0.1 } else { 0. })
            .gamma_multiply(0.94);
        let accent = ctx.style().visuals.selection.bg_fill;
        let on_accent = ctx.style().visuals.selection.stroke.color;
        egui::TopBottomPanel::bottom("player")
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(egui::Margin {
                        left: 14,
                        right: 14,
                        top: 10,
                        bottom: 10,
                    }),
            )
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(glass)
                    .stroke(egui::Stroke::new(
                        1.0_f32,
                        Color32::from_white_alpha(22).blend(AQUA.gamma_multiply(0.06)),
                    ))
                    .corner_radius(16)
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .shadow(egui::Shadow {
                        offset: [0, 4],
                        blur: 36,
                        spread: 0,
                        // No glow until something is loaded; idle bar is just surface and hairline.
                        color: glow.gamma_multiply(if track.is_none() {
                            0.
                        } else if light {
                            0.3
                        } else {
                            0.55
                        }),
                    })
                    .show(ui, |ui| {
                        ui.visuals_mut().widgets.inactive.bg_fill = Color32::from_white_alpha(34);
                        ui.visuals_mut().widgets.noninteractive.bg_fill =
                            Color32::from_white_alpha(22);
                        self.visualizer(ui);
                        let total = ui.available_width();
                        let left_width = (total * 0.27).clamp(190., 320.);
                        let right_width = (total * 0.38).clamp(410., 460.);
                        let center_width = (total - left_width - right_width).max(220.);
                        let height = 56.;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.;
                            self.now_playing(
                                ui,
                                track.as_ref(),
                                cover.as_deref(),
                                fade,
                                left_width,
                                height,
                            );
                            ui.allocate_ui_with_layout(
                                egui::vec2(center_width, height),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.set_width(center_width);
                                    ui.spacing_mut().item_spacing = egui::vec2(12., 2.);
                                    self.transport(ui, center_width, accent, on_accent);
                                    self.timeline(ui, center_width);
                                },
                            );
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), height),
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.spacing_mut().item_spacing.x = 8.;
                                    ui.spacing_mut().button_padding = egui::vec2(10., 5.);
                                    self.player_options(ui, track.as_ref());
                                },
                            );
                        });
                    });
            });
    }
    fn now_playing(
        &mut self,
        ui: &mut egui::Ui,
        track: Option<&Track>,
        cover: Option<&str>,
        fade: f32,
        width: f32,
        height: f32,
    ) {
        ui.allocate_ui_with_layout(
            egui::vec2(width, height),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_width(width);
                self.artwork(ui, cover, 48., fade);
                let text_width = (ui.available_width() - 34.).max(80.);
                ui.allocate_ui_with_layout(
                    egui::vec2(text_width, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(text_width);
                        ui.spacing_mut().item_spacing.y = 2.;
                        ui.add_space(6.);
                        let title = ui.add(
                            egui::Label::new(
                                RichText::new(
                                    track.map_or("Choose something to play", |t| t.title.as_str()),
                                )
                                .strong(),
                            )
                            .truncate(),
                        );
                        // Matched recording details stay one hover away instead of taking a whole row.
                        if let Some(source) = self.player["playback_source"]["title"].as_str() {
                            title.on_hover_text(format!(
                                "Audio: {} - {} (duration checked)",
                                source,
                                time(
                                    self.player["playback_source"]["duration"]
                                        .as_f64()
                                        .unwrap_or(0.)
                                )
                            ));
                        }
                        let pending = self.player["pending_track_id"].is_string()
                            || (self.player["track_id"].is_string()
                                && self.player["paused"] == false
                                && self.player["playing"] != true);
                        let subtitle = if pending {
                            playback_status(&self.player).to_owned()
                        } else {
                            track.map_or_else(|| "Nothing playing".into(), |t| t.artist.clone())
                        };
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.;
                            // Preparing audio gets its own spinner so it never reads as playing.
                            if pending {
                                ui.add(egui::Spinner::new().size(11.).color(AQUA));
                            }
                            ui.add(
                                egui::Label::new(RichText::new(subtitle).small().color(
                                    if pending {
                                        AQUA
                                    } else {
                                        ui.visuals().weak_text_color()
                                    },
                                ))
                                .truncate(),
                            );
                        });
                    },
                );
                if let Some(track) = track {
                    self.like_button(ui, track);
                }
            },
        );
    }
    /// Text tab; the active one is marked with an underline, not a filled pill.
    fn tab(ui: &mut egui::Ui, label: &str, active: bool, size: f32) -> egui::Response {
        let accent = ui.visuals().selection.bg_fill;
        let response = ui.add(
            egui::Label::new(RichText::new(label).size(size).strong().color(if active {
                TEXT
            } else {
                MUTED
            }))
            .sense(egui::Sense::click())
            .selectable(false),
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, label)
        });
        if active || response.has_focus() {
            let r = response.rect;
            ui.painter().line_segment(
                [
                    r.left_bottom() + egui::vec2(0., 3.),
                    r.right_bottom() + egui::vec2(0., 3.),
                ],
                egui::Stroke::new(2.0_f32, accent),
            );
        }
        response
    }
    /// Low icon button: transparent at rest, soft circle on hover, focus ring, Iris + dot when active.
    fn icon_button(ui: &mut egui::Ui, glyph: &str, label: &str, active: bool) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(34., 34.), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), active, label)
        });
        let accent = ui.visuals().selection.bg_fill;
        let painter = ui.painter();
        if response.is_pointer_button_down_on() {
            painter.circle_filled(rect.center(), 16., Color32::from_white_alpha(22));
        } else if response.hovered() {
            painter.circle_filled(rect.center(), 16., Color32::from_white_alpha(12));
        }
        if response.has_focus() {
            painter.circle_stroke(rect.center(), 16., egui::Stroke::new(1.5_f32, accent));
        }
        let color = if !ui.is_enabled() {
            MUTED.gamma_multiply(0.5)
        } else if active {
            accent
        } else if response.hovered() {
            TEXT
        } else {
            MUTED
        };
        painter.text(
            rect.center() - egui::vec2(0., if active { 2. } else { 0. }),
            egui::Align2::CENTER_CENTER,
            glyph,
            egui::FontId::proportional(17.),
            color,
        );
        // State is marked by shape as well as colour.
        if active {
            painter.circle_filled(rect.center_bottom() - egui::vec2(0., 5.), 2., accent);
        }
        response.on_hover_text(label)
    }
    /// On/off toggle without a visible label; `label` is the screen-reader name.
    fn toggle(ui: &mut egui::Ui, on: &mut bool, label: &str) -> egui::Response {
        let accent = ui.visuals().selection.bg_fill;
        let (rect, mut response) =
            ui.allocate_exact_size(egui::vec2(34., 20.), egui::Sense::click());
        if response.clicked() {
            *on = !*on;
            response.mark_changed();
        }
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, label)
        });
        let t = ui.ctx().animate_bool(response.id, *on);
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            10.,
            Color32::from_white_alpha(40).lerp_to_gamma(accent, t),
        );
        if response.has_focus() {
            painter.rect_stroke(
                rect.expand(2.),
                12.,
                egui::Stroke::new(1.5_f32, accent),
                egui::StrokeKind::Outside,
            );
        }
        let x = egui::lerp(rect.left() + 10.0..=rect.right() - 10., t);
        painter.circle_filled(egui::pos2(x, rect.center().y), 7., Color32::WHITE);
        response
    }
    /// Toggle followed by its visible label; the longer explanation goes in the tooltip.
    fn switch(ui: &mut egui::Ui, on: &mut bool, label: &str, hint: &str) -> egui::Response {
        ui.horizontal(|ui| {
            let response = Self::toggle(ui, on, label);
            ui.label(RichText::new(label).color(TEXT));
            response
        })
        .inner
        .on_hover_text(hint)
    }
    /// Menu row: check mark and a light fill for the active choice, never a full accent bar.
    fn menu_item(ui: &mut egui::Ui, checked: bool, label: &str) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(196., 34.), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, checked, label)
        });
        let accent = ui.visuals().selection.bg_fill;
        let painter = ui.painter();
        let fill = if response.hovered() || response.has_focus() {
            Color32::from_white_alpha(14)
        } else if checked {
            Color32::from_white_alpha(8)
        } else {
            Color32::TRANSPARENT
        };
        painter.rect_filled(rect, 8., fill);
        if response.has_focus() {
            painter.rect_stroke(
                rect,
                8.,
                egui::Stroke::new(1.0_f32, accent),
                egui::StrokeKind::Inside,
            );
        }
        if checked {
            painter.text(
                rect.left_center() + egui::vec2(12., 0.),
                egui::Align2::LEFT_CENTER,
                "✓",
                egui::FontId::proportional(15.),
                accent,
            );
        }
        painter.text(
            rect.left_center() + egui::vec2(34., 0.),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(14.),
            TEXT,
        );
        response
    }
    fn shuffle_button(&mut self, ui: &mut egui::Ui) {
        let current = if self.library.queue.shuffle {
            self.library
                .settings
                .get("shuffle_mode")
                .and_then(Value::as_str)
                .unwrap_or("classic")
                .to_owned()
        } else {
            "off".to_owned()
        };
        let label = match current.as_str() {
            "classic" => "Shuffle: Classic",
            "ordered" => "Shuffle: Balanced",
            "smart" => "Shuffle: Smart Shuffle",
            _ => "Shuffle: Off",
        };
        let button = Self::icon_button(ui, "🔀", label, current != "off");
        let popup = egui::Popup::menu(&button).id(ui.make_persistent_id("shuffle-menu"));
        popup.show(|ui| {
            ui.spacing_mut().item_spacing.y = 2.;
            for (mode, name) in [
                ("off", "Off"),
                ("classic", "Classic"),
                ("ordered", "Balanced"),
                ("smart", "Smart Shuffle"),
            ] {
                if Self::menu_item(ui, current == mode, name).clicked() {
                    if mode != current {
                        self.send("mode", "queue.mode", json!({"shuffle_mode":mode}));
                    }
                    ui.close();
                }
            }
        });
    }
    fn repeat_button(&mut self, ui: &mut egui::Ui) {
        use riff_core::model::Repeat;
        let repeat = self.library.queue.repeat;
        let (glyph, label, next) = match repeat {
            Repeat::Off => ("🔁", "Repeat: Off", Repeat::All),
            Repeat::All => ("🔁", "Repeat: All", Repeat::One),
            Repeat::One => ("🔂", "Repeat: One", Repeat::Off),
        };
        if Self::icon_button(ui, glyph, label, repeat != Repeat::Off).clicked() {
            self.send("mode", "queue.mode", json!({"repeat":next}));
        }
    }
    /// Shared Shuffle / Repeat / Autoplay cluster for the Up next panel and page.
    fn play_modes(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.;
            self.shuffle_button(ui);
            self.repeat_button(ui);
            ui.add_space(8.);
            self.autoplay_toggle(ui);
        });
    }
    fn transport(&mut self, ui: &mut egui::Ui, width: f32, accent: Color32, on_accent: Color32) {
        let paused = self.player["paused"].as_bool().unwrap_or(true);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.;
            // 4 icon buttons + Play + gaps; fixed so the controls never shift between states.
            ui.add_space(((width - 4. * 34. - 40. - 24.) / 2.).max(0.));
            self.shuffle_button(ui);
            if Self::icon_button(ui, "⏮", "Previous", false).clicked() {
                self.send("previous", "player.previous", json!({}));
            }
            let label = if paused { "Play" } else { "Pause" };
            let play = ui.add(
                egui::Button::new(
                    RichText::new(if paused { "▶" } else { "⏸" })
                        .size(16.)
                        .color(on_accent),
                )
                .fill(accent)
                .corner_radius(20)
                .min_size(egui::vec2(40., 40.)),
            );
            play.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
            if play.on_hover_text(label).clicked() {
                self.send("toggle", "player.toggle", json!({}));
            }
            if Self::icon_button(ui, "⏭", "Next", false).clicked() {
                self.send("next", "player.next", json!({}));
            }
            self.repeat_button(ui);
        });
    }
    fn timeline(&mut self, ui: &mut egui::Ui, width: f32) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.;
            let position = self.player["position"].as_f64().unwrap_or(0.);
            let duration = self.player["duration"].as_f64().unwrap_or(0.);
            let clock = |value: f64| {
                RichText::new(time(value))
                    .monospace()
                    .size(12.)
                    .color(MUTED)
            };
            ui.label(clock(position));
            let mut seek = position;
            let slider = (width - 104.).max(120.);
            ui.spacing_mut().slider_width = slider;
            let can_seek = duration > 0.
                && self.player["track_id"].is_string()
                && self.player["pending_track_id"].is_null();
            let response = ui
                .add_enabled_ui(can_seek, |ui| {
                    ui.add_sized(
                        [slider, 16.],
                        egui::Slider::new(&mut seek, 0.0..=duration.max(1.)).show_value(false),
                    )
                })
                .inner;
            if response.dragged() {
                self.seek_preview = seek;
            }
            if response.drag_stopped() || response.changed() && !response.dragged() {
                self.send("seek", "player.seek", json!({"seconds":seek}));
            }
            ui.label(clock(duration));
            if self.player["buffering"] == true {
                ui.spinner();
            }
        });
    }
    /// Laid out right to left: menu, panel toggles, volume, mute.
    fn player_options(&mut self, ui: &mut egui::Ui, track: Option<&Track>) {
        ui.spacing_mut().item_spacing.x = 4.;
        let more = Self::icon_button(ui, "•••", "More", false);
        egui::Popup::menu(&more)
            .id(ui.make_persistent_id("player-more"))
            .show(|ui| {
                ui.spacing_mut().item_spacing.y = 2.;
                if Self::menu_item(ui, false, "Stop playback").clicked() {
                    self.send("stop", "player.stop", json!({}));
                    ui.close();
                }
                if let Some(track) = track
                    && track.id.starts_with("youtube:")
                    && Self::menu_item(ui, false, "Start radio").clicked()
                {
                    self.radio(track);
                    ui.close();
                }
                if let Some(url) = self.player["playback_source"]["url"].as_str()
                    && Self::menu_item(ui, false, "Open recording").clicked()
                {
                    let _ = open::that(url);
                    ui.close();
                }
                if Self::menu_item(ui, false, "Open mini player").clicked() {
                    Self::set_mini(ui.ctx(), &mut self.compact, true);
                    ui.close();
                }
            });
        let wide = ui.ctx().content_rect().width() >= SIDE_PANEL_MIN_WINDOW;
        for (page, glyph, label) in [
            (Page::Lyrics, "🎤", "Lyrics"),
            (Page::Queue, "☰", "Up next"),
        ] {
            let showing = self.side_panel == Some(page) && (wide || self.narrow_open);
            if Self::icon_button(ui, glyph, label, showing).clicked() {
                self.toggle_side_panel(page, wide);
            }
        }
        ui.add_space(6.);
        let mut volume = self.effects.volume;
        ui.spacing_mut().slider_width = 84.;
        let slider = ui.add(egui::Slider::new(&mut volume, 0.0..=1.).show_value(false));
        slider.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, "Volume"));
        if slider
            .on_hover_text(format!("Volume {:.0}%", volume * 100.))
            .changed()
        {
            self.effects.volume = volume;
            self.send_effects();
        }
        let muted = self.effects.muted;
        if Self::icon_button(
            ui,
            if muted { "🔇" } else { "🔊" },
            if muted { "Unmute" } else { "Mute" },
            muted,
        )
        .clicked()
        {
            self.effects.muted = !muted;
            self.send_effects();
        }
    }
    fn set_mini(ctx: &egui::Context, compact: &mut bool, on: bool) {
        *compact = on;
        let (min, size) = if on {
            (egui::vec2(380., 150.), egui::vec2(480., 180.))
        } else {
            (egui::vec2(1000., 600.), egui::vec2(1120., 700.))
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(min));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
    }
    fn toggle_side_panel(&mut self, page: Page, wide: bool) {
        let showing = self.side_panel == Some(page) && (wide || self.narrow_open);
        if showing {
            if wide {
                self.side_panel = None;
            }
            self.narrow_open = false;
        } else {
            self.side_panel = Some(page);
            self.narrow_open = !wide;
        }
    }
    fn like_button(&mut self, ui: &mut egui::Ui, track: &Track) {
        let liked = self.library.favorites.contains(&track.id);
        let accent = ui.visuals().selection.bg_fill;
        let heart = ui.add(
            egui::Button::new(
                RichText::new(if liked { "♥" } else { "♡" })
                    .size(18.)
                    .color(if liked { accent } else { MUTED }),
            )
            .frame(false),
        );
        heart.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, liked, "Like")
        });
        if heart
            .on_hover_text(if liked { "Liked" } else { "Like" })
            .clicked()
        {
            self.send("add", "library.add", json!({"track":track}));
            self.send(
                "favorite",
                "favorites.set",
                json!({"id":track.id,"enabled":!liked}),
            );
        }
    }
    /// Up next / Lyrics beside the content; slides in over `animation_time`, instant with reduced motion.
    fn side_panel(&mut self, ctx: &egui::Context) {
        let wide = ctx.content_rect().width() >= SIDE_PANEL_MIN_WINDOW;
        let open = self.side_panel.is_some() && (wide || self.narrow_open);
        let page = self.side_panel.unwrap_or(Page::Queue);
        egui::SidePanel::right("now-panel")
            .exact_width(280.)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(egui::Margin::symmetric(16, 14))
                    .stroke(egui::Stroke::new(1.0_f32, Color32::from_white_alpha(10))),
            )
            .show_animated(ctx, open, |ui| {
                ui.horizontal(|ui| {
                    for (tab, label) in [(Page::Queue, "Up next"), (Page::Lyrics, "Lyrics")] {
                        if Self::tab(ui, label, page == tab, 18.).clicked() {
                            self.side_panel = Some(tab);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let close = Self::icon_button(ui, "×", "Close panel", false);
                        if close.clicked() {
                            self.toggle_side_panel(page, wide);
                        }
                    });
                });
                ui.add_space(4.);
                if page == Page::Lyrics {
                    if ui.small_button("Lyric source & translation").clicked() {
                        self.page = Page::Lyrics;
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("panel-lyrics")
                        .auto_shrink(false)
                        .show(ui, |ui| self.lyric_view(ui, 19., 15.));
                } else {
                    self.play_modes(ui);
                    ui.add_space(4.);
                    self.queue_list(ui);
                }
            });
    }
    fn queue_list(&mut self, ui: &mut egui::Ui) {
        let tracks: Vec<Track> = self
            .library
            .queue
            .tracks
            .iter()
            .filter_map(|id| self.library.tracks.get(id).cloned())
            .collect();
        if tracks.is_empty() {
            ui.add_space(12.);
            ui.label(
                RichText::new("Nothing queued. Play a song or a mix to fill Up next.").color(MUTED),
            );
            return;
        }
        let accent = ui.visuals().selection.bg_fill;
        egui::ScrollArea::vertical()
            .id_salt("panel-queue")
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.;
                let playing = tracks.iter().position(|t| self.player["track_id"] == t.id);
                let next = playing.map_or(0, |i| i + 1);
                for (index, track) in tracks.iter().enumerate() {
                    let current = playing == Some(index);
                    let heading = if current {
                        Some("Now playing")
                    } else if index == next {
                        Some("Next")
                    } else {
                        None
                    };
                    if let Some(heading) = heading {
                        ui.add_space(4.);
                        ui.label(RichText::new(heading).small().strong().color(MUTED));
                    }
                    let row = egui::Frame::new()
                        .fill(if current {
                            accent.gamma_multiply(0.16)
                        } else {
                            Color32::TRANSPARENT
                        })
                        .corner_radius(10)
                        .inner_margin(6.)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                self.artwork(ui, track.cover.as_deref(), 40., 1.);
                                let width = (ui.available_width() - 44.).max(60.);
                                ui.allocate_ui_with_layout(
                                    egui::vec2(width, 40.),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.set_width(width);
                                        ui.spacing_mut().item_spacing.y = 1.;
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&track.title).color(if current {
                                                    accent
                                                } else {
                                                    TEXT
                                                }),
                                            )
                                            .truncate()
                                            .selectable(false),
                                        );
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&track.artist).small().color(MUTED),
                                            )
                                            .truncate()
                                            .selectable(false),
                                        );
                                    },
                                );
                                ui.label(
                                    RichText::new(time(track.duration))
                                        .monospace()
                                        .size(12.)
                                        .color(MUTED),
                                );
                            });
                        });
                    let response = row.response.interact(egui::Sense::click());
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            format!("Play {} by {} [{}]", track.title, track.artist, track.id),
                        )
                    });
                    if response.hovered() && !current {
                        ui.painter()
                            .rect_filled(response.rect, 10., Color32::from_white_alpha(6));
                    }
                    if response.clicked() && !current {
                        self.play(track);
                    }
                }
            });
    }
    fn lyric_view(&mut self, ui: &mut egui::Ui, active_size: f32, line_size: f32) {
        if ui.ui_contains_pointer() && ui.input(|i| i.raw_scroll_delta.y != 0.) {
            self.follow_lyrics = false;
        }
        if self.lyric_lines.is_empty() {
            ui.label(if self.lyric_text.is_empty() {
                "No lyrics"
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
                let font = egui::FontId::proportional(if active { active_size } else { line_size });
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
                        let sung =
                            active && now >= word.time && word.end.is_none_or(|end| now < end);
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
                let response = ui.add(egui::Label::new(layout).wrap().sense(egui::Sense::click()));
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
    fn navigation(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("navigation")
            .exact_width(190.)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(egui::Margin::symmetric(12, 18)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(8.);
                    ui.label(
                        RichText::new("ROYNIT")
                            .size(21.)
                            .strong()
                            .extra_letter_spacing(2.),
                    );
                    ui.label(RichText::new("Music").size(19.).color(MUTED));
                });
                ui.add_space(18.);
                egui::ScrollArea::vertical()
                    .id_salt("navigation-list")
                    .max_height((ui.available_height() - 80.).max(120.))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 4.;
                        for (page, icon, label) in [
                            (Page::Home, "🏠", "Listen now"),
                            (Page::Search, "🔍", "Search"),
                            (Page::Library, "📚", "Your library"),
                            (Page::History, "🕘", "Recently played"),
                            (Page::Offline, "💾", "Offline music"),
                            (Page::Favorites, "♡", "Favorites"),
                            (Page::Playlists, "🎵", "Playlists"),
                            (Page::Queue, "☰", "Up next"),
                            (Page::Lyrics, "🎤", "Lyrics"),
                            (Page::Downloads, "⬇", "Downloads"),
                        ] {
                            if self.nav_item(ui, icon, label, self.page == page).clicked() {
                                self.live_search_at = None;
                                self.page = page;
                                match page {
                                    Page::Home => self.send("home", "music.home", json!({})),
                                    Page::Settings => self.open_settings(),
                                    Page::Search => ui.ctx().memory_mut(|m| {
                                        m.request_focus(egui::Id::new("search-query"))
                                    }),
                                    _ => {}
                                }
                            }
                        }
                    });
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(8.);
                        ui.label(
                            RichText::new(format!(
                                "{} tracks in your library",
                                self.library.tracks.len()
                            ))
                            .small()
                            .color(MUTED),
                        );
                    });
                    // Settings stays pinned so it never scrolls out of view on short windows.
                    if self
                        .nav_item(ui, "⚙", "Settings", self.page == Page::Settings)
                        .clicked()
                    {
                        self.live_search_at = None;
                        self.open_settings();
                    }
                });
            });
    }
    /// Left-aligned icon + label row; egui buttons centre their text, which the mockup does not.
    fn nav_item(
        &self,
        ui: &mut egui::Ui,
        icon: &str,
        label: &str,
        selected: bool,
    ) -> egui::Response {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
        });
        let accent = ui.visuals().selection.bg_fill;
        let fill = if selected {
            accent.gamma_multiply(0.2)
        } else if response.hovered() {
            Color32::from_white_alpha(8)
        } else {
            Color32::TRANSPARENT
        };
        let painter = ui.painter();
        painter.rect_filled(rect, 10., fill);
        if response.has_focus() {
            painter.rect_stroke(
                rect,
                10.,
                egui::Stroke::new(1.0_f32, accent),
                egui::StrokeKind::Inside,
            );
        }
        let text = ui.visuals().text_color();
        painter.text(
            rect.left_center() + egui::vec2(14., 0.),
            egui::Align2::LEFT_CENTER,
            icon,
            egui::FontId::proportional(16.),
            if selected { accent } else { MUTED },
        );
        painter.text(
            rect.left_center() + egui::vec2(44., 0.),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(15.),
            text,
        );
        response
    }
    fn search_bar(&mut self, ctx: &egui::Context) {
        // Match modifiers on the key event itself; a fast Ctrl+K can release Ctrl within the same frame.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::K)) {
            ctx.memory_mut(|m| m.request_focus(egui::Id::new("search-query")));
        }
        egui::TopBottomPanel::top("search_bar")
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(egui::Margin {
                        left: 12,
                        right: 16,
                        top: 14,
                        bottom: 8,
                    }),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let field_width = (ui.available_width() - 190.).clamp(200., 720.);
                    egui::Frame::new()
                        .fill(ui.visuals().extreme_bg_color)
                        .stroke(egui::Stroke::new(1.0_f32, Color32::from_white_alpha(16)))
                        .corner_radius(12)
                        .inner_margin(egui::Margin::symmetric(6, 2))
                        .show(ui, |ui| {
                            ui.set_width(field_width);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.;
                                if Self::icon_button(ui, "🔍", "Search", false).clicked() {
                                    self.search();
                                }
                                // Reserve the trailing slots up front so spinner/clear never move the text.
                                let trailing = if field_width > 360. { 120. } else { 70. };
                                let response = ui.add(
                                    egui::TextEdit::singleline(&mut self.query)
                                        .id(egui::Id::new("search-query"))
                                        .frame(false)
                                        .desired_width((ui.available_width() - trailing).max(60.))
                                        .hint_text("Search music"),
                                );
                                if response.changed() {
                                    self.schedule_live_search();
                                }
                                if response.lost_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                {
                                    self.search();
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if field_width > 360. {
                                            ui.label(RichText::new("Ctrl+K").small().color(MUTED));
                                        }
                                        if !self.query.is_empty()
                                            && Self::icon_button(ui, "×", "Clear search", false)
                                                .clicked()
                                        {
                                            self.query.clear();
                                            self.live_search_at = None;
                                            ui.ctx().memory_mut(|m| {
                                                m.request_focus(egui::Id::new("search-query"))
                                            });
                                        }
                                        if self.busy && self.page == Page::Search {
                                            ui.add(egui::Spinner::new().size(14.).color(MUTED));
                                        }
                                    },
                                );
                            });
                        });
                    let old_source = self.platform;
                    egui::ComboBox::from_id_salt("Source")
                        .selected_text(
                            ["YouTube Music", "YouTube", "SoundCloud", "Spotify"][self.platform],
                        )
                        .show_ui(ui, |ui| {
                            for (i, name) in ["YouTube Music", "YouTube", "SoundCloud", "Spotify"]
                                .iter()
                                .enumerate()
                            {
                                ui.selectable_value(&mut self.platform, i, *name);
                            }
                        })
                        .response
                        .on_hover_text(if self.platform == 3 {
                            "Source · Spotify plays a matched YouTube Music recording"
                        } else {
                            "Source"
                        });
                    if self.page == Page::Search && self.platform != old_source {
                        self.schedule_live_search();
                    }
                });
                if self.platform == 0 && self.page == Page::Search {
                    ui.add_space(4.);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 18.;
                        for (kind, label) in [
                            ("songs", "Songs"),
                            ("albums", "Albums"),
                            ("artists", "Artists"),
                            ("playlists", "Playlists"),
                        ] {
                            if Self::tab(ui, label, self.search_filter == kind, 14.).clicked()
                                && self.search_filter != kind
                            {
                                self.search_filter = kind.into();
                                self.schedule_live_search();
                            }
                        }
                    });
                    ui.add_space(4.);
                }
                if let Some(deadline) = self.live_search_at {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        self.search();
                    } else {
                        ui.ctx().request_repaint_after(remaining);
                    }
                }
            });
    }
    fn send_effects(&mut self) {
        self.send("effects", "player.effects", json!(self.effects));
    }
    fn open_settings(&mut self) {
        self.page = Page::Settings;
        if self.settings_panel_stash.is_none() {
            self.settings_panel_stash = Some(self.side_panel.take());
        }
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
    fn matches(query: &str, texts: &[&str]) -> bool {
        let query = query.trim().to_lowercase();
        query.is_empty() || texts.iter().any(|t| t.to_lowercase().contains(&query))
    }
    /// One setting per row: name and an optional one-line hint on the left, the control
    /// right-aligned. Controls are added right to left. Narrow widths stack the control below.
    fn row(
        ui: &mut egui::Ui,
        query: &str,
        label: &str,
        hint: &str,
        keys: &str,
        control: impl FnOnce(&mut egui::Ui),
    ) {
        if !Self::matches(query, &[label, hint, keys]) {
            return;
        }
        ui.add_space(8.);
        if ui.available_width() < 520. {
            ui.label(RichText::new(label).color(TEXT));
            if !hint.is_empty() {
                ui.label(RichText::new(hint).small().color(MUTED));
            }
            ui.horizontal_wrapped(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
            });
        } else {
            ui.horizontal(|ui| {
                let left = (ui.available_width() * 0.48).max(220.);
                ui.allocate_ui_with_layout(
                    egui::vec2(left, 0.),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(left);
                        ui.label(RichText::new(label).color(TEXT));
                        if !hint.is_empty() {
                            ui.add(
                                egui::Label::new(RichText::new(hint).small().color(MUTED)).wrap(),
                            );
                        }
                    },
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
            });
        }
        ui.add_space(8.);
        let r = ui.available_rect_before_wrap();
        ui.painter().hline(
            r.x_range(),
            r.top(),
            egui::Stroke::new(1.0_f32, Color32::from_white_alpha(10)),
        );
    }
    fn group_title(ui: &mut egui::Ui, title: &str) {
        ui.add_space(18.);
        ui.label(RichText::new(title).size(20.).strong());
        ui.add_space(2.);
    }
    fn settings_page(&mut self, ui: &mut egui::Ui) {
        const GROUPS: [(&str, &str, &str); 7] = [
            (
                "▶",
                "Playback",
                "output device speaker headphones sleep timer autoplay crossfade normalize volume media keys tray mini player fullscreen lyrics yt-dlp audio tools",
            ),
            (
                "⬇",
                "Downloads & storage",
                "download format opus mp3 flac m4a wav cache limit clear storage disk folder",
            ),
            (
                "🎨",
                "Appearance",
                "theme dark light accent colour color text size font reduce motion animation visualizer",
            ),
            (
                "👤",
                "Accounts",
                "google youtube spotify cookie sign in login oauth client discord last.fm lastfm scrobble account",
            ),
            (
                "🌐",
                "Network",
                "proxy socks http connection test browser cookies",
            ),
            (
                "★",
                "Extensions",
                "themes plugins import export native warnings",
            ),
            (
                "💾",
                "Data & updates",
                "backup restore import export riff rfacc data folder diagnostics report install update version release rollback",
            ),
        ];
        let narrow = ui.available_width() < 760.;
        let body = |this: &mut Self, ui: &mut egui::Ui| {
            let query = this.settings_query.clone();
            let mut shown = 0;
            for (index, (_, name, keys)) in GROUPS.iter().enumerate() {
                let visible = if query.trim().is_empty() {
                    index == this.settings_group
                } else {
                    Self::matches(&query, &[name, keys])
                };
                if !visible {
                    continue;
                }
                shown += 1;
                Self::group_title(ui, name);
                let rows = if Self::matches(&query, &[name]) {
                    String::new()
                } else {
                    query.clone()
                };
                let q = rows.as_str();
                ui.push_id(name, |ui| match index {
                    0 => this.settings_playback(ui, q),
                    1 => this.settings_storage(ui, q),
                    2 => this.settings_appearance(ui, q),
                    3 => this.settings_accounts(ui, q),
                    4 => this.settings_network(ui, q),
                    5 => this.settings_extensions(ui, q),
                    _ => this.settings_data(ui, q),
                });
            }
            if shown == 0 {
                ui.add_space(24.);
                ui.label(RichText::new("No matching settings").size(18.));
            }
        };
        let search = |this: &mut Self, ui: &mut egui::Ui, width: f32| {
            ui.add(
                egui::TextEdit::singleline(&mut this.settings_query)
                    .hint_text("Search settings")
                    .desired_width(width),
            )
            .on_hover_text("Filter by name, e.g. proxy, cache, theme");
        };
        if narrow {
            ui.set_max_width(ui.available_width() - 28.);
            search(self, ui, ui.available_width());
            egui::ComboBox::from_id_salt("settings-group")
                .selected_text(GROUPS[self.settings_group].1)
                .show_ui(ui, |ui| {
                    for (index, (_, name, _)) in GROUPS.iter().enumerate() {
                        ui.selectable_value(&mut self.settings_group, index, *name);
                    }
                });
            body(self, ui);
        } else {
            let column = (ui.available_width() - 200. - 20. - 28.).clamp(320., 720.);
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(200., 0.),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(200.);
                        search(self, ui, 190.);
                        ui.add_space(10.);
                        ui.spacing_mut().item_spacing.y = 2.;
                        for (index, (icon, name, _)) in GROUPS.iter().enumerate() {
                            let active = self.settings_query.trim().is_empty()
                                && self.settings_group == index;
                            if self.nav_item(ui, icon, name, active).clicked() {
                                self.settings_group = index;
                                self.settings_query.clear();
                            }
                        }
                    },
                );
                ui.add_space(20.);
                ui.vertical(|ui| {
                    ui.set_width(column);
                    body(self, ui);
                });
            });
        }
    }
    fn settings_playback(&mut self, ui: &mut egui::Ui, q: &str) {
        let saved_device = self
            .library
            .settings
            .get("audio_device")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        Self::row(ui, q, "Output", "", "device speaker headphones", |ui| {
            if Self::icon_button(ui, "🔄", "Refresh devices", false).clicked() {
                self.send("devices", "player.devices", json!({}));
            }
            // Apply only appears when the choice differs from what is in use.
            if self.selected_device != saved_device && ui.button("Apply").clicked() {
                self.send(
                    "device",
                    "player.device",
                    json!({"name":if self.selected_device.is_empty(){None}else{Some(&self.selected_device)}}),
                );
            }
            egui::ComboBox::from_id_salt("output-device")
                .selected_text(if self.selected_device.is_empty() {
                    "System default"
                } else {
                    &self.selected_device
                })
                .width(220.)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.selected_device, String::new(), "System default");
                    for name in self.devices.clone() {
                        ui.selectable_value(&mut self.selected_device, name.clone(), name);
                    }
                });
        });
        let remaining = self.player["sleep_remaining"].as_u64();
        Self::row(ui, q, "Sleep timer", "", "stop pause", |ui| {
            if let Some(seconds) = remaining {
                if ui.button("Cancel").clicked() {
                    self.send("sleep", "player.sleep", json!({"seconds":0}));
                }
                ui.label(
                    RichText::new(format!("Stops in {}", time(seconds as f64)))
                        .monospace()
                        .color(MUTED),
                );
            } else {
                if ui.button("Start").clicked() {
                    self.send(
                        "sleep",
                        "player.sleep",
                        json!({"seconds":self.sleep_minutes*60}),
                    );
                }
                ui.add(
                    egui::DragValue::new(&mut self.sleep_minutes)
                        .range(1..=720)
                        .suffix(" min"),
                );
            }
        });
        let mut autoplay = self.library.settings.get("autoplay") != Some(&json!(false));
        Self::row(
            ui,
            q,
            "Autoplay",
            "Add similar songs when the queue ends",
            "radio",
            |ui| {
                if Self::toggle(ui, &mut autoplay, "Autoplay").changed() {
                    self.send("preferences", "settings.set", json!({"autoplay":autoplay}));
                }
            },
        );
        Self::row(ui, q, "Crossfade", "", "fade transition gap", |ui| {
            if ui
                .add(
                    egui::Slider::new(&mut self.effects.crossfade_seconds, 0.0..=12.)
                        .suffix(" s")
                        .step_by(0.5),
                )
                .changed()
            {
                self.effects_pending = Some(Instant::now());
            }
        });
        Self::row(
            ui,
            q,
            "Normalize volume",
            "Keep loudness similar between songs",
            "loudness gain",
            |ui| {
                if Self::toggle(ui, &mut self.effects.normalize, "Normalize volume").changed() {
                    self.effects_pending = Some(Instant::now());
                }
            },
        );
        let mut media_enabled = self.media_keys.is_some();
        Self::row(
            ui,
            q,
            "Media keys",
            "Use keyboard play, pause and skip keys anywhere in Windows",
            "global hotkeys",
            |ui| {
                if Self::toggle(ui, &mut media_enabled, "Media keys").changed() {
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
            },
        );
        if self._tray.is_some() {
            Self::row(
                ui,
                q,
                "Play in tray",
                "Keep playing when the window closes",
                "system tray background",
                |ui| {
                    if Self::toggle(ui, &mut self.close_to_tray, "Play in tray").changed() {
                        self.send(
                            "preferences",
                            "settings.set",
                            json!({"close_to_tray":self.close_to_tray}),
                        );
                    }
                },
            );
        }
        Self::row(ui, q, "Mini player", "", "compact small window", |ui| {
            if ui.button("Open").clicked() {
                Self::set_mini(ui.ctx(), &mut self.compact, true);
            }
        });
        Self::row(ui, q, "Fullscreen lyrics", "", "karaoke", |ui| {
            if ui.button("Open").clicked() {
                self.page = Page::Lyrics;
                self.fullscreen = true;
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
            }
        });
        Self::row(
            ui,
            q,
            "Audio tools",
            "yt-dlp is used when a direct stream is unavailable",
            "yt-dlp ffmpeg install",
            |ui| {
                if ui.button("Install or update").clicked() {
                    self.job("tools", "tools.install", json!({"name":"yt-dlp"}));
                }
                if ui.button("Check").clicked() {
                    self.send("tools", "tools.status", json!({}));
                }
            },
        );
    }
    fn settings_storage(&mut self, ui: &mut egui::Ui, q: &str) {
        let mut format = self
            .library
            .settings
            .get("download_format")
            .and_then(Value::as_str)
            .unwrap_or("opus")
            .to_owned();
        Self::row(
            ui,
            q,
            "Download format",
            "",
            "opus mp3 flac m4a wav audio",
            |ui| {
                let before = format.clone();
                egui::ComboBox::from_id_salt("download-format")
                    .selected_text(&format)
                    .show_ui(ui, |ui| {
                        for value in ["opus", "mp3", "flac", "m4a", "wav"] {
                            ui.selectable_value(&mut format, value.into(), value);
                        }
                    });
                if format != before {
                    self.send(
                        "preferences",
                        "settings.set",
                        json!({"download_format":format}),
                    );
                }
            },
        );
        let saved_limit = self
            .library
            .settings
            .get("cache_limit_mb")
            .and_then(Value::as_u64)
            .unwrap_or(1024);
        Self::row(ui, q, "Cache limit", "", "storage disk size", |ui| {
            if self.cache_limit_mb != saved_limit && ui.button("Save").clicked() {
                self.send("cache", "cache.limit", json!({"mb":self.cache_limit_mb}));
            }
            ui.add(
                egui::DragValue::new(&mut self.cache_limit_mb)
                    .range(64..=65536)
                    .speed(16)
                    .suffix(" MB"),
            );
        });
        let used = self.cache_info["bytes"].as_u64().unwrap_or(0) as f64 / 1_048_576.;
        Self::row(
            ui,
            q,
            "Cache used",
            "Audio that is playing is never removed",
            "clear storage",
            |ui| {
                if ui.button("Clear cache").clicked() {
                    self.send("cache", "cache.clear", json!({}));
                }
                if Self::icon_button(ui, "🔄", "Refresh cache size", false).clicked() {
                    self.send("cache", "cache.status", json!({}));
                }
                ui.label(
                    RichText::new(format!("{used:.1} MB"))
                        .monospace()
                        .color(MUTED),
                );
            },
        );
        Self::row(ui, q, "Downloads folder", "", "offline files", |ui| {
            if ui.button("Open").clicked() {
                let _ = open::that(self.data_root.join("downloads"));
            }
        });
    }
    fn settings_appearance(&mut self, ui: &mut egui::Ui, q: &str) {
        Self::row(ui, q, "Theme", "", "dark light mode", |ui| {
            egui::ComboBox::from_id_salt("theme-mode")
                .selected_text(if self.appearance.mode == "light" {
                    "Light"
                } else {
                    "Dark"
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.appearance.mode, "dark".into(), "Dark");
                    ui.selectable_value(&mut self.appearance.mode, "light".into(), "Light");
                });
        });
        Self::row(ui, q, "Accent", "", "colour color iris", |ui| {
            ui.color_edit_button_srgb(&mut self.appearance.accent);
            let [r, g, b] = self.appearance.accent;
            ui.label(
                RichText::new(format!("#{r:02X}{g:02X}{b:02X}"))
                    .monospace()
                    .color(MUTED),
            );
        });
        Self::row(ui, q, "Text size", "", "font scale", |ui| {
            ui.add(egui::Slider::new(&mut self.appearance.font_size, 12.0..=24.).suffix(" pt"));
        });
        Self::row(
            ui,
            q,
            "Reduce motion",
            "Change states instantly instead of animating",
            "animation",
            |ui| {
                Self::toggle(ui, &mut self.appearance.reduced_motion, "Reduce motion");
            },
        );
        Self::row(ui, q, "Visualizer", "", "bars waveform spectrum", |ui| {
            egui::ComboBox::from_id_salt("visualizer")
                .selected_text(match self.appearance.visualizer.as_str() {
                    "bars" => "Bars",
                    "waveform" => "Waveform",
                    _ => "Off",
                })
                .show_ui(ui, |ui| {
                    for (value, label) in
                        [("off", "Off"), ("bars", "Bars"), ("waveform", "Waveform")]
                    {
                        ui.selectable_value(&mut self.appearance.visualizer, value.into(), label);
                    }
                });
        });
        // Appearance previews live; Save persists it, and only shows while there are changes.
        let saved = self
            .library
            .settings
            .get("appearance")
            .cloned()
            .and_then(|v| riff_core::appearance::Appearance::from_tokens(v).ok())
            .unwrap_or_default();
        if saved != self.appearance && Self::matches(q, &["appearance save revert"]) {
            ui.add_space(8.);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Unsaved changes").color(MUTED));
                if ui.button("Save").clicked() {
                    self.send("appearance", "appearance.set", json!(self.appearance));
                }
                if ui.button("Revert").clicked() {
                    self.appearance = saved;
                }
            });
        }
    }
    fn account_row(&mut self, ui: &mut egui::Ui, q: &str, provider: usize) {
        let (key, name, hint) = if provider == 0 {
            (
                "google",
                "Google / YouTube",
                "Likes and your playlists shape Mixes",
            )
        } else {
            ("spotify", "Spotify", "Import liked songs and playlists")
        };
        let status = if self.account_status["provider"] == key {
            if self.account_status["signed_in"] == true {
                "Connected"
            } else if self.account_status["pending"] == true {
                "Waiting for sign-in"
            } else {
                "Not connected"
            }
        } else {
            "Not checked"
        };
        Self::row(ui, q, name, hint, "account sign in login oauth", |ui| {
            ui.menu_button("•••", |ui| {
                if ui.button("Check connection").clicked() {
                    self.send("account-status", "oauth.status", json!({"provider":key}));
                    ui.close();
                }
                if ui.button("Refresh session").clicked() {
                    self.send("auth", "oauth.refresh", json!({"provider":key}));
                    ui.close();
                }
                if provider == 0 {
                    if ui.button("Import YouTube likes").clicked() {
                        self.send("import", "google.import", json!({"kind":"likes"}));
                        ui.close();
                    }
                    if ui.button("Load my playlists").clicked() {
                        self.job(
                            "google-playlists",
                            "google.library",
                            json!({"kind":"playlists"}),
                        );
                        ui.close();
                    }
                } else {
                    if ui.button("Import liked songs").clicked() {
                        self.send("import", "spotify.import", json!({"kind":"liked"}));
                        ui.close();
                    }
                    if ui.button("Load my playlists").clicked() {
                        self.send(
                            "spotify-playlists",
                            "spotify.library",
                            json!({"kind":"playlists"}),
                        );
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Disconnect").clicked() {
                    self.retry_login = false;
                    self.login_generation = self.login_generation.wrapping_add(1);
                    if let Some(cancel) = &self.login_cancel {
                        cancel.store(true, Ordering::Relaxed);
                    }
                    self.send("logout", "oauth.logout", json!({"provider":key}));
                    ui.close();
                }
            });
            if status != "Connected"
                && ui
                    .button("Connect")
                    .on_hover_text("Uses the client ID under Advanced")
                    .clicked()
            {
                self.oauth_provider = provider;
                self.login();
            }
            ui.label(
                RichText::new(status)
                    .small()
                    .color(if status == "Connected" { AQUA } else { MUTED }),
            );
        });
    }
    fn settings_accounts(&mut self, ui: &mut egui::Ui, q: &str) {
        Self::row(
            ui,
            q,
            "YouTube Music session",
            "Optional cookie file so YouTube can use your session",
            "cookie netscape youtube music",
            |ui| {
                if ui.button("Remove").clicked() {
                    self.send("cookies-cleared", "cookies.clear", json!({}));
                }
                if ui.button("Choose file…").clicked()
                    && let Some(path) = rfd::FileDialog::new().pick_file()
                {
                    self.send("cookies", "cookies.set", json!({"path":path}));
                    self.status = "YouTube cookie file configured for this session.".into();
                }
            },
        );
        self.account_row(ui, q, 0);
        for playlist in self.google_playlists.clone() {
            let name = playlist["snippet"]["title"]
                .as_str()
                .unwrap_or("YouTube playlist")
                .to_owned();
            Self::row(ui, q, &name, "", "youtube playlist import", |ui| {
                if ui.button("Import").clicked() {
                    self.send(
                        "import",
                        "google.import",
                        json!({"kind":"tracks","playlist_id":playlist["id"],"name":name}),
                    );
                }
            });
        }
        self.account_row(ui, q, 1);
        for playlist in self.spotify_playlists.clone() {
            let name = playlist["name"]
                .as_str()
                .unwrap_or("Spotify playlist")
                .to_owned();
            Self::row(ui, q, &name, "", "spotify playlist import", |ui| {
                if ui.button("Import").clicked() {
                    self.job(
                        "import",
                        "catalog.import",
                        json!({"url":playlist["external_urls"]["spotify"]}),
                    );
                }
            });
        }
        let lastfm = if self.lastfm_status["signed_in"] == true {
            format!(
                "Connected as {}",
                self.lastfm_status["username"].as_str().unwrap_or("")
            )
        } else {
            "Not connected".to_owned()
        };
        Self::row(
            ui,
            q,
            "Last.fm",
            "Scrobbles after enough real listening time",
            "scrobble lastfm",
            |ui| {
                ui.menu_button("•••", |ui| {
                    if ui.button("Authorize").clicked() {
                        self.send(
                            "authorize",
                            "lastfm.authorize",
                            json!({"api_key":self.lastfm_key,"api_secret":self.lastfm_secret}),
                        );
                        ui.close();
                    }
                    if ui.button("Finish authorization").clicked() {
                        self.send("lastfm-status", "lastfm.finish", json!({}));
                        ui.close();
                    }
                    if ui.button("Pause scrobbling").clicked() {
                        self.send("lastfm-status", "lastfm.enable", json!({"enabled":false}));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Disconnect").clicked() {
                        self.send("lastfm-status", "lastfm.disconnect", json!({}));
                        ui.close();
                    }
                });
                ui.label(RichText::new(&lastfm).small().color(MUTED));
            },
        );
        Self::row(
            ui,
            q,
            "Discord",
            "Show the current track in your Discord status",
            "rich presence",
            |ui| {
                if ui.button("Turn off").clicked() {
                    self.send("discord", "discord.disconnect", json!({}));
                }
                if ui.button("Connect").clicked() {
                    self.send(
                        "discord",
                        "discord.connect",
                        json!({"application_id":self.discord_id}),
                    );
                }
            },
        );
        if Self::matches(
            q,
            &["advanced oauth client id secret callback api key application"],
        ) {
            ui.add_space(6.);
            egui::CollapsingHeader::new(RichText::new("Advanced").color(MUTED)).show(ui, |ui| {
                ui.label(
                    RichText::new("Use your own app credentials. OAuth callback: http://127.0.0.1:8751/callback")
                        .small()
                        .color(MUTED),
                );
                egui::Grid::new("account-advanced").num_columns(2).spacing([12., 8.]).show(ui, |ui| {
                    ui.label("Sign-in provider");
                    egui::ComboBox::from_id_salt("oauth-provider")
                        .selected_text(if self.oauth_provider == 0 { "Google" } else { "Spotify" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.oauth_provider, 0, "Google");
                            ui.selectable_value(&mut self.oauth_provider, 1, "Spotify");
                        });
                    ui.end_row();
                    ui.label("Client ID");
                    ui.text_edit_singleline(&mut self.oauth_client);
                    ui.end_row();
                    ui.label("Client secret");
                    ui.add(egui::TextEdit::singleline(&mut self.oauth_secret).password(true));
                    ui.end_row();
                    ui.label("Last.fm API key");
                    ui.text_edit_singleline(&mut self.lastfm_key);
                    ui.end_row();
                    ui.label("Last.fm secret");
                    ui.add(egui::TextEdit::singleline(&mut self.lastfm_secret).password(true));
                    ui.end_row();
                    ui.label("Discord app ID");
                    ui.text_edit_singleline(&mut self.discord_id);
                    ui.end_row();
                });
            });
        }
    }
    fn settings_network(&mut self, ui: &mut egui::Ui, q: &str) {
        let configured = self.network_info["proxy"].as_str().map(str::to_owned);
        Self::row(
            ui,
            q,
            "Proxy",
            "HTTP or SOCKS5; also used for audio. Stored encrypted for your Windows account",
            "socks http connection",
            |ui| {
                ui.label(
                    RichText::new(configured.as_deref().unwrap_or("System network"))
                        .small()
                        .color(if configured.is_some() { AQUA } else { MUTED }),
                );
            },
        );
        if Self::matches(q, &["proxy socks http connection test"]) {
            ui.add_space(6.);
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.proxy_input)
                        .password(true)
                        .hint_text("http://127.0.0.1:7890")
                        .desired_width(240.),
                );
                if !self.proxy_input.trim().is_empty() && ui.button("Apply").clicked() {
                    self.send("network", "network.set", json!({"proxy":self.proxy_input}));
                }
                if configured.is_some() {
                    if ui.button("Test").clicked() {
                        self.send("network-test", "network.test", json!({}));
                    }
                    if ui.button("Use system network").clicked() {
                        self.send("network", "network.set", json!({"proxy":null}));
                    }
                }
            });
        }
        Self::row(
            ui,
            q,
            "Browser cookies for playback",
            "Lets the playback helper reuse a signed-in browser; locked cookies may be unavailable",
            "chrome edge firefox brave cookies",
            |ui| {
                let before = self.cookie_browser.clone();
                egui::ComboBox::from_id_salt("cookie-browser")
                    .selected_text(if self.cookie_browser.is_empty() {
                        "Off"
                    } else {
                        &self.cookie_browser
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.cookie_browser, String::new(), "Off");
                        for browser in ["chrome", "edge", "firefox", "brave", "vivaldi", "opera"] {
                            ui.selectable_value(&mut self.cookie_browser, browser.into(), browser);
                        }
                    });
                if before != self.cookie_browser {
                    self.send(
                        "cookies",
                        "cookies.browser",
                        json!({"browser":self.cookie_browser}),
                    );
                }
            },
        );
    }
    fn settings_extensions(&mut self, ui: &mut egui::Ui, q: &str) {
        for area in ["themes", "plugins"] {
            if let Some(warnings) = self.extension_status[area]["warnings"].as_array() {
                for warning in warnings {
                    ui.label(
                        RichText::new(format!(
                            "Skipped {area} file {}: {}",
                            warning["file"].as_str().unwrap_or("manifest"),
                            warning["error"].as_str().unwrap_or("Invalid format")
                        ))
                        .small()
                        .color(Color32::from_rgb(255, 193, 198)),
                    );
                }
            }
        }
        Self::row(ui, q, "Save current look", "", "theme save name", |ui| {
            if ui.button("Save").clicked() && !self.theme_name.trim().is_empty() {
                let id = format!("user-{}", uuid::Uuid::new_v4());
                self.send(
                    "theme",
                    "themes.save",
                    json!({"id":id,"name":self.theme_name,"tokens":self.appearance}),
                );
                self.send("themes", "themes.list", json!({}));
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.theme_name)
                    .hint_text("Theme name")
                    .desired_width(160.),
            );
        });
        Self::row(
            ui,
            q,
            "Themes",
            "Import a Roynit or original Riff theme",
            "theme import export",
            |ui| {
                if ui.button("Export").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .set_file_name("riff-theme.json")
                        .save_file()
                {
                    let value = json!({"id":"exported-theme","name":"My Riff theme","tokens":self.appearance});
                    if let Err(e) = riff_core::store::write_json(&path, &value) {
                        self.error = Some(format!("Couldn't export theme: {e}"));
                    }
                }
                if ui.button("Import…").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("Riff JSON theme or settings", &["json"])
                        .pick_file()
                {
                    self.send("theme-import", "themes.import", json!({"path":path}));
                }
            },
        );
        for theme in self.themes.clone() {
            let name = theme["name"].as_str().unwrap_or("Theme").to_owned();
            Self::row(ui, q, &name, "", "theme", |ui| {
                ui.menu_button("•••", |ui| {
                    if ui.button(format!("Delete {name}")).clicked() {
                        self.send("theme", "themes.delete", json!({"id":theme["id"]}));
                        self.send("themes", "themes.list", json!({}));
                        ui.close();
                    }
                });
                if ui.button("Use").clicked() {
                    self.send("appearance", "themes.apply", json!({"id":theme["id"]}));
                }
            });
        }
        Self::row(
            ui,
            q,
            "Native plugins",
            "Run as your Windows user; enable only code you trust",
            "plugin import manifest",
            |ui| {
                if ui.button("Import…").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("Plugin manifest", &["json"])
                        .pick_file()
                {
                    self.send("plugin", "plugins.import", json!({"path":path}));
                }
            },
        );
        for plugin in self.plugins.clone() {
            let name = plugin["name"].as_str().unwrap_or("Plugin").to_owned();
            Self::row(ui, q, &name, "", "plugin", |ui| {
                let mut enabled = plugin["enabled"].as_bool().unwrap_or(false);
                if enabled && ui.button("Run").clicked() {
                    self.send("plugin", "plugins.run", json!({"id":plugin["id"]}));
                }
                if Self::toggle(ui, &mut enabled, &format!("Enable {name}")).changed() {
                    self.send(
                        "plugin",
                        "plugins.enable",
                        json!({"id":plugin["id"],"enabled":enabled}),
                    );
                }
            });
        }
    }
    fn settings_data(&mut self, ui: &mut egui::Ui, q: &str) {
        Self::row(
            ui,
            q,
            "Library backup",
            "",
            "export import restore json",
            |ui| {
                if ui.button("Restore…").clicked()
                && let Some(path) = rfd::FileDialog::new().add_filter("Riff JSON", &["json"]).pick_file()
                && rfd::MessageDialog::new()
                    .set_title("Replace library?")
                    .set_description("This replaces the current library with the selected backup. A before-import backup is kept.")
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show()
                    == rfd::MessageDialogResult::Yes
            {
                self.send("backup", "backup.import", json!({"path":path}));
            }
                if ui.button("Export…").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .set_file_name("riff-library.json")
                        .save_file()
                {
                    self.send("backup", "backup.export", json!({"path":path}));
                }
            },
        );
        Self::row(
            ui,
            q,
            "Original Riff account",
            "",
            "rfacc migrate import export",
            |ui| {
                if ui.button("Export .rfacc…").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .set_file_name("riff-account.rfacc")
                        .save_file()
                {
                    self.send("export", "account.export", json!({"path":path}));
                }
                if ui.button("Import…").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("Original Riff account", &["rfacc", "json"])
                        .pick_file()
                {
                    self.send("migration", "legacy.import", json!({"path":path}));
                }
            },
        );
        let data = self.data_root.display().to_string();
        Self::row(ui, q, "App data", &data, "folder path", |ui| {
            if ui.button("Open").clicked() {
                let _ = open::that(&self.data_root);
            }
        });
        Self::row(ui, q, "Diagnostic report", "", "support logs", |ui| {
            if ui.button("Save…").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .set_file_name("riff-diagnostics.json")
                    .save_file()
            {
                self.send("support", "support.export", json!({"path":path}));
            }
        });
        Self::row(
            ui,
            q,
            "Start menu shortcut",
            "Library and account data stay where they are",
            "install",
            |ui| {
                if ui.button("Install").clicked() {
                    self.send("install", "app.install", json!({}));
                }
            },
        );
        let current = env!("CARGO_PKG_VERSION");
        let latest = self.update_info["version"].as_str().map(str::to_owned);
        let hint = match (&latest, self.update_info["reason"].as_str()) {
            (Some(v), _) => format!("Installed {current} · latest {v}"),
            (None, Some(reason)) => format!("Installed {current} · {reason}"),
            (None, None) => format!("Installed {current}"),
        };
        Self::row(
            ui,
            q,
            "Updates",
            &hint,
            "version release check download",
            |ui| {
                ui.menu_button("•••", |ui| {
                    if ui.button("Prepare previous version").clicked() {
                        self.send("update", "updates.rollback", json!({}));
                        ui.close();
                    }
                });
                if self.update_info["available"] == true && ui.button("Download").clicked() {
                    self.send("update", "updates.stage", json!({}));
                }
                if ui.button("Check").clicked() {
                    self.send("update", "updates.check", json!({}));
                }
            },
        );
        if let Some(path) = self.update_info["staged_path"].as_str().map(str::to_owned) {
            let hint = if self.update_info["rollback"] == true {
                "Previous version verified; restarting keeps your library"
            } else {
                "Downloaded and SHA-256 verified; Roynit will restart"
            };
            Self::row(ui, q, "Ready to install", hint, "update restart", |ui| {
                if ui.button("Restart and apply").clicked() {
                    match riff_core::tools::command(&path)
                        .args([
                            "--install",
                            "--wait-pid",
                            &std::process::id().to_string(),
                            "--launch",
                            "--data-dir",
                        ])
                        .arg(&self.data_root)
                        .spawn()
                    {
                        Ok(_) => {
                            self.shutdown.store(true, Ordering::Relaxed);
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        Err(e) => self.error = Some(format!("Couldn't start the update: {e}")),
                    }
                }
            });
        }
        let mut automatic = self.library.settings.get("auto_update_check") != Some(&json!(false));
        Self::row(ui, q, "Check on start", "", "automatic update", |ui| {
            if Self::toggle(ui, &mut automatic, "Check for updates on start").changed() {
                self.send(
                    "preferences",
                    "settings.set",
                    json!({"auto_update_check":automatic}),
                );
            }
        });
        let saved_repo = self
            .library
            .settings
            .get("release_repo")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        Self::row(
            ui,
            q,
            "Release source",
            "GitHub repository; leave empty to turn checks off",
            "github repo",
            |ui| {
                if self.release_repo != saved_repo && ui.button("Save").clicked() {
                    self.send(
                        "preferences",
                        "updates.configure",
                        json!({"repo":self.release_repo}),
                    );
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.release_repo)
                        .hint_text("Roynit/roynit-music")
                        .desired_width(200.),
                );
            },
        );
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
            graphite_visuals()
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
            0.18
        };
        style.visuals.widgets.hovered.weak_bg_fill = accent.gamma_multiply(0.18);
        style.visuals.widgets.active.weak_bg_fill = accent.gamma_multiply(0.3);
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::proportional(self.appearance.font_size),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::proportional(self.appearance.font_size),
        );
        // Metadata line (artist, counts) scales with the body size: 15 → 12.5.
        style.text_styles.insert(
            egui::TextStyle::Small,
            egui::FontId::proportional(self.appearance.font_size * 0.83),
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
    fn reset_mixes(&mut self) {
        self.mix_generation = self.mix_generation.wrapping_add(1);
        self.mix_signature = None;
        self.mix_loading = false;
        self.mix_error = false;
        self.mixes.clear();
        self.suggested.clear();
        self.youtube_mix_signal = Value::Null;
        self.mix_warnings.clear();
    }

    fn load_mixes_if_needed(&mut self) {
        let signature = (
            self.library.history.len(),
            self.library.history.last().map(|entry| entry.started_at),
            self.library.favorites.clone(),
        );
        if !self.mix_loading && self.mix_signature.as_ref() != Some(&signature) {
            self.mix_signature = Some(signature);
            self.mix_loading = true;
            self.mix_error = false;
            self.send("mixes", "music.mixes", json!({}));
        }
    }

    fn personal_mixes(&mut self, ui: &mut egui::Ui) {
        self.load_mixes_if_needed();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Mixes for you").size(20.).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Refresh mixes").clicked() && !self.mix_loading {
                    self.mix_signature = None;
                    ui.ctx().request_repaint();
                }
                if self.mix_loading && !self.mixes.is_empty() {
                    ui.spinner();
                }
            });
        });
        let likes = self.youtube_mix_signal["likes"].as_u64().unwrap_or(0);
        let playlist_tracks = self.youtube_mix_signal["playlist_tracks"]
            .as_u64()
            .unwrap_or(0);
        let generated = self.youtube_mix_signal["generated_mixes"]
            .as_u64()
            .unwrap_or(0);
        if likes + playlist_tracks + generated > 0 {
            ui.label(RichText::new(format!("YouTube library · {likes} likes · {playlist_tracks} playlist tracks · {generated} YouTube mixes")).small().color(MUTED));
        }
        for warning in &self.mix_warnings {
            ui.label(RichText::new(warning).small().color(MUTED));
        }
        if self.mix_loading && self.mixes.is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Building mixes from your library and listening history…");
            });
        } else if self.mix_error {
            ui.label("Mixes could not load right now.");
        } else if self.mixes.is_empty() {
            ui.label("Play or like a few songs to start building your mixes.");
        }
        let mixes = self.mixes.clone();
        let (columns, side) = Self::card_layout(ui.available_width() - 12., mixes.len(), 3);
        for chunk in mixes.chunks(columns) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.;
                for mix in chunk {
                    let cover = mix.tracks.iter().find_map(|t| t.cover.as_deref());
                    let artists: Vec<_> = mix
                        .tracks
                        .iter()
                        .map(|t| t.artist.as_str())
                        .filter(|a| !a.is_empty())
                        .take(3)
                        .collect();
                    let subtitle = format!("{} songs · {}", mix.tracks.len(), artists.join(", "));
                    let (rect, open, play) =
                        self.cover_card(ui, side, cover, &mix.name, &subtitle, true);
                    if play {
                        self.send(
                            "play",
                            "mix.apply",
                            json!({"name":mix.name,"tracks":mix.tracks,"play":true}),
                        );
                    }
                    let corner = egui::Rect::from_min_size(
                        egui::pos2(rect.right() - 40., rect.top() + 8.),
                        egui::vec2(32., 28.),
                    );
                    let mut save = false;
                    let mut view = open;
                    ui.scope_builder(egui::UiBuilder::new().max_rect(corner), |ui| {
                        let menu =
                            ui.menu_button(RichText::new("•••").color(Color32::WHITE), |ui| {
                                if ui.button("View songs").clicked() {
                                    view = true;
                                    ui.close();
                                }
                                if ui.button("Save playlist").clicked() {
                                    save = true;
                                    ui.close();
                                }
                            });
                        menu.response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                true,
                                format!("Options for {}", mix.name),
                            )
                        });
                    });
                    if save {
                        self.send(
                            "mix-saved",
                            "mix.apply",
                            json!({"name":mix.name,"tracks":mix.tracks}),
                        );
                    }
                    if view {
                        self.navigation.push((
                            self.page,
                            self.tracks.clone(),
                            self.items.clone(),
                            self.browse_title.clone(),
                            self.current_browse.clone(),
                        ));
                        self.results_generation += 1;
                        self.page = Page::Browse;
                        self.browse_title = mix.name.clone();
                        self.current_browse = None;
                        self.tracks = mix.tracks.clone();
                        self.items.clear();
                    }
                }
            });
            ui.add_space(6.);
        }
        if self.page == Page::Home && !self.suggested.is_empty() {
            ui.add_space(10.);
            ui.label(RichText::new("Recommended for you").size(20.).strong());
            self.rows(ui, self.suggested.clone());
        }
        ui.add_space(12.);
    }

    fn autoplay_toggle(&mut self, ui: &mut egui::Ui) {
        let mut autoplay = self.library.settings.get("autoplay") != Some(&json!(false));
        if Self::switch(
            ui,
            &mut autoplay,
            "Autoplay",
            "Add similar songs when the queue ends",
        )
        .changed()
        {
            self.send("preferences", "settings.set", json!({"autoplay":autoplay}));
        }
    }

    fn content(&mut self, ui: &mut egui::Ui) {
        ui.add_space(12.);
        if self.page != Page::Browse {
            ui.label(RichText::new(self.page.title()).size(28.).strong());
            ui.add_space(6.);
        }
        match self.page {
            Page::Home | Page::Search | Page::Browse => {
                if self.page == Page::Browse
                    && self.browse_header(ui)
                    && let Some((page, tracks, items, title, browse)) = self.navigation.pop()
                {
                    self.results_generation += 1;
                    self.page = page;
                    self.tracks = tracks;
                    self.items = items;
                    self.browse_title = title;
                    self.current_browse = browse;
                }

                if self.page == Page::Home {
                    self.personal_mixes(ui);
                    if self.page != Page::Home {
                        return;
                    }
                    ui.label(RichText::new("From YouTube Music").size(20.).strong());
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().button_padding = egui::vec2(10., 4.);
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
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                let mut seen = std::collections::HashSet::new();
                let mut groups: Vec<(&'static str, Vec<Track>)> = Vec::new();
                let mut count = 0;
                for entry in self.library.history.iter().rev() {
                    if count >= 200 || !seen.insert(entry.track_id.clone()) {
                        continue;
                    }
                    let Some(track) = self.library.tracks.get(&entry.track_id).cloned() else {
                        continue;
                    };
                    count += 1;
                    let label = history_group(now, entry.started_at);
                    match groups.last_mut() {
                        Some((last, tracks)) if *last == label => tracks.push(track),
                        _ => groups.push((label, vec![track])),
                    }
                }
                if groups.is_empty() {
                    self.rows(ui, Vec::new());
                }
                for (label, tracks) in groups {
                    ui.add_space(6.);
                    ui.label(RichText::new(label).size(18.).strong());
                    ui.push_id(label, |ui| self.rows(ui, tracks));
                }
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
                ui.push_id("queue-page", |ui| self.play_modes(ui));
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
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.playlist_name)
                            .hint_text("Playlist name")
                            .desired_width(180.),
                    );
                    let label = if self.selected.is_empty() {
                        "New playlist".to_owned()
                    } else {
                        format!("New playlist from {} selected", self.selected.len())
                    };
                    if ui.button(label).clicked() && !self.playlist_name.trim().is_empty() {
                        self.send(
                            "playlist",
                            "playlist.save",
                            json!({"name":self.playlist_name,"tracks":self.selected}),
                        );
                        self.playlist_name.clear();
                    }
                });
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.import_url)
                            .hint_text("Playlist link (YouTube, Spotify, SoundCloud, Deezer)")
                            .desired_width(260.),
                    );
                    if ui.button("Import").clicked() && !self.import_url.trim().is_empty() {
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
                let playlists = self.library.playlists.values().cloned().collect::<Vec<_>>();
                if playlists.is_empty() {
                    ui.add_space(16.);
                    ui.label(RichText::new("No playlists yet").size(20.));
                }
                ui.add_space(6.);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 18.;
                    for playlist in playlists {
                        let active = self.active_playlist.as_deref() == Some(&playlist.id);
                        if Self::tab(ui, &playlist.name, active, 15.).clicked() {
                            self.rename_playlist = playlist.name.clone();
                            self.active_playlist = Some(playlist.id);
                        }
                    }
                });
                ui.add_space(8.);
                if let Some(playlist) = self
                    .active_playlist
                    .as_ref()
                    .and_then(|id| self.library.playlists.get(id))
                    .cloned()
                {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&playlist.name).size(20.).strong());
                        ui.label(
                            RichText::new(format!("{} songs", playlist.tracks.len()))
                                .small()
                                .color(MUTED),
                        );
                        let accent = ui.visuals().selection.bg_fill;
                        let on_accent = ui.visuals().selection.stroke.color;
                        if !playlist.tracks.is_empty()
                            && ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("▶  Play").strong().color(on_accent),
                                    )
                                    .fill(accent)
                                    .corner_radius(16),
                                )
                                .clicked()
                        {
                            self.send(
                                "playlist-play",
                                "queue.set",
                                json!({"tracks":playlist.tracks,"play":true}),
                            );
                        }
                        let menu = ui.menu_button("•••", |ui| {
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.rename_playlist)
                                        .desired_width(150.),
                                );
                                if ui.button("Rename").clicked()
                                    && !self.rename_playlist.trim().is_empty()
                                {
                                    let mut value = playlist.clone();
                                    value.name = self.rename_playlist.clone();
                                    self.send("playlist", "playlist.save", json!(value));
                                    ui.close();
                                }
                            });
                            if !self.selected.is_empty()
                                && ui
                                    .button(format!("Add {} selected", self.selected.len()))
                                    .clicked()
                            {
                                let mut value = playlist.clone();
                                value.tracks.extend(self.selected.clone());
                                self.send("playlist", "playlist.save", json!(value));
                                self.selected.clear();
                                ui.close();
                            }
                            if ui.button("Download playlist").clicked() {
                                self.send(
                                    "download.bulk",
                                    "download.bulk",
                                    json!({"ids":playlist.tracks,"format":self.library.settings.get("download_format").and_then(Value::as_str).unwrap_or("opus")}),
                                );
                                ui.close();
                            }
                            ui.separator();
                            if ui
                                .button(RichText::new("Delete playlist").color(Color32::from_rgb(255, 170, 178)))
                                .clicked()
                            {
                                if rfd::MessageDialog::new()
                                    .set_title("Delete playlist?")
                                    .set_description(format!("“{}” will be removed. Songs stay in your library.", playlist.name))
                                    .set_buttons(rfd::MessageButtons::YesNo)
                                    .show()
                                    == rfd::MessageDialogResult::Yes
                                {
                                    self.send("playlist", "playlist.delete", json!({"id":playlist.id}));
                                    self.active_playlist = None;
                                }
                                ui.close();
                            }
                        });
                        menu.response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                true,
                                "Playlist options",
                            )
                        });
                    });
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
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("lyric-source")
                        .selected_text(format!("Source: {}", self.lyric_source))
                        .show_ui(ui, |ui| {
                            for source in
                                ["auto", "lrclib", "netease", "kugou", "musixmatch", "genius"]
                            {
                                ui.selectable_value(&mut self.lyric_source, source.into(), source);
                            }
                        });
                    ui.add(
                        egui::TextEdit::singleline(&mut self.translation_source)
                            .hint_text("from")
                            .desired_width(50.),
                    )
                    .on_hover_text("Source language, or auto");
                    ui.label(RichText::new("→").color(MUTED));
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
                ui.horizontal_wrapped(|ui| {
                    Self::switch(
                        ui,
                        &mut self.follow_lyrics,
                        "Follow",
                        "Scroll with the current line",
                    );
                    ui.add(
                        egui::Slider::new(&mut self.lyric_offset, -10.0..=10.).text("Offset (s)"),
                    )
                    .on_hover_text("Shift lyric timing");
                    if ui
                        .button("Import…")
                        .on_hover_text("Import an .lrc or .txt file")
                        .clicked()
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
                self.lyric_view(ui, 26., 21.);
            }
            Page::Downloads => {
                let active = self
                    .jobs
                    .values()
                    .filter(|j| j["state"] == "running" || j["state"] == "queued")
                    .count();
                ui.horizontal(|ui| {
                    if ui.button("Open folder").clicked() {
                        let _ = open::that(self.data_root.join("downloads"));
                    }
                    if active > 0 && ui.button(format!("Cancel all ({active})")).clicked() {
                        self.send("cancel", "jobs.cancel_all", json!({}));
                    }
                });
                if self.jobs.is_empty() {
                    ui.add_space(24.);
                    ui.label(RichText::new("No downloads").size(20.));
                }
                let mut tasks = self
                    .jobs
                    .values()
                    .filter(|j| {
                        j["operation"].as_str().is_some_and(|op| {
                            op.contains("download")
                                || op.contains("import")
                                || op == "tools.install"
                        })
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                if tasks.is_empty() && !self.jobs.is_empty() {
                    ui.add_space(24.);
                    ui.label(RichText::new("No downloads").size(20.));
                }
                tasks.sort_by_key(|j| j["id"].as_str().unwrap_or("").to_owned());
                // Backend reports state and elapsed time only, so no percentages are shown.
                for (state, heading) in [
                    ("running", "In progress"),
                    ("queued", "Waiting"),
                    ("failed", "Failed"),
                    ("cancelled", "Cancelled"),
                    ("completed", "Done"),
                ] {
                    let group: Vec<&Value> = tasks
                        .iter()
                        .filter(|j| j["state"] == state)
                        .take(100)
                        .collect();
                    if group.is_empty() {
                        continue;
                    }
                    ui.add_space(8.);
                    ui.label(
                        RichText::new(format!("{heading} · {}", group.len()))
                            .size(18.)
                            .strong(),
                    );
                    for job in group {
                        ui.horizontal(|ui| {
                            if state == "running" {
                                ui.add(egui::Spinner::new().size(14.));
                            }
                            let operation = job["operation"].as_str().unwrap_or("task");
                            let mut name = operation.replace(['.', '_'], " ");
                            if let Some(first) = name.get_mut(0..1) {
                                first.make_ascii_uppercase();
                            }
                            ui.label(name);
                            if let Some(ms) = job["elapsed_ms"].as_u64() {
                                ui.label(
                                    RichText::new(time(ms as f64 / 1000.))
                                        .monospace()
                                        .size(12.)
                                        .color(MUTED),
                                );
                            }
                            if (state == "running" || state == "queued")
                                && ui.small_button("Cancel").clicked()
                            {
                                self.send("cancel", "job.cancel", json!({"id":job["id"]}));
                            }
                        });
                        if let Some(error) = job["error"].as_str() {
                            ui.push_id(job["id"].as_str().unwrap_or(""), |ui| {
                                egui::CollapsingHeader::new(
                                    RichText::new("Why it failed").small().color(MUTED),
                                )
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(error)
                                            .small()
                                            .color(Color32::from_rgb(255, 193, 198)),
                                    );
                                });
                            });
                        }
                    }
                }
            }
            Page::Settings => self.settings_page(ui),
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
        if self.page != Page::Settings
            && let Some(previous) = self.settings_panel_stash.take()
            && self.side_panel.is_none()
        {
            self.side_panel = previous;
        }
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
            self.send("toggle", "player.toggle", json!({}));
        }
        ctx.request_repaint_after(Duration::from_millis(80));
        if self.compact {
            // Mini player reuses the bar's parts so states and keyboard access match.
            let track = self.player["track_id"]
                .as_str()
                .and_then(|id| self.library.tracks.get(id))
                .cloned();
            let cover = track.as_ref().and_then(|t| t.cover.clone());
            let accent = ctx.style().visuals.selection.bg_fill;
            let on_accent = ctx.style().visuals.selection.stroke.color;
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::new()
                        .fill(ctx.style().visuals.panel_fill)
                        .inner_margin(12.),
                )
                .show(ctx, |ui| {
                    let width = ui.available_width();
                    ui.horizontal(|ui| {
                        self.now_playing(
                            ui,
                            track.as_ref(),
                            cover.as_deref(),
                            1.,
                            width - 44.,
                            52.,
                        );
                        if Self::icon_button(ui, "↗", "Back to full player", false).clicked() {
                            Self::set_mini(ctx, &mut self.compact, false);
                        }
                    });
                    self.transport(ui, width, accent, on_accent);
                    self.timeline(ui, width);
                });
            return;
        }
        self.player_bar(ctx);
        // Side panels first so the search bar spans only the content column, as in the design.
        self.side_panel(ctx);
        self.navigation(ctx);
        self.search_bar(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            // Errors sit just above the player, near where playback and actions happen,
            // with the first line visible and the technical detail one click away.
            if let Some(error) = self.error.clone() {
                let area = ui.max_rect();
                egui::Area::new(egui::Id::new("error-toast"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(egui::pos2(area.left() + 12., area.bottom() - 60.))
                    .show(ui.ctx(), |ui| {
                        egui::Frame::new()
                            .fill(Color32::from_rgb(58, 30, 38))
                            .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(120, 60, 72)))
                            .corner_radius(10)
                            .inner_margin(egui::Margin::symmetric(12, 8))
                            .show(ui, |ui| {
                                ui.set_max_width((area.width() - 24.).max(200.));
                                ui.horizontal(|ui| {
                                    let first =
                                        error.lines().next().unwrap_or("Something went wrong");
                                    let short: String = first.chars().take(110).collect();
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(short)
                                                .color(Color32::from_rgb(255, 193, 198)),
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(&error);
                                    if Self::icon_button(ui, "×", "Dismiss error", false).clicked()
                                    {
                                        self.error = None;
                                    }
                                });
                            });
                    });
            }
            if self.busy {
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
            }
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
const GRAPHITE: Color32 = Color32::from_rgb(0x14, 0x15, 0x1D);
const SURFACE: Color32 = Color32::from_rgb(0x1E, 0x20, 0x2B);
const TEXT: Color32 = Color32::from_rgb(0xF2, 0xF1, 0xF7);
const MUTED: Color32 = Color32::from_rgb(0xAA, 0xAE, 0xBE);
const IRIS: Color32 = Color32::from_rgb(0xAA, 0x98, 0xFF);
const AQUA: Color32 = Color32::from_rgb(0x78, 0xDD, 0xDA);
const ARTWORK_FADE: f32 = 0.18;
/// Below this window width the side panel only opens on request from the player bar.
const SIDE_PANEL_MIN_WINDOW: f32 = 1080.;
fn graphite_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.panel_fill = GRAPHITE;
    v.window_fill = SURFACE;
    v.faint_bg_color = Color32::from_rgb(0x19, 0x1A, 0x24);
    v.extreme_bg_color = Color32::from_rgb(0x0F, 0x10, 0x16);
    v.code_bg_color = SURFACE;
    v.weak_text_color = Some(MUTED);
    v.window_stroke = egui::Stroke::new(1.0_f32, Color32::from_white_alpha(18));
    v.window_corner_radius = 14.into();
    v.menu_corner_radius = 10.into();
    let line = egui::Stroke::new(1.0_f32, Color32::from_white_alpha(14));
    for (w, fill) in [
        (&mut v.widgets.noninteractive, GRAPHITE),
        (&mut v.widgets.inactive, SURFACE),
        (&mut v.widgets.hovered, Color32::from_rgb(0x28, 0x2A, 0x38)),
        (&mut v.widgets.active, Color32::from_rgb(0x30, 0x32, 0x42)),
        (&mut v.widgets.open, SURFACE),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.fg_stroke.color = TEXT;
        w.corner_radius = 8.into();
    }
    v.widgets.noninteractive.fg_stroke.color = TEXT;
    v.widgets.noninteractive.bg_stroke = line;
    v.widgets.inactive.bg_stroke = egui::Stroke::NONE;
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, IRIS.gamma_multiply(0.5));
    v.hyperlink_color = AQUA;
    v
}
/// Average colour of a cover, pulled toward a usable glow brightness.
fn cover_tint(bytes: &[u8]) -> Option<Color32> {
    let thumb = image::load_from_memory(bytes)
        .ok()?
        .thumbnail(24, 24)
        .into_rgb8();
    let (mut r, mut g, mut b, mut weight) = (0f32, 0f32, 0f32, 0f32);
    for p in thumb.pixels() {
        let [pr, pg, pb] = p.0.map(f32::from);
        // Favour saturated pixels so grey borders and black bars do not wash the glow out.
        let w = 0.15 + (pr.max(pg).max(pb) - pr.min(pg).min(pb)) / 255.;
        (r, g, b, weight) = (r + pr * w, g + pg * w, b + pb * w, weight + w);
    }
    if weight == 0. {
        return None;
    }
    let (r, g, b) = (r / weight, g / weight, b / weight);
    let scale = 200. / r.max(g).max(b).max(1.);
    Some(Color32::from_rgb(
        (r * scale).min(255.) as u8,
        (g * scale).min(255.) as u8,
        (b * scale).min(255.) as u8,
    ))
}
/// Recency bucket for listening history; days are counted in UTC.
// shortcut: UTC day boundaries, use the local offset if late-night plays land in the wrong group.
fn history_group(now: u64, started_at: u64) -> &'static str {
    match (now / 86_400).saturating_sub(started_at / 86_400) {
        0 => "Today",
        1 => "Yesterday",
        2..=6 => "This week",
        _ => "Earlier",
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
    fn history_groups_by_recency() {
        let day = 86_400;
        let now = 100 * day + 10;
        assert_eq!(history_group(now, now - 5), "Today");
        assert_eq!(history_group(now, now - day), "Yesterday");
        assert_eq!(history_group(now, now - 3 * day), "This week");
        assert_eq!(history_group(now, now - 30 * day), "Earlier");
        assert_eq!(history_group(now, now + 60), "Today");
    }
    #[test]
    fn mix_cards_fit_three_across_the_default_window() {
        // 1120x700 with navigation and Up next open leaves roughly 570 points of content.
        let (columns, side) = App::card_layout(570., 3, 3);
        assert_eq!(columns, 3);
        assert!(side < 190.);
        assert_eq!(App::card_layout(300., 3, 3).0, 1);
        assert_eq!(App::card_layout(2000., 2, 3), (2, 240.));
    }
    #[test]
    fn cover_tint_follows_dominant_colour() {
        let mut png = Vec::new();
        image::RgbImage::from_pixel(8, 8, image::Rgb([180, 20, 40]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let tint = cover_tint(&png).unwrap();
        assert!(tint.r() > 190 && tint.g() < 40 && tint.b() < 60, "{tint:?}");
        assert!(cover_tint(b"not an image").is_none());
    }
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
