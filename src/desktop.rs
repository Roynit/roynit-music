#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod friends;
mod crop_ui;
mod friends_ui;
mod image_cache;
mod import_ui;
mod notify;
mod shop_assets;
mod shop_ui;
mod reorder_ui;
mod roy_account_ui;
mod stats_ui;
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
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use unicode_segmentation::UnicodeSegmentation;

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
    analysis: Arc<AtomicBool>,
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
        // The window sets this while the visualizer is on, even before the choice is saved.
        let visual = analysis.load(Ordering::Relaxed);
        if last.elapsed() > Duration::from_millis(if visual { 45 } else { 150 }) {
            let mut state = engine.snapshot();
            if visual {
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

/// A window started quietly with Windows sits far off screen until it is first shown.
static OFFSCREEN: AtomicBool = AtomicBool::new(false);
/// The window's handle. A hidden window gets no frames, so asking egui to show it never happens; Windows is
/// asked directly instead.
static WINDOW: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

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
    Profile,
    Friends,
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
            Self::Profile => "Profile",
            Self::Friends => "Friends",
        }
    }
}
type NavigationEntry = (Page, Vec<Track>, Vec<Value>, String, Option<Value>);
struct App {
    roy_account: roy_account_ui::State,
    media: Option<system_media::Session>,
    tx: mpsc::Sender<Request>,
    rx: mpsc::Receiver<Message>,
    page: Page,
    query: String,
    search_source: usize,
    search_tracks: [Vec<Track>; 4],
    search_pending: usize,
    search_collections_requested: bool,
    search_generation: u64,
    search_matches: Vec<Track>,
    search_items: Vec<Value>,
    search_open: bool,
    search_failed: Vec<usize>,
    library: Library,
    player: Value,
    player_sample_at: Instant,
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
    sidebar_query: String,
    sidebar_search: bool,
    selected: Vec<String>,
    active_playlist: Option<String>,
    effects: Effects,
    seek_preview: f64,
    browse_title: String,
    data_root: PathBuf,
    import_url: String,
    stats_open: bool,
    shop_open: bool,
    shop_tab: String,
    shop_choice: HashMap<String, usize>,
    greeted_day: String,
    greeted_birthday: i32,
    birthday_dialog: Option<i32>,
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
    mix_warnings: Vec<String>,
    hover_candidate: Option<(Track, Instant)>,
    hover_warmed: Option<String>,
    first_visible_warmed: Option<String>,
    current_browse: Option<Value>,
    shutdown: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
    follow_lyrics: bool,
    lyric_offset: f64,
    lyric_word_mode: bool,
    lyric_words_requested: bool,
    effects_pending: Option<Instant>,
    compact: bool,
    fullscreen: bool,
    fullscreen_lyrics: bool,
    lyrics_was_maximized: bool,
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
    update_staging: bool,
    /// Update now was pressed: install as soon as the download is verified.
    update_now: bool,
    update_dismissed: bool,
    /// A short answer to a manual "Check for updates" when there is nothing new.
    update_flash: Option<(String, std::time::Instant)>,
    /// Photos of artists for the profile page, by artist name (an empty address = none found), and the names already asked for.
    pub(crate) artist_pics: std::collections::HashMap<String, String>,
    pub(crate) artist_pics_asked: std::collections::HashSet<String>,
    pub(crate) artist_pic_queue: Vec<String>,
    background: bool,
    quiet_frames: u8,
    startup_supported: bool,
    startup_state: Arc<AtomicU8>,
    startup_checked: Instant,
    quit_for_update: bool,
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
    card_rect: egui::Rect,
    home_tint: Color32,
    side_visible: bool,
    page_since: Instant,
    page_seen: u8,
    settings_query: String,
    settings_panel_stash: Option<Option<Page>>,
    settings_open: bool,
    ghosts: Vec<reorder_ui::Ghost>,
    social: friends::Social,
    friends_tab: friends_ui::FriendsTab,
    friend_input: String,
    friend_filter: String,
    open_chat: Option<String>,
    friends_notice: Option<(String, bool, Instant)>,
    pending_playlist_order: Option<(Vec<String>, Instant)>,
    pending_track_order: Option<(String, Vec<String>, Instant)>,
    import_source: Option<usize>,
    import_tab: u8,
    import_to_favorites: bool,
    import_pending_fav: bool,
    import_error: Option<String>,
    import_return: bool,
    analysis: Arc<AtomicBool>,
    viz_levels: Vec<f32>,
    viz_presence: f32,
    viz_peak: f32,
}
impl App {
    fn new(cc: &eframe::CreationContext<'_>, root: PathBuf, background: bool) -> Self {
        let ctx = &cc.egui_ctx;
        egui_extras::install_image_loaders(ctx);
        image_cache::DiskImages::install(ctx, root.join("cache").join("pictures"));
        let mut fonts = egui::FontDefinitions::default();
        for (key, file) in [("system", "segoeui.ttf"), ("bold", "segoeuib.ttf")] {
            if let Ok(bytes) = std::fs::read(format!("C:\\Windows\\Fonts\\{file}")) {
                fonts
                    .font_data
                    .insert(key.into(), egui::FontData::from_owned(bytes).into());
            }
        }
        if fonts.font_data.contains_key("system") {
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "system".into());
        }
        // Bold falls back to the regular chain, so a machine without Segoe UI Bold still renders.
        let mut bold_chain: Vec<String> = Vec::new();
        if fonts.font_data.contains_key("bold") {
            bold_chain.push("bold".into());
        }
        bold_chain.extend(
            fonts.families[&egui::FontFamily::Proportional]
                .iter()
                .cloned(),
        );
        fonts
            .families
            .insert(egui::FontFamily::Name("bold".into()), bold_chain);
        ctx.set_fonts(fonts);
        let mut style = (*ctx.style()).clone();
        style.visuals = graphite_visuals();
        style.visuals.selection.bg_fill = DEFAULT_ACCENT;
        style.spacing.item_spacing = egui::vec2(10., 10.);
        style.spacing.button_padding = egui::vec2(12., 6.);
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
        let analysis = Arc::new(AtomicBool::new(false));
        let analysis_worker = analysis.clone();
        let worker = Some(std::thread::spawn(move || {
            worker(root, requests, events, shutdown_worker, analysis_worker)
        }));
        let tray = (|| -> anyhow::Result<tray_icon::TrayIcon> {
            use tray_icon::menu::{Menu, MenuEvent, MenuItem};
            let menu = Menu::new();
            for (id, label) in [
                ("riff_show", "Show Roy Music"),
                ("riff_toggle", "Play / pause"),
                ("riff_next", "Next track"),
                ("riff_quit", "Quit Roy Music"),
            ] {
                menu.append(&MenuItem::with_id(id, label, true, None))?;
            }
            let context = ctx.clone();
            let sender = tx.clone();
            let quit = shutdown.clone();
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                match event.id.0.as_str() {
                    "riff_show" => Self::reveal(&context),
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
                    Self::reveal(&context);
                    context.request_repaint();
                }
            }));
            let image = image::load_from_memory(include_bytes!("../assets/icon-app.png"))?.into_rgba8();
            let (width, height) = image.dimensions();
            let icon = tray_icon::Icon::from_rgba(image.into_raw(), width, height)?;
            Ok(tray_icon::TrayIconBuilder::new()
                .with_tooltip("Roy Music")
                .with_menu(Box::new(menu))
                .with_icon(icon)
                .build()?)
        })()
        .ok();
        #[cfg(windows)]
        if let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(cc)
            && let raw_window_handle::RawWindowHandle::Win32(window) = handle.as_raw()
        {
            WINDOW.store(window.hwnd.get(), Ordering::Relaxed);
        }
        // A second launch of the app wakes this window instead of opening another one.
        {
            let context = ctx.clone();
            riff_core::instance::on_wake(&riff_core::instance::key_for(&data_root), move || {
                Self::reveal(&context);
                context.request_repaint();
            });
        }
        // Started with Windows: stay in the tray; without a tray, wait minimized in the taskbar instead.
        if background && tray.is_none() {
            OFFSCREEN.store(false, Ordering::Relaxed);
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(20., 20.)));
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        let has_tray = tray.is_some();
        let media = system_media::Session::new(cc, tx.clone());
        let media_error = media.as_ref().err().cloned();
        let mut app = Self {
            roy_account: roy_account_ui::State::new(&data_root),
            media: media.ok(),
            tx,
            rx,
            page: Page::Home,
            query: String::new(),
            search_source: 0,
            search_tracks: Default::default(),
            search_pending: 0,
            search_collections_requested: false,
            search_generation: 0,
            search_matches: Vec::new(),
            search_items: Vec::new(),
            search_open: false,
            search_failed: Vec::new(),
            library: Library::default(),
            player: json!({}),
            player_sample_at: Instant::now(),
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
            sidebar_query: String::new(),
            sidebar_search: false,
            selected: vec![],
            active_playlist: None,
            effects: Effects::default(),
            seek_preview: 0.,
            browse_title: String::new(),
            data_root,
            import_url: String::new(),
            stats_open: false,
            shop_open: false,
            shop_tab: "quests".into(),
            shop_choice: HashMap::new(),
            greeted_day: String::new(),
            greeted_birthday: 0,
            birthday_dialog: None,
            oauth_client: String::new(),
            oauth_secret: String::new(),
            oauth_provider: 0,
            search_filter: "all".into(),
            search_limit: 10,
            live_search_at: None,
            mixes: Vec::new(),
            suggested: Vec::new(),
            mix_signature: None,
            mix_loading: false,
            mix_error: false,
            mix_generation: 0,
            mix_warnings: Vec::new(),
            hover_candidate: None,
            hover_warmed: None,
            first_visible_warmed: None,
            current_browse: None,
            shutdown,
            analysis,
            worker,
            follow_lyrics: true,
            lyric_offset: 0.,
            lyric_word_mode: false,
            lyric_words_requested: false,
            effects_pending: None,
            compact: false,
            fullscreen: false,
            fullscreen_lyrics: false,
            lyrics_was_maximized: false,
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
            discord_id: riff_core::DISCORD_APP_ID.into(),
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
            update_staging: false,
            update_now: false,
            update_dismissed: false,
            update_flash: None,
            artist_pics: Default::default(),
            artist_pics_asked: Default::default(),
            artist_pic_queue: Vec::new(),
            background,
            quiet_frames: if background && has_tray { 6 } else { 0 },
            startup_supported: riff_core::startup::is_installed_copy(),
            startup_state: Arc::new(AtomicU8::new(0)),
            startup_checked: Instant::now() - Duration::from_secs(10),
            quit_for_update: false,
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
            card_rect: egui::Rect::NOTHING,
            home_tint: Color32::from_rgb(34, 74, 54),
            side_visible: false,
            page_since: Instant::now(),
            page_seen: 0,
            settings_query: String::new(),
            settings_panel_stash: None,
            settings_open: false,
            ghosts: Vec::new(),
            social: friends::Social::new(),
            friends_tab: friends_ui::FriendsTab::default(),
            friend_input: String::new(),
            friend_filter: String::new(),
            open_chat: None,
            friends_notice: None,
            pending_playlist_order: None,
            pending_track_order: None,
            import_source: None,
            import_tab: 0,
            import_to_favorites: false,
            import_pending_fav: false,
            import_error: None,
            import_return: false,
            viz_levels: Vec::new(),
            viz_presence: 0.,
            viz_peak: 0.05,
        };
        if let Some(error) = media_error {
            app.error = Some(format!("Windows media session unavailable: {error}"));
        }
        app.send("home", "music.home", json!({}));
        for key in ["google", "spotify"] {
            app.send("account-status", "oauth.status", json!({"provider":key}));
        }
        riff_core::warm_up();
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
                Page::Profile,
            ]
            .into_iter()
            .find(|p| p.title().eq_ignore_ascii_case(&start))
            {
                if page == Page::Settings {
                    app.open_settings();
                } else {
                    app.page = page;
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
        } else if matches!(tag, "lyrics" | "lyrics-words" | "translation") {
            let generation = if tag == "lyrics" {
                self.lyric_generation += 1;
                self.lyric_words_requested = false;
                self.lyric_generation
            } else if tag == "lyrics-words" {
                self.lyric_generation
            } else {
                self.translation_generation += 1;
                self.translation_generation
            };
            format!(
                "{tag}@{generation}@{}",
                self.player["track_id"].as_str().unwrap_or("")
            )
        } else if ["home", "browse"].contains(&tag) {
            let old: Vec<_> = self
                .job_tags
                .iter()
                .filter(|(_, tag)| {
                    ["home:", "browse:"]
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
            self.error = Some("Audio engine is no longer running. Restart Roy Music.".into());
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
            ("lyrics-words@", self.lyric_generation, "lyrics-words"),
            ("translation@", self.translation_generation, "translation"),
        ] {
            if let Some(rest) = tag.strip_prefix(prefix) {
                let (number, id) = rest.split_once('@')?;
                return (number.parse::<u64>().ok() == Some(generation)
                    && Some(id) == self.player["track_id"].as_str())
                .then_some(name);
            }
        }
        if let Some((name, number)) = tag.split_once(':') {
            let generation = if Self::search_provider(name).is_some() {
                Some(self.search_generation)
            } else if ["home", "browse"].contains(&name) {
                Some(self.results_generation)
            } else {
                None
            };
            if let Some(generation) = generation {
                return (number.parse::<u64>().ok() == Some(generation)).then_some(name);
            }
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
            if tag != "warm" && !tag.starts_with("mixes:") && !tag.starts_with("lyrics-words@") {
                self.busy = true;
            }
            return;
        }
        let Some(tag) = self.current_tag(tag) else {
            return;
        };
        if let Some(provider) = Self::search_provider(tag) {
            self.complete_search(provider, Some(data));
            return;
        }
        match tag {
            "account-merged" => self.roynit_merge_complete(),
            "account-reset" => {
                self.roy_account.library_reset();
                self.reset_mixes();
            }
            "update" | "update-silent" => {
                self.update_info = data;
                if tag == "update" {
                    self.update_dismissed = false;
                    self.status = match (self.update_info["available"] == true, self.update_info["version"].as_str()) {
                        (true, Some(version)) => format!("Update {version} is available"),
                        _ => format!("You are up to date ({})", env!("CARGO_PKG_VERSION")),
                    };
                    if self.update_info["available"] != true {
                        self.update_flash = Some((self.status.clone(), std::time::Instant::now()));
                    }
                }
                self.stage_update_quietly();
            }
            "update-auto" => {
                self.update_staging = false;
                if data["verified"] == true
                    && let (Some(version), Some(path), Some(sha256)) = (
                        data["version"].as_str(),
                        data["staged_path"].as_str(),
                        data["sha256"].as_str(),
                    )
                {
                    self.send(
                        "preferences",
                        "settings.set",
                        json!({"pending_update":{"version":version,"path":path,"sha256":sha256}}),
                    );
                    if self.update_now {
                        self.update_now = false;
                        self.launch_update(std::path::Path::new(path));
                    }
                } else if self.update_now {
                    self.update_now = false;
                    self.error = Some("The update could not be downloaded and verified. Check your connection and try again.".into());
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
            "account-status" | "adopted" => {
                if let Some(key) = data["provider"].as_str() {
                    self.account_status[key] = data.clone();
                }
            }
            "logout" => {
                self.reset_mixes();
                self.spotify_playlists.clear();
                self.google_playlists.clear();
                self.account_status = json!({});
                for key in ["google", "spotify"] {
                    self.send("account-status", "oauth.status", json!({"provider":key}));
                }
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
                                "Complete sign-in in your browser, then return to Roy Music.".into()
                        }
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
            }
            "auth" => {
                if let Some(key) = data["provider"].as_str() {
                    self.account_status[key] = data.clone();
                }
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
            "playlist-created" => {
                self.active_playlist = data["id"].as_str().map(str::to_owned);
                self.rename_playlist = data["name"].as_str().unwrap_or_default().into();
                self.navigate(Page::Playlists);
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
                if std::mem::take(&mut self.import_pending_fav)
                    && let Some(id) = data["playlist"]["id"].as_str().map(str::to_owned)
                {
                    let tracks: Vec<String> = data["playlist"]["tracks"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|t| t.as_str().map(str::to_owned))
                        .collect();
                    for track in &tracks {
                        self.send("favorites", "favorites.set", json!({"id":track,"enabled":true}));
                    }
                    self.send("playlist", "playlist.delete", json!({"id":id}));
                    self.active_playlist = None;
                    self.page = Page::Favorites;
                    self.status = format!("Added {} songs to Favorites", tracks.len());
                }
            }
            "mixes" => {
                self.mix_loading = false;
                self.mix_error = false;
                self.mix_warnings =
                    serde_json::from_value(data["warnings"].clone()).unwrap_or_default();
                if data["youtube_oauth"] == true {
                    self.google_playlists =
                        data["playlists"].as_array().cloned().unwrap_or_default();
                }
                self.mixes = serde_json::from_value(data["mixes"].clone()).unwrap_or_default();
                self.suggested = serde_json::from_value(data["picks"].clone()).unwrap_or_default();
                // The tracks most likely to be played next get their stream links ready now.
                riff_core::warm_streams(
                    self.suggested
                        .iter()
                        .chain(self.mixes.iter().filter_map(|mix| mix.tracks.first()))
                        .take(10)
                        .map(|track| track.source.clone())
                        .collect(),
                );
            }
            tag if tag.starts_with("artistpic/") => {
                let name = tag["artistpic/".len()..].to_owned();
                let cover = data["items"]
                    .as_array()
                    .and_then(|items| items.iter().find(|item| item["kind"] == "artist"))
                    .and_then(|item| item["cover"].as_str())
                    .unwrap_or("")
                    .to_owned();
                self.artist_pics.insert(name, cover);
            }
            "open-artist" => {
                self.busy = false;
                let artist = data["items"].as_array().and_then(|items| items.iter().find(|item| item["kind"] == "artist")).cloned();
                match artist {
                    Some(item) => self.open_search_collection(&item),
                    None => self.status = "Could not find that artist".into(),
                }
            }
            "mix-saved" => self.status = "Mix saved to your playlists".into(),
            "home" | "browse" | "recommendations" => {
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
            "lyrics-words" => {
                let timed: Vec<Line> =
                    serde_json::from_value(data["lines"].clone()).unwrap_or_default();
                riff_core::lyrics::merge_word_timings(&mut self.lyric_lines, &timed);
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
                    if !self.native_preferences_loaded
                        || data.settings.get("lyrics_words")
                            != self.library.settings.get("lyrics_words")
                    {
                        self.lyric_word_mode = data
                            .settings
                            .get("lyrics_words")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
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
                        // A version downloaded during the last session is installed now, on this launch.
                        if !self.apply_pending_update()
                            && self.library.settings.get("auto_update_check") != Some(&json!(false))
                        {
                            self.send("update-silent", "updates.check", json!({}));
                        }
                        self.startup_defaults();
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
                    self.player_sample_at = Instant::now();
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
                            || tag.starts_with("artistpic/")
                            || tag == "update-silent"
                            || tag == "update-auto"
                            || tag == "cancel"
                            || self.current_tag(&tag) == Some("lyrics-words")
                            || self.current_tag(&tag).is_none() =>
                    {
                        let _ = error;
                    }
                    Err(error) => {
                        if let Some(provider) =
                            self.current_tag(&tag).and_then(Self::search_provider)
                        {
                            self.complete_search(provider, None);
                        } else {
                            self.error = Some(error);
                            self.busy = false;
                        }
                    }
                },
                Message::Event(event) => {
                    if event["event"] == "job_finished" {
                        let job = event["job"].clone();
                        let id = job["id"].as_str().unwrap_or("").to_string();
                        if let Some(tag) = self.job_tags.remove(&id) {
                            if job["state"] == "completed" {
                                self.complete(&tag, job["data"].clone());
                            } else if let Some(provider) =
                                self.current_tag(&tag).and_then(Self::search_provider)
                            {
                                self.complete_search(provider, None);
                            } else if self.current_tag(&tag) == Some("mixes") {
                                self.mix_loading = false;
                                self.mix_error = true;
                            } else if tag == "warm"
                                || tag.starts_with("artistpic/")
                                || tag == "update-silent"
                                || tag == "update-auto"
                                || self.current_tag(&tag) == Some("lyrics-words")
                            {
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
                                && j["operation"] != "lyrics.words"
                                && !j["operation"]
                                    .as_str()
                                    .is_some_and(|op| op.starts_with("lyrics-words@"))
                                && !j["operation"]
                                    .as_str()
                                    .is_some_and(|op| op.starts_with("mixes:"))
                        });
                    } else if event["event"] == "track_changed" {
                        self.player_sample_at = Instant::now();
                        self.player = event["player"].clone();
                        self.status = playback_status(&self.player).into();
                    } else if event["event"] == "playback_recovering" {
                        self.player_sample_at = Instant::now();
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
                .filter(|(_, tag)| tag.starts_with("lyrics@") || tag.starts_with("lyrics-words@"))
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
    const SEARCH_TAGS: [&str; 6] = [
        "search-music",
        "search-spotify",
        "search-soundcloud",
        "search-youtube",
        "search-albums",
        "search-playlists",
    ];
    const SEARCH_SOURCES: [&str; 4] = ["YouTube Music", "Spotify", "SoundCloud", "YouTube"];

    fn search_provider(tag: &str) -> Option<usize> {
        Self::SEARCH_TAGS.iter().position(|name| *name == tag)
    }

    fn reset_search(&mut self) {
        self.search_generation += 1;
        let old: Vec<_> = self
            .job_tags
            .iter()
            .filter(|(_, tag)| tag.starts_with("search-"))
            .map(|(id, _)| id.clone())
            .collect();
        for id in old {
            self.send("cancel", "job.cancel", json!({"id":id}));
        }
        self.search_matches.clear();
        self.search_items.clear();
        self.search_tracks.iter_mut().for_each(Vec::clear);
        self.search_pending = 0;
        self.search_collections_requested = false;
        self.search_failed.clear();
        self.error = None;
    }

    fn schedule_live_search(&mut self) {
        self.reset_search();
        self.search_limit = 12;
        self.search_open = self.query.trim().chars().count() >= 2;
        self.live_search_at = self
            .search_open
            .then(|| Instant::now() + Duration::from_millis(250));
    }

    fn search(&mut self) {
        self.search_open = false;
        if self.query.trim().is_empty() {
            return;
        }
        self.page = Page::Search;
        self.live_search_at = None;
        self.search_limit = 12;
        self.search_current();
        self.load_search_collections();
    }

    fn submit_search(&mut self) {
        if self.search_open
            && self.live_search_at.is_none()
            && (self.search_pending > 0
                || !self.search_matches.is_empty()
                || !self.search_items.is_empty())
        {
            self.search_open = false;
            self.page = Page::Search;
            self.load_search_collections();
        } else {
            self.search();
        }
    }

    fn load_search_collections(&mut self) {
        if self.search_collections_requested
            || self.search_filter != "all"
            || !matches!(self.search_source, 0 | 1)
        {
            return;
        }
        self.search_collections_requested = true;
        for (provider, filter) in [(4, "albums"), (5, "playlists")] {
            let tag = format!("{}:{}", Self::SEARCH_TAGS[provider], self.search_generation);
            self.search_pending += 1;
            self.send(
                &tag,
                "music.search",
                json!({"query":self.query.trim(),"filter":filter}),
            );
        }
    }

    fn search_current(&mut self) {
        if self.query.trim().is_empty() {
            return;
        }
        self.reset_search();
        let categories_only = !matches!(self.search_filter.as_str(), "all" | "songs");
        self.status.clear();
        for (provider, tag) in Self::SEARCH_TAGS
            .iter()
            .take(Self::SEARCH_SOURCES.len())
            .enumerate()
        {
            if (categories_only && provider != 0)
                || (!categories_only
                    && self.search_source != 0
                    && self.search_source != provider + 1)
                || (self.search_source == 0 && provider == 3)
            {
                continue;
            }
            let tag = format!("{tag}:{}", self.search_generation);
            self.search_pending += 1;
            match provider {
                0 => self.send(
                    &tag,
                    "music.search",
                    json!({
                        "query": self.query.trim(),
                        "filter": (self.search_filter != "all").then_some(&self.search_filter),
                    }),
                ),
                1 => self.send(
                    &tag,
                    "spotify.search",
                    json!({"query":self.query.trim(),"limit":12}),
                ),
                _ => self.job(
                    &tag,
                    "search",
                    json!({
                        "query":self.query.trim(),
                        "platform": if provider == 2 { "soundcloud" } else { "youtube" },
                        "limit":self.search_limit,
                    }),
                ),
            }
        }
    }

    fn complete_search(&mut self, provider: usize, data: Option<Value>) {
        self.search_pending = self.search_pending.saturating_sub(1);
        if let Some(data) = data {
            if (provider == 0 && self.search_filter != "songs") || provider >= 4 {
                for item in data["items"].as_array().into_iter().flatten() {
                    let id = item["id"].as_str().unwrap_or_default();
                    if item["kind"] == "playlist"
                        && !["VL", "PL", "RD", "OLAK"]
                            .iter()
                            .any(|prefix| id.starts_with(prefix))
                        || item["kind"] == "artist"
                            && item["subtitle"]
                                .as_str()
                                .is_some_and(|subtitle| subtitle.starts_with("Profile"))
                    {
                        continue;
                    }
                    if !self
                        .search_items
                        .iter()
                        .any(|found| found["id"] == item["id"])
                    {
                        self.search_items.push(item.clone());
                    }
                }
            }
            if provider >= 4 {
                return;
            }
            let tracks: Vec<Track> =
                serde_json::from_value(data.get("tracks").cloned().unwrap_or(data))
                    .unwrap_or_default();
            if matches!(self.search_filter.as_str(), "all" | "songs") {
                self.search_tracks[provider] = tracks;
            }
            let mut seen = std::collections::HashSet::new();
            let longest = self.search_tracks.iter().map(Vec::len).max().unwrap_or(0);
            self.search_matches = (0..longest)
                .flat_map(|rank| {
                    self.search_tracks
                        .iter()
                        .filter_map(move |tracks| tracks.get(rank))
                })
                .filter(|track| seen.insert(track.id.clone()))
                .cloned()
                .collect();
        } else {
            let provider = if provider >= 4 { 0 } else { provider };
            if !self.search_failed.contains(&provider) {
                self.search_failed.push(provider);
            }
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
                            let body = "Sign-in callback received. You can return to Roy Music.";
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
    /// Starting with Windows is on unless the person turned it off: set it up the first time only, and keep the
    /// entry pointing at this copy of the program afterwards.
    fn startup_defaults(&mut self) {
        if !self.startup_supported {
            return;
        }
        let first_time = !self.library.settings.contains_key("startup_initialized");
        if first_time {
            self.send("preferences", "settings.set", json!({"startup_initialized":true}));
        }
        let state = self.startup_state.clone();
        std::thread::spawn(move || {
            if let Ok(exe) = std::env::current_exe() {
                if first_time {
                    let _ = riff_core::startup::enable(&exe);
                } else {
                    riff_core::startup::repair(&exe);
                }
            }
            state.store(Self::startup_code(riff_core::startup::status()), Ordering::Relaxed);
        });
    }
    /// Brings the window back from the tray (or from a quiet start), where the person can see it.
    fn reveal(context: &egui::Context) {
        // A quiet start keeps the window far off screen so nothing flashes; it moves home the first time.
        let moved_home = OFFSCREEN.swap(false, Ordering::Relaxed);
        #[cfg(windows)]
        {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                SW_RESTORE, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow, SetWindowPos, ShowWindow,
            };
            let window = WINDOW.load(Ordering::Relaxed) as *mut std::ffi::c_void;
            if !window.is_null() {
                // SAFETY: the handle belongs to this process's own window, which lives as long as the app.
                unsafe {
                    if moved_home {
                        SetWindowPos(window, std::ptr::null_mut(), 20, 20, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                    }
                    ShowWindow(window, SW_RESTORE);
                    SetForegroundWindow(window);
                }
            }
        }
        if moved_home {
            context.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(20., 20.)));
        }
        context.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        context.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        context.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
    /// 0 off, 1 on, 2 switched off in Task Manager.
    fn startup_code(status: riff_core::startup::Status) -> u8 {
        if status.enabled() {
            1
        } else if status.registered {
            2
        } else {
            0
        }
    }
    /// Reads the real state every couple of seconds while Settings is open, so a change made in Task Manager
    /// shows up here.
    fn refresh_startup_state(&mut self) {
        if self.startup_checked.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.startup_checked = Instant::now();
        let state = self.startup_state.clone();
        std::thread::spawn(move || {
            state.store(Self::startup_code(riff_core::startup::status()), Ordering::Relaxed);
        });
    }
    /// Downloads a newer version in the background so it can be installed the next time the app opens.
    fn stage_update_quietly(&mut self) {
        let settings = &self.library.settings;
        if self.update_staging
            || self.update_info["available"] != true
            || settings.get("auto_update_install") == Some(&json!(false))
        {
            return;
        }
        let latest = self.update_info["version"].as_str().unwrap_or_default();
        let waiting = settings.get("pending_update").and_then(|p| p["version"].as_str());
        if waiting == Some(latest) {
            return;
        }
        self.update_staging = true;
        self.send("update-auto", "updates.stage", json!({}));
    }
    /// Starts the installer for an update downloaded last session, then closes. Returns whether it did.
    fn apply_pending_update(&mut self) -> bool {
        let Some(pending) = self.library.settings.get("pending_update").filter(|v| v.is_object()).cloned() else {
            return false;
        };
        // Forget it first, so a failed install can never repeat on every launch.
        self.send("preferences", "settings.set", json!({"pending_update":null}));
        let (Some(version), Some(path), Some(sha256)) = (
            pending["version"].as_str(),
            pending["path"].as_str(),
            pending["sha256"].as_str(),
        ) else {
            return false;
        };
        let path = std::path::PathBuf::from(path);
        if !path.starts_with(self.data_root.join("updates")) {
            return false;
        }
        if !riff_core::updates::is_newer(version, env!("CARGO_PKG_VERSION")) {
            let _ = std::fs::remove_file(&path);
            return false;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            return false;
        };
        use sha2::{Digest, Sha256};
        if !bytes.starts_with(b"MZ") || !format!("{:x}", Sha256::digest(&bytes)).eq_ignore_ascii_case(sha256) {
            return false;
        }
        self.launch_update(&path)
    }
    /// Starts a verified installer, which waits for this app to close, installs over it and opens it again.
    fn launch_update(&mut self, path: &std::path::Path) -> bool {
        let started = riff_core::tools::command(&path.to_string_lossy())
            .args(["--install", "--wait-pid", &std::process::id().to_string(), "--launch", "--data-dir"])
            .arg(&self.data_root)
            .args(self.background.then_some("--startup"))
            .spawn();
        if started.is_err() {
            self.error = Some("Couldn't start the update installer.".into());
            return false;
        }
        self.shutdown.store(true, Ordering::Relaxed);
        self.quit_for_update = true;
        true
    }
    /// A bar across the top of the window whenever a newer version exists, so nobody has to dig through Settings.
    fn update_banner(&mut self, ctx: &egui::Context) {
        if self.update_info["available"] != true {
            if let Some((text, at)) = self.update_flash.clone() {
                if at.elapsed() < std::time::Duration::from_secs(4) {
                    let visuals = &ctx.style().visuals;
                    let (fill, ink) = (visuals.selection.bg_fill, visuals.selection.stroke.color);
                    egui::TopBottomPanel::top("update_banner")
                        .frame(egui::Frame::new().fill(fill).inner_margin(egui::Margin::symmetric(16, 6)))
                        .show(ctx, |ui| {
                            ui.label(RichText::new(text).color(ink).bold());
                        });
                    ctx.request_repaint_after(std::time::Duration::from_millis(250));
                } else {
                    self.update_flash = None;
                }
            }
            return;
        }
        if self.update_dismissed {
            return;
        }
        let version = self.update_info["version"].as_str().unwrap_or("").to_owned();
        let pending = self
            .library
            .settings
            .get("pending_update")
            .filter(|p| p["version"].as_str() == Some(version.as_str()))
            .and_then(|p| p["path"].as_str())
            .map(str::to_owned);
        let staged = self.update_info["staged_path"].as_str().map(str::to_owned);
        let visuals = &ctx.style().visuals;
        let (fill, ink) = (visuals.selection.bg_fill, visuals.selection.stroke.color);
        let busy = self.update_now;
        let mut action = 0;
        egui::TopBottomPanel::top("update_banner")
            .frame(egui::Frame::new().fill(fill).inner_margin(egui::Margin::symmetric(16, 6)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Roy Music {version} is available")).color(ink).bold());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new(RichText::new("Later").color(ink)).frame(false)).on_hover_text("Hide until the next check").clicked() {
                            action = 3;
                        }
                        let label = if pending.is_some() || staged.is_some() {
                            "Restart and update"
                        } else if busy {
                            "Downloading…"
                        } else {
                            "Update now"
                        };
                        if ui.add_enabled(!busy, egui::Button::new(RichText::new(label).bold()).corner_radius(14)).clicked() {
                            action = if pending.is_some() { 1 } else if staged.is_some() { 2 } else { 4 };
                        }
                    });
                });
            });
        if busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
        match action {
            1 => {
                self.apply_pending_update();
            }
            2 => {
                if let Some(path) = staged {
                    self.launch_update(std::path::Path::new(&path));
                }
            }
            3 => self.update_dismissed = true,
            4 => {
                self.update_now = true;
                if !self.update_staging {
                    self.update_staging = true;
                    self.send("update-auto", "updates.stage", json!({}));
                }
            }
            _ => {}
        }
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
                        menu_style(ui);
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
        let list_id = match self.page {
            Page::Queue => Some("queue".to_owned()),
            Page::Playlists => self.active_playlist.clone(),
            _ => None,
        };
        let reorder = list_id.is_some();
        let selecting = !self.selected.is_empty();
        let mut hovered = None;
        let mut first_visible = None;
        // Reorderable lists give every row a key (id + occurrence) so a song added twice still drags right.
        let mut keys: Vec<String> = Vec::new();
        if reorder {
            let mut seen = std::collections::HashMap::new();
            keys = tracks
                .iter()
                .map(|t| {
                    let n = seen.entry(t.id.clone()).or_insert(0usize);
                    *n += 1;
                    format!("{}#{n}", t.id)
                })
                .collect();
            // Until the saved order comes back from the engine, keep showing the order that was just dropped.
            if let Some((owner, wanted, at)) = &self.pending_track_order {
                let (mut a, mut b) = (wanted.clone(), keys.clone());
                a.sort();
                b.sort();
                let wanted = wanted.clone();
                if Some(owner) != list_id.as_ref()
                    || wanted == keys
                    || at.elapsed() > Duration::from_millis(1500)
                    || a != b
                {
                    self.pending_track_order = None;
                } else {
                    let mut by_key: std::collections::HashMap<String, Track> =
                        keys.iter().cloned().zip(tracks.drain(..)).collect();
                    tracks = wanted.iter().filter_map(|k| by_key.remove(k)).collect();
                    keys = wanted;
                }
            }
        }
        let real_count = tracks.len();
        let mut items: Vec<(Track, String, Option<Instant>)> = tracks
            .into_iter()
            .enumerate()
            .map(|(i, t)| (t, keys.get(i).cloned().unwrap_or_default(), None))
            .collect();
        self.ghosts.retain(|g| reorder_ui::collapse(g.at) > 0.);
        if let Some(id) = &list_id {
            for g in self.ghosts.iter().filter(|g| &g.list == id) {
                if let reorder_ui::Snapshot::Track(t) = &g.snapshot {
                    items.insert(g.index.min(items.len()), (t.clone(), g.key.clone(), Some(g.at)));
                }
            }
            if self.ghosts.iter().any(|g| &g.list == id) {
                ui.ctx().request_repaint();
            }
        }
        let mut seen_real = 0;
        let real_index: Vec<usize> = items
            .iter()
            .map(|item| {
                if item.2.is_some() {
                    seen_real
                } else {
                    seen_real += 1;
                    seen_real - 1
                }
            })
            .collect();
        let collapse_of = |ghost: Option<Instant>| ghost.map_or(1., reorder_ui::collapse);
        ui.spacing_mut().item_spacing.y = 2.;
        let mut list = reorder.then(|| {
            reorder_ui::List::begin(
                ui,
                list_id.as_deref().unwrap_or("rows"),
                items.iter().map(|i| i.1.clone()).collect(),
                items.iter().map(|i| 50. * collapse_of(i.2)).collect(),
                items.iter().map(|i| 48. * collapse_of(i.2)).collect(),
                items.iter().map(|i| i.2.is_none()).collect(),
                self.motion(),
            )
        });
        let order: Vec<usize> = list.as_ref().map_or_else(|| (0..items.len()).collect(), |l| l.order());
        for i in order {
            let (track, key, ghost) = &items[i];
            let index = real_index[i];
            let alpha = collapse_of(*ghost);
            let lift = list.as_ref().map_or(0., |l| l.lift(ui, i));
            let (rect, response) = match &list {
                Some(l) => {
                    let rect = l.rect(i);
                    if rect.height() < 1. {
                        continue;
                    }
                    let sense = if ghost.is_some() { egui::Sense::hover() } else { egui::Sense::click() };
                    (rect, ui.interact(rect, ui.id().with(("track-row", key)), sense))
                }
                None => ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 48.),
                    egui::Sense::click(),
                ),
            };
            let current = self.player["track_id"] == track.id;
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    format!("Play {} by {} [{}]", track.title, track.artist, track.id),
                )
            });
            let pointer_inside = ui.rect_contains_pointer(rect);
            if ui.is_rect_visible(rect) {
                let hover = ui.ctx().animate_bool_with_time(
                    response.id.with("row-hover"),
                    pointer_inside || response.has_focus(),
                    self.motion(),
                );
                reorder_ui::paint_lift(ui, rect, 10, lift);
                let fill = if lift > 0. {
                    Color32::from_gray(0x2A)
                } else if current {
                    Color32::from_white_alpha(14)
                } else {
                    Color32::from_white_alpha((hover * 20.) as u8)
                };
                ui.painter().rect_filled(rect, 10., fill.gamma_multiply(alpha));
                if response.has_focus() {
                    ui.painter().rect_stroke(
                        rect,
                        10.,
                        egui::Stroke::new(1.0_f32, accent),
                        egui::StrokeKind::Inside,
                    );
                }
                let inner = rect.shrink2(egui::vec2(8., 0.)).with_max_x(rect.right() - 8. - if reorder { reorder_ui::GRIP - 8. } else { 0. });
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .id_salt(("row", &track.id, index))
                        .max_rect(inner)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    |ui| {
                        ui.set_clip_rect(rect.intersect(ui.clip_rect()));
                        ui.set_opacity(alpha);
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
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 5.;
                                    if self.page == Page::Search {
                                        Self::search_badge(ui, &track.platform);
                                    }
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(artist).small().color(MUTED),
                                        )
                                        .truncate()
                                        .selectable(false),
                                    );
                                });
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
                        if ghost.is_none() {
                            self.track_menu(ui, track, index, real_count, reorder);
                        }
                    },
                );
            }
            if ghost.is_some() {
                continue;
            }
            if let Some(l) = list.as_mut() {
                l.grip(ui, i, rect);
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
        if let Some(order) = list.and_then(reorder_ui::List::finish) {
            let wanted: Vec<String> = order
                .into_iter()
                .filter(|&i| items[i].2.is_none())
                .map(|i| items[i].1.clone())
                .collect();
            let real: Vec<&(Track, String, Option<Instant>)> =
                items.iter().filter(|i| i.2.is_none()).collect();
            let place = |key: &String| real.iter().position(|i| &i.1 == key);
            if let Some(id) = &list_id {
                if self.page == Page::Queue {
                    let order: Vec<usize> = wanted.iter().filter_map(place).collect();
                    self.send("queue", "queue.reorder", json!({"order":order}));
                } else if let Some(mut playlist) = self.library.playlists.get(id).cloned() {
                    playlist.tracks = wanted
                        .iter()
                        .filter_map(place)
                        .map(|i| real[i].0.id.clone())
                        .collect();
                    self.send("playlist", "playlist.save", json!(playlist));
                }
                self.pending_track_order = Some((id.clone(), wanted, Instant::now()));
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
                    ui.label(RichText::new(format!("{} selected", self.selected.len())).bold());
                    ui.add_space(8.);
                    let ids = self.selected.clone();
                    if ui.button("Add to queue").clicked() {
                        self.send("append", "queue.append", json!({"tracks":ids}));
                    }
                    ui.menu_button("Add to playlist", |ui| {
                        menu_style(ui);
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
                    if self.page == Page::Playlists
                        && ui
                            .button(
                                RichText::new("Delete from playlist")
                                    .color(Color32::from_rgb(255, 170, 178)),
                            )
                            .clicked()
                    {
                        self.remove_selected_from_playlist();
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
            menu_style(ui);
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
                    egui::Label::new(RichText::new(&self.browse_title).size(26.).bold()).truncate(),
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
                                egui::Button::new(RichText::new("▶  Play").bold().color(on_accent))
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
            menu_style(ui);
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
            let disliked = self.library.dislikes.contains(&track.id);
            if ui.button(if disliked { "Remove dislike" } else { "Dislike (do not suggest)" }).clicked() {
                self.set_dislike(track, !disliked);
                ui.close();
            }
            if !track.artist.is_empty() && ui.button("Go to artist").clicked() {
                self.go_to_artist(&track.artist);
                ui.close();
            }
            if !track.album.is_empty() && ui.button("Find album").clicked() {
                self.query = format!("{} {}", track.artist, track.album);
                self.search_source = 0;
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
                    let removed = list.tracks.remove(index);
                    if let Some(gone) = self.library.tracks.get(&removed).cloned() {
                        self.ghosts.push(reorder_ui::Ghost {
                            list: list.id.clone(),
                            key: format!("gone-{}-{index}", removed),
                            index,
                            at: Instant::now(),
                            snapshot: reorder_ui::Snapshot::Track(gone),
                        });
                    }
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
    /// Takes every selected song out of the open playlist (they stay in the library); each row fades out.
    fn remove_selected_from_playlist(&mut self) {
        let Some(mut list) = self
            .active_playlist
            .as_ref()
            .and_then(|id| self.library.playlists.get(id))
            .cloned()
        else {
            return;
        };
        let mut seen = std::collections::HashMap::new();
        let mut kept = Vec::new();
        for (index, id) in list.tracks.iter().enumerate() {
            let n = seen.entry(id.clone()).or_insert(0usize);
            *n += 1;
            match self.library.tracks.get(id).filter(|_| self.selected.contains(id)) {
                Some(track) => self.ghosts.push(reorder_ui::Ghost {
                    list: list.id.clone(),
                    key: format!("{id}#{n}"),
                    index,
                    at: Instant::now(),
                    snapshot: reorder_ui::Snapshot::Track(track.clone()),
                }),
                None => kept.push(id.clone()),
            }
        }
        list.tracks = kept;
        self.selected.clear();
        self.send("playlist", "playlist.save", json!(list));
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
    /// Paint a cover centre-cropped into `rect`, so non-square thumbnails never distort or overlap.
    fn paint_cover(
        ui: &egui::Ui,
        rect: egui::Rect,
        cover: &str,
        radius: impl Into<egui::CornerRadius>,
    ) {
        let image = egui::Image::new(thumb(cover));
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
    }
    /// Round Play button that rises into view with its card; returns whether it was pressed.
    fn reveal_play(
        &self,
        ui: &mut egui::Ui,
        id: egui::Id,
        center: egui::Pos2,
        size: f32,
        shown: f32,
        label: &str,
    ) -> bool {
        if shown < 0.02 {
            return false;
        }
        let ctx = ui.ctx().clone();
        let center = center + egui::vec2(0., (1. - shown) * 8.);
        let rect = egui::Rect::from_center_size(center, egui::vec2(size, size));
        let response = ui.interact(
            rect,
            id,
            if shown > 0.5 {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            },
        );
        let grow = ctx.animate_bool_with_time(id.with("grow"), response.hovered(), self.motion());
        let accent = ui.visuals().selection.bg_fill;
        let r = size / 2. * (1. + 0.06 * grow);
        let painter = ui.painter();
        painter.circle_filled(
            center + egui::vec2(0., 2.),
            r,
            Color32::from_black_alpha((shown * 80.) as u8),
        );
        painter.circle_filled(
            center,
            r,
            accent
                .lerp_to_gamma(GREEN_HOVER, grow)
                .gamma_multiply(shown),
        );
        let k = size / 44.;
        let tri = vec![
            center + egui::vec2(-5., -8.) * k,
            center + egui::vec2(-5., 8.) * k,
            center + egui::vec2(9., 0.) * k,
        ];
        painter.add(egui::Shape::convex_polygon(
            tri,
            Color32::BLACK.gamma_multiply(shown),
            egui::Stroke::NONE,
        ));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
        response.on_hover_text(label.to_owned()).clicked()
    }
    fn cover_card(
        &self,
        ui: &mut egui::Ui,
        side: f32,
        cover: Option<&str>,
        title: &str,
        subtitle: &str,
        with_play: bool,
    ) -> (egui::Rect, bool, bool) {
        let text_height = 58.;
        let (rect, open) =
            ui.allocate_exact_size(egui::vec2(side, side + text_height), egui::Sense::click());
        open.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Open {title}"))
        });
        if !ui.is_rect_visible(rect) {
            return (rect, open.clicked(), false);
        }
        let ctx = ui.ctx().clone();
        let play_id = open.id.with("play");
        let hover = ctx.animate_bool_with_time(
            open.id.with("hover"),
            open.hovered() || open.has_focus() || ctx.memory(|m| m.has_focus(play_id)),
            self.motion(),
        );
        let art = egui::Rect::from_min_size(
            rect.min + egui::vec2(12., 12.),
            egui::vec2(side - 24., side - 24.),
        );
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            8.,
            ELEVATED.lerp_to_gamma(Color32::from_gray(0x28), hover),
        );
        painter.rect_filled(
            art.translate(egui::vec2(0., 6.)).shrink(2.),
            6.,
            Color32::from_black_alpha(70),
        );
        match cover {
            Some(cover) => Self::paint_cover(ui, art, cover, 6.),
            None => {
                ui.painter().rect_filled(art, 6., SURFACE);
            }
        }
        {
            let text = egui::Rect::from_min_size(
                art.left_bottom() + egui::vec2(0., 10.),
                egui::vec2(art.width(), text_height - 18.),
            );
            let ui = &mut ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("cover-text")
                    .max_rect(text)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            ui.spacing_mut().item_spacing.y = 2.;
            ui.add(
                egui::Label::new(RichText::new(title).bold().size(15.))
                    .truncate()
                    .selectable(false),
            );
            ui.add(
                egui::Label::new(RichText::new(subtitle).size(13.).color(MUTED))
                    .truncate()
                    .selectable(false),
            );
        }
        let mut play = false;
        if with_play {
            let center = egui::pos2(art.right() - 30., art.bottom() - 30.);
            play = self.reveal_play(ui, play_id, center, 44., hover, &format!("Play {title}"));
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
    fn catalog(&mut self, ui: &mut egui::Ui, items: Vec<Value>) {
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
                        self.open_search_collection(item);
                    }
                }
            });
            ui.add_space(6.);
        }
        ui.add_space(10.);
    }
    /// Opens an artist's page from just a name, like tapping the name on the phone app.
    fn go_to_artist(&mut self, name: &str) {
        let name = name.split(',').next().unwrap_or(name).trim();
        if name.is_empty() {
            return;
        }
        self.status = format!("Looking for {name}…");
        self.job("open-artist", "music.search", json!({"query":name,"filter":"artists"}));
    }
    /// An artist name that opens the artist's page when clicked.
    fn artist_link(&mut self, ui: &mut egui::Ui, artist: &str, text: RichText) {
        let label = ui.add(egui::Label::new(text).truncate().sense(egui::Sense::click()));
        if label.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Go to artist").clicked() {
            self.go_to_artist(artist);
        }
    }
    fn open_search_collection(&mut self, item: &Value) {
        self.search_open = false;
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
        if item["kind"] == "podcast" {
            self.send("browse", "job.submit", json!({"operation":"music.podcast","params":{"id":item["id"]}}));
        } else {
            self.send(
                "browse",
                "music.browse",
                json!({"id":item["id"],"params":item["params"]}),
            );
        }
    }

    /// Glow colour for a cover; decoded once per URI on a helper thread, never on the audio path.
    fn cover_glow(&self, ctx: &egui::Context, uri: &str) -> Option<Color32> {
        let mut tints = self.cover_tints.lock().ok()?;
        if let Some(tint) = tints.get(uri) {
            return *tint;
        }
        if let Ok(egui::load::BytesPoll::Ready { bytes, .. }) = ctx.try_load_bytes(&thumb(uri)) {
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
                egui::Image::new(thumb(cover))
                    .fit_to_exact_size(egui::vec2(size, size))
                    .corner_radius((size / 7.) as u8)
                    .tint(Color32::WHITE.gamma_multiply(opacity)),
            );
        } else {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
            ui.painter().rect_filled(
                rect,
                size / 7.,
                DEFAULT_ACCENT.gamma_multiply(0.28 * opacity),
            );
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
        let background = if self.appearance.mode == "light" {
            ctx.style().visuals.window_fill
        } else {
            Color32::BLACK
        };
        let accent = ctx.style().visuals.selection.bg_fill;
        let on_accent = ctx.style().visuals.selection.stroke.color;
        egui::TopBottomPanel::bottom("player")
            .frame(
                egui::Frame::new()
                    .fill(background)
                    .inner_margin(egui::Margin {
                        left: 14,
                        right: 14,
                        top: 10,
                        bottom: 10,
                    }),
            )
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(background)
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.visuals_mut().widgets.inactive.bg_fill = Color32::from_white_alpha(34);
                        ui.visuals_mut().widgets.noninteractive.bg_fill =
                            Color32::from_white_alpha(22);
                        if self.appearance.visualizer == "waveform" {
                            let full = ui.max_rect().expand2(egui::vec2(28., 18.));
                            self.wave(ui, full);
                        }
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
                if track.is_none() {
                    ui.add(
                        egui::Image::new(egui::include_image!("../assets/icon.png"))
                            .fit_to_exact_size(egui::vec2(48., 48.)),
                    );
                    return;
                }
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
                                RichText::new(track.map_or("", |t| t.title.as_str())).bold(),
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
                            track.map_or_else(String::new, |t| t.artist.clone())
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
                    self.dislike_button(ui, track);
                }
            },
        );
    }
    fn filter_chip(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
        let fill = if selected {
            ui.visuals().selection.bg_fill
        } else {
            ui.visuals().widgets.inactive.weak_bg_fill
        };
        let text = if selected {
            ui.visuals().selection.stroke.color
        } else {
            ui.visuals().text_color()
        };
        let response = ui.add(
            egui::Button::new(RichText::new(label).size(13.).color(text))
                .fill(fill)
                .corner_radius(20)
                .min_size(egui::vec2(0., 30.)),
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
        });
        response
    }

    fn primary_button(label: RichText, fill: Color32, text: Color32) -> egui::Button<'static> {
        egui::Button::new(label.bold().color(text))
            .fill(fill)
            .corner_radius(24)
    }

    /// Text tab; the active one is marked with an underline, not a filled pill.
    fn tab(ui: &mut egui::Ui, label: &str, active: bool, size: f32) -> egui::Response {
        let accent = ui.visuals().selection.bg_fill;
        let response = ui.add(
            egui::Label::new(RichText::new(label).size(size).bold().color(if active {
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
    /// Low icon button: transparent at rest, soft circle on hover, focus ring, Accent + dot when active.
    fn icon_button(ui: &mut egui::Ui, glyph: &str, label: &str, active: bool) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(34., 34.), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), active, label)
        });
        let accent = ui.visuals().selection.bg_fill;
        let painter = ui.painter();
        if response.is_pointer_button_down_on() || response.hovered() {
            let fill = if response.is_pointer_button_down_on() {
                ui.visuals().widgets.active.weak_bg_fill
            } else {
                ui.visuals().widgets.hovered.weak_bg_fill
            };
            painter.circle_filled(rect.center(), 16., fill);
        }
        if response.has_focus() {
            painter.circle_stroke(rect.center(), 16., egui::Stroke::new(1.5_f32, accent));
        }
        let color = if !ui.is_enabled() {
            MUTED.gamma_multiply(0.5)
        } else if active {
            accent
        } else if response.hovered() {
            ui.visuals().text_color()
        } else {
            ui.visuals().weak_text_color()
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
        let (target, response) = Self::toggle_target(ui, on, label, egui::vec2(50., 32.));
        let t = ui.ctx().animate_bool_with_time(response.id, *on, 0.14);
        let painter = ui.painter();
        let track = target.shrink2(egui::vec2(0., 2.));
        let off = Color32::from_white_alpha(if response.hovered() { 40 } else { 28 });
        painter.rect_filled(track, 14, off.lerp_to_gamma(Color32::WHITE, t));
        painter.rect_stroke(
            track,
            14,
            egui::Stroke::new(1.5_f32, Color32::from_white_alpha(90).gamma_multiply(1. - t)),
            egui::StrokeKind::Inside,
        );
        if response.has_focus() {
            painter.rect_stroke(
                track.expand(2.),
                16,
                egui::Stroke::new(1.5_f32, ui.visuals().selection.bg_fill),
                egui::StrokeKind::Outside,
            );
        }
        let x = egui::lerp(track.left() + 14.0..=track.right() - 14., t);
        painter.circle_filled(
            egui::pos2(x, track.center().y),
            egui::lerp(6.0..=9.5, t),
            Color32::from_gray(0x9A).lerp_to_gamma(Color32::from_gray(0x14), t),
        );
        response
    }
    fn toggle_target(
        ui: &mut egui::Ui,
        on: &mut bool,
        label: &str,
        size: egui::Vec2,
    ) -> (egui::Rect, egui::Response) {
        let (target, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
        if response.clicked() {
            *on = !*on;
            response.mark_changed();
        }
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, label)
        });
        (target, response)
    }

    fn autoplay_switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
        let (target, response) = Self::toggle_target(ui, on, "Autoplay", egui::vec2(48., 32.));
        let t = ui.ctx().animate_bool(response.id, *on);
        let visuals = ui.visuals();
        let muted = visuals.weak_text_color();
        let ink = muted.lerp_to_gamma(visuals.text_color(), t);
        let stroke = egui::Stroke::new(1.6_f32, ink);
        let surface = visuals.panel_fill;
        let center = egui::pos2(
            egui::lerp(target.left() + 12.0..=target.right() - 12., t),
            target.center().y,
        );
        let painter = ui.painter();
        let track = target.shrink2(egui::vec2(2., 8.));
        painter.rect_stroke(track, 8, stroke, egui::StrokeKind::Inside);
        let knob = if response.hovered() {
            visuals.widgets.hovered.weak_bg_fill
        } else {
            surface
        };
        painter.circle(center, 11., knob, stroke);
        painter.add(egui::Shape::closed_line(
            vec![
                center + egui::vec2(-2.8, -4.2),
                center + egui::vec2(4.4, 0.),
                center + egui::vec2(-2.8, 4.2),
            ],
            stroke,
        ));
        if response.has_focus() {
            painter.rect_stroke(
                track.expand(5.),
                13,
                egui::Stroke::new(1_f32, visuals.selection.bg_fill),
                egui::StrokeKind::Outside,
            );
        }
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
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.autoplay_toggle(ui);
            });
        });
    }
    fn transport(&mut self, ui: &mut egui::Ui, width: f32, _accent: Color32, _on_accent: Color32) {
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
                Self::primary_button(
                    RichText::new(if paused { "▶" } else { "⏸" }).size(16.),
                    Color32::WHITE,
                    Color32::BLACK,
                )
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
    /// Spotify's slim bar: a 4 px rail that turns accent-coloured and grows a knob on hover.
    /// Returns the new 0..1 position while the user drags, clicks or presses an arrow key.
    fn thin_bar(
        ui: &mut egui::Ui,
        width: f32,
        frac: f32,
        enabled: bool,
        label: &str,
        step: f32,
    ) -> (egui::Response, Option<f32>) {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(width, 16.),
            if enabled {
                egui::Sense::click_and_drag()
            } else {
                egui::Sense::hover()
            },
        );
        response.widget_info(|| egui::WidgetInfo::slider(enabled, frac as f64, label));
        let hot = enabled && (response.hovered() || response.dragged() || response.has_focus());
        let t = ui
            .ctx()
            .animate_bool_with_time(response.id.with("hot"), hot, 0.12);
        let accent = ui.visuals().selection.bg_fill;
        let rail = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), 4.));
        let painter = ui.painter();
        painter.rect_filled(rail, 2., Color32::from_gray(0x4D));
        let mut fill = rail;
        fill.set_right(rail.left() + rail.width() * frac.clamp(0., 1.));
        let color = if enabled {
            Color32::WHITE.lerp_to_gamma(accent, t)
        } else {
            Color32::from_gray(0x6A)
        };
        painter.rect_filled(fill, 2., color);
        if t > 0.01 {
            painter.circle_filled(
                egui::pos2(fill.right(), rail.center().y),
                6. * t,
                Color32::WHITE,
            );
        }
        if response.has_focus() {
            painter.rect_stroke(
                rect.expand(2.),
                4.,
                egui::Stroke::new(1.5_f32, accent),
                egui::StrokeKind::Outside,
            );
        }
        let mut new = None;
        if enabled
            && let Some(pos) = response.interact_pointer_pos()
            && (response.dragged() || response.clicked() || response.drag_started())
        {
            new = Some(((pos.x - rail.left()) / rail.width()).clamp(0., 1.));
        }
        if enabled && response.has_focus() {
            let (left, right) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::ArrowLeft),
                    i.key_pressed(egui::Key::ArrowRight),
                )
            });
            if left {
                new = Some((frac - step).clamp(0., 1.));
            } else if right {
                new = Some((frac + step).clamp(0., 1.));
            }
        }
        (response, new)
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
            let slider = (width - 104.).max(120.);
            let can_seek = duration > 0.
                && self.player["track_id"].is_string()
                && self.player["pending_track_id"].is_null();
            let frac = if duration > 0. {
                (position / duration) as f32
            } else {
                0.
            };
            let (response, new) = Self::thin_bar(
                ui,
                slider,
                frac,
                can_seek,
                "Seek",
                (5. / duration.max(1.)) as f32,
            );
            if let Some(f) = new {
                self.seek_preview = f as f64 * duration;
            }
            let keyed = response.has_focus() && new.is_some() && !response.dragged();
            if response.drag_stopped() || response.clicked() || keyed {
                self.send("seek", "player.seek", json!({"seconds":self.seek_preview}));
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
        let (volume_bar, new_volume) =
            Self::thin_bar(ui, 96., self.effects.volume, true, "Volume", 0.05);
        volume_bar.on_hover_text(format!("Volume {:.0}%", self.effects.volume * 100.));
        if let Some(volume) = new_volume {
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
        #[cfg(windows)]
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(on));
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
    /// Turns a song off: it is not suggested again, leaves the queue, and if it is playing the next one starts.
    fn set_dislike(&mut self, track: &Track, disliked: bool) {
        self.send("add", "library.add", json!({"track":track}));
        self.send("dislike", "dislikes.set", json!({"id":track.id,"enabled":disliked,"track":track}));
        if disliked && self.current_track().is_some_and(|current| current.id == track.id) {
            self.send("next", "player.next", json!({}));
        }
    }
    /// A "not for me" button: a circle with a slash, red once pressed. A second press takes it back.
    fn dislike_button(&mut self, ui: &mut egui::Ui, track: &Track) {
        let disliked = self.library.dislikes.contains(&track.id);
        let (rect, response) = ui.allocate_exact_size(egui::vec2(28., 28.), egui::Sense::click());
        let color = if disliked {
            Color32::from_rgb(0xED, 0x42, 0x45)
        } else if response.hovered() {
            Color32::WHITE
        } else {
            MUTED
        };
        // A thumb pointing down, drawn from points so it needs no emoji font: the hand, then the cuff.
        let at = |x: f32, y: f32| rect.center() + egui::vec2((x - 12.) * 0.62, (y - 12.) * 0.62);
        let stroke = egui::Stroke::new(1.6_f32, color);
        let hand = [
            (15., 3.), (6., 3.), (4.16, 4.22), (1.14, 11.27), (1., 12.), (1., 14.), (3., 16.), (9.31, 16.), (8.36, 20.57),
            (8.4, 20.9), (8.8, 21.9), (9.83, 23.), (16.42, 16.41), (17., 15.), (17., 5.), (15., 3.),
        ];
        let outline: Vec<egui::Pos2> = hand.iter().map(|(x, y)| at(*x, *y)).collect();
        ui.painter().add(egui::Shape::line(outline, stroke));
        ui.painter().rect_stroke(egui::Rect::from_two_pos(at(19., 3.), at(23., 15.)), 1.0, stroke, egui::StrokeKind::Inside);
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, disliked, "Dislike"));
        let response = response.on_hover_text(if disliked {
            "Disliked: it will not be suggested. Click to undo"
        } else {
            "Dislike: do not suggest this song"
        });
        if response.clicked() {
            self.set_dislike(track, !disliked);
        }
    }
    fn like_button(&mut self, ui: &mut egui::Ui, track: &Track) {
        let liked = self.library.favorites.contains(&track.id);
        let accent = ui.visuals().selection.bg_fill;
        let settled = ui.ctx().animate_value_with_time(
            egui::Id::new(("heart", &track.id)),
            if liked { 1. } else { 0. },
            if self.appearance.reduced_motion {
                0.
            } else {
                0.22
            },
        );
        // Grows then settles while the value travels between 0 and 1.
        let size = 18. + 6. * (settled * std::f32::consts::PI).sin();
        let heart = ui.add(
            egui::Button::new(
                RichText::new(if liked { "♥" } else { "♡" })
                    .size(size)
                    .color(if liked { accent } else { MUTED }),
            )
            .frame(false)
            .min_size(egui::vec2(28., 28.)),
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
                json!({"id":track.id,"enabled":!liked,"track":track}),
            );
        }
    }
    /// Up next / Lyrics beside the content; slides in over `animation_time`, instant with reduced motion.
    fn side_panel(&mut self, ctx: &egui::Context) {
        self.side_visible = false;
        if self.page == Page::Profile {
            return;
        }
        let wide = ctx.content_rect().width() >= SIDE_PANEL_MIN_WINDOW;
        let open = self.side_panel.is_some() && (wide || self.narrow_open);
        self.side_visible = open;
        let page = self.side_panel.unwrap_or(Page::Queue);
        egui::SidePanel::right("now-panel")
            .exact_width(300.)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(Self::chrome(ctx))
                    .inner_margin(egui::Margin {
                        left: 4,
                        right: 8,
                        top: 0,
                        bottom: 4,
                    }),
            )
            .show_animated(ctx, open, |ui| {
                Self::panel_card(ui, 16, |ui| {
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
                        self.lyrics_panel(ui);
                    } else if self.player["track_id"].is_string()
                        || !self.library.queue.tracks.is_empty()
                    {
                        self.play_modes(ui);
                        ui.add_space(4.);
                        self.queue_list(ui);
                    }
                })
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
                        ui.label(RichText::new(heading).small().bold().color(MUTED));
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
    fn current_track(&self) -> Option<Track> {
        self.player["track_id"]
            .as_str()
            .and_then(|id| self.library.tracks.get(id))
            .cloned()
    }

    fn show_fullscreen_lyrics(&mut self, ctx: &egui::Context, show: bool) {
        if show {
            self.lyrics_was_maximized =
                ctx.input(|input| input.viewport().maximized.unwrap_or(false));
        }
        self.fullscreen_lyrics = show;
        // Keep the Windows compositor active while replacing the app content.
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(
            show || self.lyrics_was_maximized,
        ));
        ctx.request_repaint();
    }

    fn follow_lyrics_button(ui: &mut egui::Ui, active: bool) -> egui::Response {
        let response = Self::icon_button(ui, "", "Follow current lyric", active);
        let center = response.rect.center() - egui::vec2(0., if active { 2. } else { 0. });
        let stroke = egui::Stroke::new(1.4_f32, ui.visuals().text_color());
        ui.painter().circle_stroke(center, 6.5, stroke);
        ui.painter().add(egui::Shape::line(
            vec![
                center - egui::vec2(0., 4.),
                center,
                center + egui::vec2(3.5, 2.),
            ],
            stroke,
        ));
        response
    }

    fn lyrics_panel(&mut self, ui: &mut egui::Ui) {
        let Some(track) = self.current_track() else {
            return;
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.;
            if Self::icon_button(ui, "⋯", "Lyric source, timing and translation", false).clicked()
            {
                self.page = Page::Lyrics;
            }
            if Self::follow_lyrics_button(ui, self.follow_lyrics).clicked() {
                self.follow_lyrics = !self.follow_lyrics;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if Self::expand_lyrics_button(ui).clicked() {
                    self.show_fullscreen_lyrics(ui.ctx(), true);
                }
                self.lyric_mode(ui);
            });
        });
        egui::ScrollArea::vertical()
            .id_salt("panel-lyrics")
            .auto_shrink(false)
            .show(ui, |ui| {
                let size = (ui.available_width() - 4.).max(48.);
                self.artwork(ui, track.cover.as_deref(), size, 1.);
                if ui
                    .interact(
                        ui.min_rect()
                            .with_min_y(ui.cursor().top() - size - ui.spacing().item_spacing.y),
                        ui.id().with("expand-lyric-cover"),
                        egui::Sense::click(),
                    )
                    .on_hover_text("Expand lyrics")
                    .clicked()
                {
                    self.show_fullscreen_lyrics(ui.ctx(), true);
                }
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 44.).max(70.);
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        ui.add(
                            egui::Label::new(RichText::new(&track.title).size(20.).bold())
                                .truncate(),
                        );
                        self.artist_link(ui, &track.artist, RichText::new(&track.artist).color(MUTED));
                    });
                    self.like_button(ui, &track);
                    self.dislike_button(ui, &track);
                });
                if !self.lyric_text.is_empty() || !self.lyric_lines.is_empty() {
                    ui.add_space(12.);
                    ui.separator();
                    self.lyric_view(ui, 20., 16.);
                }
            });
    }

    fn expand_lyrics_button(ui: &mut egui::Ui) -> egui::Response {
        let response = Self::icon_button(ui, "", "Fullscreen lyrics", false);
        let center = response.rect.center();
        let stroke = egui::Stroke::new(1.5_f32, ui.visuals().text_color());
        for (x, y) in [(-1., -1.), (-1., 1.), (1., -1.), (1., 1.)] {
            let corner = center + egui::vec2(x * 7., y * 7.);
            ui.painter().add(egui::Shape::line(
                vec![
                    corner - egui::vec2(x * 4., 0.),
                    corner,
                    corner - egui::vec2(0., y * 4.),
                ],
                stroke,
            ));
        }
        response
    }

    fn fullscreen_lyrics_view(&mut self, ctx: &egui::Context) {
        let track = self.current_track();
        let tint = track
            .as_ref()
            .and_then(|track| track.cover.as_deref())
            .and_then(|cover| self.cover_glow(ctx, cover))
            .unwrap_or(SURFACE);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_gray(15).lerp_to_gamma(tint, 0.18))
                    .inner_margin(28),
            )
            .show(ctx, |ui| {
                ui.visuals_mut().override_text_color = Some(Color32::WHITE);
                ui.visuals_mut().weak_text_color = Some(Color32::from_gray(182));
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), 36.),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if Self::icon_button(ui, "×", "Exit fullscreen lyrics", false).clicked() {
                            self.show_fullscreen_lyrics(ctx, false);
                        }
                        if Self::follow_lyrics_button(ui, self.follow_lyrics).clicked() {
                            self.follow_lyrics = !self.follow_lyrics;
                        }
                        self.lyric_mode(ui);
                    },
                );
                let Some(track) = track else { return };
                let width = ui.available_width();
                let height = ui.available_height();
                let left = (width * 0.43).min(520.);
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(left, height),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            let size = left.min((height - 160.).max(120.));
                            self.artwork(ui, track.cover.as_deref(), size, 1.);
                            ui.add(
                                egui::Label::new(RichText::new(&track.title).size(30.).bold())
                                    .truncate(),
                            );
                            self.artist_link(ui, &track.artist, RichText::new(&track.artist).size(17.).color(MUTED));
                            let accent = ui.visuals().selection;
                            self.transport(ui, left, accent.bg_fill, accent.stroke.color);
                            self.timeline(ui, left);
                        },
                    );
                    ui.add_space(24.);
                    if !self.lyric_text.is_empty() || !self.lyric_lines.is_empty() {
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                egui::ScrollArea::vertical()
                                    .id_salt("fullscreen-lyrics-body")
                                    .auto_shrink([false, false])
                                    .max_height(height)
                                    .show(ui, |ui| self.lyric_view(ui, 34., 27.));
                            },
                        );
                    }
                });
            });
    }

    fn lyric_mode(&mut self, ui: &mut egui::Ui) {
        let mode = if self.lyric_word_mode {
            "Words"
        } else {
            "Line"
        };
        if Self::filter_chip(ui, mode, self.lyric_word_mode)
            .on_hover_text("Switch between line and word highlighting")
            .clicked()
        {
            self.lyric_word_mode = !self.lyric_word_mode;
            self.send(
                "preferences",
                "settings.set",
                json!({"lyrics_words":self.lyric_word_mode}),
            );
        }
    }

    fn request_word_timings(&mut self) {
        if !self.lyric_word_mode
            || self.lyric_words_requested
            || self.lyric_lines.is_empty()
            || self.lyric_lines.iter().all(|line| !line.words.is_empty())
        {
            return;
        }
        if let Some(track) = self.current_track() {
            self.lyric_words_requested = true;
            self.send(
                "lyrics-words",
                "job.submit",
                json!({
                    "operation":"lyrics.words",
                    "params":{"title":track.title,"artist":track.artist,"duration":track.duration}
                }),
            );
        }
    }

    fn lyric_view(&mut self, ui: &mut egui::Ui, active_size: f32, line_size: f32) {
        self.request_word_timings();
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
            let playing = self.player["playing"] == true && self.player["buffering"] != true;
            let reduced_motion = self.appearance.reduced_motion;
            let duration = self.player["duration"].as_f64().unwrap_or_default();
            let position = self.player["position"].as_f64().unwrap_or_default();
            let mut now = position;
            if playing && !reduced_motion {
                let speed = self.player["effects"]["speed"].as_f64().unwrap_or(1.);
                now += self.player_sample_at.elapsed().as_secs_f64().min(0.16) * speed;
                if duration > 0. {
                    now = now.min(duration);
                }
                ui.ctx().request_repaint_after(Duration::from_millis(16));
            }
            let index = riff_core::lyrics::active(&self.lyric_lines, now, self.lyric_offset);
            now += self.lyric_offset;
            let sync_id = ui.id().with("lyrics-scroll-target");
            let target = self
                .follow_lyrics
                .then_some(index)
                .flatten()
                .map(|line| (self.lyric_generation, line));
            let recenter = ui.ctx().data_mut(|data| {
                let previous = data.get_temp::<(u64, usize)>(sync_id);
                if let Some(target) = target {
                    data.insert_temp(sync_id, target);
                } else {
                    data.remove::<(u64, usize)>(sync_id);
                }
                target.is_some() && previous != target
            });
            let font = egui::FontId::proportional(line_size + (active_size - line_size) * 0.46);
            let ink = ui.visuals().text_color();
            let muted = ui.visuals().weak_text_color();
            let dim = muted.lerp_to_gamma(ui.visuals().panel_fill, 0.24);
            let mut seek_line = None;
            for (i, line) in self.lyric_lines.iter().enumerate() {
                ui.add_space(10.);
                let active = index == Some(i);
                let emphasis = ui.ctx().animate_bool_with_time(
                    ui.id().with(("lyric-emphasis", self.lyric_generation, i)),
                    active,
                    if reduced_motion { 0. } else { 0.17 },
                );
                let estimated;
                let words = if active && self.lyric_word_mode {
                    if line.words.is_empty() {
                        estimated = riff_core::lyrics::estimate_word_timings(
                            line,
                            self.lyric_lines.get(i + 1),
                            self.player["duration"].as_f64().unwrap_or_default(),
                        );
                        &estimated[..]
                    } else {
                        &line.words[..]
                    }
                } else {
                    &[]
                };
                let foreground = if words.is_empty() { ink } else { muted };
                let base = dim.lerp_to_gamma(foreground, emphasis);
                let mut layout = egui::text::LayoutJob::default();
                let mut cursor = 0;
                for (word_index, word) in words.iter().enumerate() {
                    let text = word.text.trim();
                    if text.is_empty() {
                        continue;
                    }
                    let Some(relative) = line.text[cursor..].find(text) else {
                        continue;
                    };
                    let at = cursor + relative;
                    layout.append(
                        &line.text[cursor..at],
                        0.,
                        egui::TextFormat::simple(font.clone(), base),
                    );
                    let end = word
                        .end
                        .or_else(|| words.get(word_index + 1).map(|next| next.time))
                        .or_else(|| self.lyric_lines.get(i + 1).map(|next| next.time))
                        .unwrap_or(word.time + 0.45);
                    let progress =
                        ((now - word.time) / (end - word.time).max(0.08)).clamp(0., 1.) as f32;
                    if reduced_motion || progress == 0. || progress == 1. {
                        let color = if progress == 1. || (reduced_motion && now >= word.time) {
                            ink
                        } else {
                            base
                        };
                        layout.append(text, 0., egui::TextFormat::simple(font.clone(), color));
                    } else {
                        let count = text.graphemes(true).count().max(1) as f32;
                        let sweep = progress * (count + 0.7);
                        for (glyph, grapheme) in text.graphemes(true).enumerate() {
                            let amount = ((sweep - glyph as f32) / 1.7).clamp(0., 1.);
                            let amount = amount * amount * (3. - 2. * amount);
                            layout.append(
                                grapheme,
                                0.,
                                egui::TextFormat::simple(
                                    font.clone(),
                                    base.lerp_to_gamma(ink, amount),
                                ),
                            );
                        }
                    }
                    cursor = at + text.len();
                }
                layout.append(
                    &line.text[cursor..],
                    0.,
                    egui::TextFormat::simple(font.clone(), base),
                );
                let response = ui.add(egui::Label::new(layout).wrap().sense(egui::Sense::click()));
                if response.clicked() {
                    seek_line = Some(line.time);
                }
                if active && recenter {
                    let motion = if reduced_motion {
                        egui::style::ScrollAnimation::none()
                    } else {
                        egui::style::ScrollAnimation::new(1050., egui::Rangef::new(0.22, 0.42))
                    };
                    response.scroll_to_me_animation(Some(egui::Align::Center), motion);
                }
            }
            if let Some(seconds) = seek_line {
                self.send("seek", "player.seek", json!({"seconds":seconds}));
            }
        }
    }
    const LIBRARY_WIDTH: f32 = 224.;

    fn navigate(&mut self, page: Page) {
        if self.page == page {
            return;
        }
        self.live_search_at = None;
        self.page = page;
        if page == Page::Home {
            self.send("home", "music.home", json!({}));
        }
    }

    /// Window chrome stays pure black in dark mode so the panels read as cards floating on it.
    fn chrome(ctx: &egui::Context) -> Color32 {
        if ctx.style().visuals.dark_mode {
            Color32::BLACK
        } else {
            ctx.style().visuals.panel_fill
        }
    }
    /// The rounded #121212 card Spotify's side panels sit in; it fills the panel it is placed in.
    fn panel_card<R>(ui: &mut egui::Ui, margin: i8, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
        let fill = ui.visuals().panel_fill;
        egui::Frame::new()
            .fill(fill)
            .corner_radius(PANEL_RADIUS)
            .inner_margin(margin)
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                add(ui)
            })
            .inner
    }
    /// Animation length for hovers and reveals; zero when the user asked for reduced motion.
    fn motion(&self) -> f32 {
        if self.appearance.reduced_motion {
            0.
        } else {
            0.16
        }
    }

    fn navigation(&mut self, ctx: &egui::Context) {
        if self.page == Page::Friends {
            self.friends_sidebar(ctx);
            return;
        }
        egui::SidePanel::left("navigation")
            .exact_width(Self::LIBRARY_WIDTH)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(Self::chrome(ctx))
                    .inner_margin(egui::Margin {
                        left: 8,
                        right: 4,
                        top: 0,
                        bottom: 4,
                    }),
            )
            .show(ctx, |ui| {
                Self::panel_card(ui, 12, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Your library").size(17.).bold());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (button, _) = egui::containers::menu::MenuButton::new(
                                RichText::new("+ Create").size(13.).bold(),
                            )
                            .config(
                                egui::containers::menu::MenuConfig::new()
                                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside),
                            )
                            .ui(ui, |ui| {
                                let name = ui.add(
                                    egui::TextEdit::singleline(&mut self.playlist_name)
                                        .hint_text("Playlist name")
                                        .desired_width(165.),
                                );
                                let enter = name.lost_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                if (ui.button("Create playlist").clicked() || enter)
                                    && !self.playlist_name.trim().is_empty()
                                {
                                    self.save_new_playlist();
                                    ui.close();
                                }
                                ui.separator();
                                if ui.button("Import playlist").clicked() {
                                    self.active_playlist = None;
                                    self.open_import();
                                    ui.close();
                                }
                            });
                            button.on_hover_text("Create or import a playlist");
                        });
                    });
                    ui.add_space(5.);
                    ui.horizontal(|ui| {
                        if Self::icon_button(ui, "🔍", "Find a playlist", self.sidebar_search)
                            .clicked()
                        {
                            self.sidebar_search = !self.sidebar_search;
                            if self.sidebar_search {
                                ui.memory_mut(|memory| {
                                    memory.request_focus(egui::Id::new("library-playlist-filter"))
                                });
                            } else {
                                self.sidebar_query.clear();
                            }
                        }
                        if Self::icon_button(
                            ui,
                            "♫",
                            "All library tracks",
                            self.page == Page::Library,
                        )
                        .clicked()
                        {
                            self.navigate(Page::Library);
                        }
                        if Self::icon_button(ui, "↓", "Offline music", self.page == Page::Offline)
                            .clicked()
                        {
                            self.navigate(Page::Offline);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .selectable_label(self.page == Page::History, "Recents")
                                .clicked()
                            {
                                self.navigate(Page::History);
                            }
                        });
                    });
                    if self.sidebar_search {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.sidebar_query)
                                .id(egui::Id::new("library-playlist-filter"))
                                .hint_text("Find a playlist")
                                .desired_width(ui.available_width()),
                        );
                    }
                    ui.add_space(9.);
                    egui::ScrollArea::vertical()
                        .id_salt("library-playlists")
                        .show(ui, |ui| {
                            if Self::matches(&self.sidebar_query, &["Liked songs"])
                                && self
                                    .library_tile(
                                        ui,
                                        "Liked songs",
                                        self.library.favorites.len(),
                                        None,
                                        "♥",
                                        self.page == Page::Favorites,
                                    )
                                    .clicked()
                            {
                                self.navigate(Page::Favorites);
                            }
                            self.playlist_list(ui);
                        });
                })
            });
    }

    fn library_tile(
        &self,
        ui: &mut egui::Ui,
        name: &str,
        count: usize,
        cover: Option<&str>,
        glyph: &str,
        active: bool,
    ) -> egui::Response {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 54.), egui::Sense::hover());
        let id = ui.id().with(("tile", name));
        self.tile_at(ui, rect, id, egui::Sense::click(), name, count, cover, glyph, active, 1., 0.)
    }

    /// A library tile drawn into `rect`. `alpha` fades it (a playlist being removed) and `lift` raises it
    /// off the page while it is dragged.
    #[allow(clippy::too_many_arguments)]
    fn tile_at(
        &self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        id: egui::Id,
        sense: egui::Sense,
        name: &str,
        count: usize,
        cover: Option<&str>,
        glyph: &str,
        active: bool,
        alpha: f32,
        lift: f32,
    ) -> egui::Response {
        let response = ui.interact(rect, id, sense);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, name)
        });
        let accent = ui.visuals().selection.bg_fill;
        reorder_ui::paint_lift(ui, rect, 9, lift);
        let hover = ui
            .ctx()
            .animate_bool_with_time(id.with("hover"), response.hovered() || lift > 0., self.motion());
        let fill = if lift > 0. {
            Color32::from_gray(0x2A)
        } else if active {
            accent.gamma_multiply(0.18)
        } else {
            ui.visuals().widgets.hovered.weak_bg_fill.gamma_multiply(hover)
        };
        ui.painter().rect_filled(rect, 9, fill.gamma_multiply(alpha));
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect,
                9,
                egui::Stroke::new(1_f32, accent),
                egui::StrokeKind::Inside,
            );
        }
        let inner = rect.shrink2(egui::vec2(6., 0.));
        {
            let ui = &mut ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("library-tile-content")
                    .max_rect(inner)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            ui.set_opacity(alpha);
            if cover.is_some() {
                self.artwork(ui, cover, 40., 1.);
            } else {
                let (art, _) = ui.allocate_exact_size(egui::vec2(40., 40.), egui::Sense::hover());
                let fill = if glyph == "♥" {
                    accent.gamma_multiply(0.36)
                } else {
                    ui.visuals().widgets.inactive.bg_fill
                };
                ui.painter().rect_filled(art, 8, fill);
                ui.painter().text(
                    art.center(),
                    egui::Align2::CENTER_CENTER,
                    glyph,
                    egui::FontId::proportional(21.),
                    ui.visuals().text_color(),
                );
            }
            let width = ui.available_width();
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.add(
                    egui::Label::new(RichText::new(name).bold())
                        .truncate()
                        .selectable(false),
                );
                let unit = if count == 1 { "song" } else { "songs" };
                ui.label(
                    RichText::new(format!("{count} {unit}"))
                        .small()
                        .color(MUTED),
                );
            });
        }
        response.on_hover_text(name)
    }

    /// The playlists in the sidebar: drag one to move it, right-click or use its menu to delete it.
    fn playlist_list(&mut self, ui: &mut egui::Ui) {
        struct Row {
            key: String,
            name: String,
            count: usize,
            cover: Option<String>,
            /// Whether the cover is a picture the person chose (it can be taken away).
            custom: bool,
            ghost: Option<Instant>,
        }
        let filtering = !self.sidebar_query.trim().is_empty();
        let mut ids: Vec<String> = self.library.ordered_playlists().iter().map(|p| p.id.clone()).collect();
        if let Some((pending, at)) = &self.pending_playlist_order {
            let same: std::collections::HashSet<_> = pending.iter().collect();
            if *pending == ids || at.elapsed() > Duration::from_millis(1500) || same != ids.iter().collect() {
                self.pending_playlist_order = None;
            } else {
                ids = pending.clone();
            }
        }
        let mut rows: Vec<Row> = ids
            .iter()
            .filter_map(|id| self.library.playlists.get(id))
            .filter(|p| Self::matches(&self.sidebar_query, &[&p.name]))
            .map(|p| Row {
                key: p.id.clone(),
                name: p.name.clone(),
                count: p.tracks.len(),
                cover: self.roy_account.playlist_cover_uri(&p.id, p.cover).or_else(|| {
                    p.tracks
                        .iter()
                        .filter_map(|id| self.library.tracks.get(id))
                        .find_map(|t| t.cover.clone())
                }),
                custom: p.cover > 0,
                ghost: None,
            })
            .collect();
        self.ghosts.retain(|g| reorder_ui::collapse(g.at) > 0.);
        if !filtering {
            for ghost in self.ghosts.iter().filter(|g| g.list == "playlists") {
                if let reorder_ui::Snapshot::Playlist { name, count, cover } = &ghost.snapshot {
                    rows.insert(
                        ghost.index.min(rows.len()),
                        Row { key: ghost.key.clone(), name: name.clone(), count: *count, cover: cover.clone(), custom: false, ghost: Some(ghost.at) },
                    );
                }
            }
        }
        if self.ghosts.iter().any(|g| g.list == "playlists") {
            ui.ctx().request_repaint();
        }
        let factor = |row: &Row| row.ghost.map_or(1., reorder_ui::collapse);
        let mut list = reorder_ui::List::begin(
            ui,
            "playlists",
            rows.iter().map(|r| r.key.clone()).collect(),
            rows.iter().map(|r| 56. * factor(r)).collect(),
            rows.iter().map(|r| 54. * factor(r)).collect(),
            rows.iter().map(|r| !filtering && r.ghost.is_none()).collect(),
            self.motion(),
        );
        let (mut open, mut delete) = (None, None);
        let (mut change_cover, mut remove_cover): (Option<String>, Option<String>) = (None, None);
        for i in list.order() {
            let row = &rows[i];
            let rect = list.rect(i);
            if rect.height() < 1. {
                continue;
            }
            let active = self.page == Page::Playlists && self.active_playlist.as_deref() == Some(row.key.as_str());
            let id = ui.id().with(("playlist-tile", &row.key));
            let sense = if row.ghost.is_some() { egui::Sense::hover() } else { egui::Sense::click() };
            let lift = list.lift(ui, i);
            let response = self.tile_at(ui, rect, id, sense, &row.name, row.count, row.cover.as_deref(), "♫", active, factor(row), lift);
            if row.ghost.is_some() {
                continue;
            }
            list.grip(ui, i, rect);
            if response.clicked() {
                open = Some((row.key.clone(), row.name.clone()));
            }
            // A "•••" appears on hover; the same menu opens on right-click.
            let dots = egui::Rect::from_center_size(
                egui::pos2(rect.right() - reorder_ui::GRIP - 14., rect.center().y),
                egui::vec2(30., 30.),
            );
            let hot = response.hovered() && !list.is_held(i);
            let dots_response = ui.interact(dots, id.with("dots"), egui::Sense::click());
            if hot || dots_response.hovered() {
                let color = if dots_response.hovered() { Color32::WHITE } else { MUTED };
                ui.painter().text(dots.center(), egui::Align2::CENTER_CENTER, "•••", egui::FontId::proportional(14.), color);
            }
            let menu = |ui: &mut egui::Ui| {
                if ui.button("Open playlist").clicked() {
                    open = Some((row.key.clone(), row.name.clone()));
                    ui.close();
                }
                if ui.button("Change picture…").clicked() {
                    change_cover = Some(row.key.clone());
                    ui.close();
                }
                if row.custom && ui.button("Remove picture").clicked() {
                    remove_cover = Some(row.key.clone());
                    ui.close();
                }
                ui.separator();
                if ui
                    .button(RichText::new("Delete playlist").color(Color32::from_rgb(255, 170, 178)))
                    .clicked()
                {
                    delete = Some(i);
                    ui.close();
                }
            };
            egui::Popup::menu(&dots_response).show(menu);
            response.context_menu(|ui| {
                menu_style(ui);
                if ui.button("Open playlist").clicked() {
                    open = Some((row.key.clone(), row.name.clone()));
                    ui.close();
                }
                if ui.button("Change picture…").clicked() {
                    change_cover = Some(row.key.clone());
                    ui.close();
                }
                if row.custom && ui.button("Remove picture").clicked() {
                    remove_cover = Some(row.key.clone());
                    ui.close();
                }
                ui.separator();
                if ui
                    .button(RichText::new("Delete playlist").color(Color32::from_rgb(255, 170, 178)))
                    .clicked()
                {
                    delete = Some(i);
                    ui.close();
                }
            });
        }
        if let Some(id) = change_cover {
            self.roy_account.choose_playlist_cover(ui.ctx(), &id);
        }
        if let Some(id) = remove_cover {
            self.roy_account.remove_playlist_cover(&id);
        }
        let reordered = list.finish();
        let keys: Vec<String> = rows.iter().map(|r| r.key.clone()).collect();
        if let Some(order) = reordered {
            let ids: Vec<String> = order.into_iter().map(|i| keys[i].clone()).collect();
            self.pending_playlist_order = Some((ids.clone(), Instant::now()));
            self.send("playlist-order", "playlist.reorder", json!({"ids":ids}));
        }
        if let Some((id, name)) = open {
            self.active_playlist = Some(id);
            self.rename_playlist = name;
            self.navigate(Page::Playlists);
        }
        if let Some(i) = delete {
            self.confirm_delete_playlist(&rows[i].key, &rows[i].name, rows[i].count, rows[i].cover.clone(), i);
        }
    }

    /// Asks first, then removes the playlist; its tile fades and collapses instead of vanishing.
    fn confirm_delete_playlist(&mut self, id: &str, name: &str, count: usize, cover: Option<String>, index: usize) {
        let sure = rfd::MessageDialog::new()
            .set_title("Delete playlist?")
            .set_description(format!("“{name}” will be removed. Songs stay in your library."))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            == rfd::MessageDialogResult::Yes;
        if !sure {
            return;
        }
        self.ghosts.push(reorder_ui::Ghost {
            list: "playlists".into(),
            key: id.to_owned(),
            index,
            at: Instant::now(),
            snapshot: reorder_ui::Snapshot::Playlist { name: name.to_owned(), count, cover },
        });
        self.send("playlist", "playlist.delete", json!({"id":id}));
        if self.active_playlist.as_deref() == Some(id) {
            self.active_playlist = None;
        }
    }

    fn save_new_playlist(&mut self) {
        self.send(
            "playlist-created",
            "playlist.save",
            json!({"name":self.playlist_name.trim(),"tracks":self.selected}),
        );
        self.playlist_name.clear();
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
    #[cfg(windows)]
    fn window_caption(&mut self, ui: &mut egui::Ui, search: egui::Rect, left: f32) {
        let ctx = ui.ctx().clone();
        let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
        let header = ui.max_rect();
        let controls = egui::Rect::from_min_max(
            egui::pos2(header.right() - 120., header.top()),
            header.right_bottom(),
        );
        let drag_regions = [
            egui::Rect::from_min_max(
                egui::pos2(left, header.top()),
                egui::pos2(search.left() - 97., header.bottom()),
            ),
            egui::Rect::from_min_max(
                egui::pos2(search.right() + 5., header.top()),
                controls.left_bottom(),
            ),
        ];
        for (index, rect) in drag_regions.into_iter().enumerate() {
            if rect.width() <= 0. {
                continue;
            }
            let response = ui.interact(
                rect,
                ui.id().with(("caption-drag", index)),
                egui::Sense::click_and_drag(),
            );
            if response.double_clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            } else if response.drag_started_by(egui::PointerButton::Primary) {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
        }
        let badge = egui::Rect::from_center_size(
            egui::pos2(controls.left() - 24., controls.center().y),
            egui::vec2(32., 32.),
        );
        let account = roy_account_ui::badge(ui, &self.roy_account, badge);
        let signed_out = self.roy_account.is_signed_out();
        let label = if signed_out {
            "Roynit ID".to_owned()
        } else {
            self.roy_account.name().to_owned()
        };
        egui::Popup::menu(&account.clone().on_hover_text(label.clone())).show(|ui| {
            ui.set_width(190.);
            if !signed_out {
                ui.label(RichText::new(&label).bold());
                ui.add_space(2.);
            }
            if self
                .nav_item(ui, "👤", if signed_out { "Sign in" } else { "Profile" }, self.page == Page::Profile)
                .clicked()
            {
                self.navigate(Page::Profile);
                ui.close();
            }
            if self.nav_item(ui, "⚙", "Settings", self.settings_open).clicked() {
                self.live_search_at = None;
                self.settings_query.clear();
                self.settings_group = 0;
                self.open_settings();
                ui.close();
            }
            if !signed_out {
                ui.separator();
                if self.nav_item(ui, "↪", "Sign out", false).clicked() {
                    self.roy_account.sign_out();
                    ui.close();
                }
            }
        });
        let mut buttons = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(controls)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        buttons.spacing_mut().item_spacing.x = 0.;
        for (index, title, command) in [
            (0, "Minimize", egui::ViewportCommand::Minimized(true)),
            (
                1,
                if maximized { "Restore" } else { "Maximize" },
                egui::ViewportCommand::Maximized(!maximized),
            ),
            (2, "Close", egui::ViewportCommand::Close),
        ] {
            let (rect, response) =
                buttons.allocate_exact_size(egui::vec2(40., 40.), egui::Sense::click());
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
            let painter = buttons.painter();
            let center = rect.center();
            let color = if response.hovered() && index == 2 {
                Color32::WHITE
            } else {
                buttons.visuals().text_color()
            };
            let stroke = egui::Stroke::new(1.2_f32, color);
            if response.hovered() || response.has_focus() {
                let fill = if index == 2 && response.hovered() {
                    Color32::from_rgb(175, 44, 49)
                } else {
                    buttons.visuals().widgets.hovered.bg_fill
                };
                painter.rect_filled(rect.shrink(2.), 7, fill);
            }
            let line = |x1, y1, x2, y2| {
                painter.line_segment(
                    [center + egui::vec2(x1, y1), center + egui::vec2(x2, y2)],
                    stroke,
                );
            };
            match index {
                0 => line(-5., 1., 5., 1.),
                1 if maximized => {
                    line(-2., -5., 5., -5.);
                    line(5., -5., 5., 2.);
                    painter.rect_stroke(
                        egui::Rect::from_center_size(
                            center + egui::vec2(-1.5, 1.5),
                            egui::vec2(8., 8.),
                        ),
                        0,
                        stroke,
                        egui::StrokeKind::Inside,
                    );
                }
                1 => {
                    painter.rect_stroke(
                        egui::Rect::from_center_size(center, egui::vec2(10., 10.)),
                        0,
                        stroke,
                        egui::StrokeKind::Inside,
                    );
                }
                _ => {
                    line(-4.5, -4.5, 4.5, 4.5);
                    line(4.5, -4.5, -4.5, 4.5);
                }
            }
            if response.on_hover_text(title).clicked() {
                if index == 2 && self.close_to_tray && !self.shutdown.load(Ordering::Relaxed) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                } else {
                    ctx.send_viewport_cmd(command);
                }
            }
        }
    }

    #[cfg(windows)]
    fn window_resize_edges(ctx: &egui::Context) {
        use egui::viewport::ResizeDirection::*;
        if ctx.input(|input| input.viewport().maximized.unwrap_or(false)) {
            return;
        }
        let bounds = ctx.content_rect();
        let edge = 5.;
        let corner = 11.;
        let (pointer, pressed) = ctx.input(|input| {
            (
                input.pointer.hover_pos(),
                input.pointer.button_pressed(egui::PointerButton::Primary),
            )
        });
        let Some(pointer) = pointer else { return };
        for (direction, cursor, min, max) in [
            (
                North,
                egui::CursorIcon::ResizeVertical,
                egui::pos2(bounds.left() + corner, bounds.top()),
                egui::pos2(bounds.right() - corner, bounds.top() + edge),
            ),
            (
                South,
                egui::CursorIcon::ResizeVertical,
                egui::pos2(bounds.left() + corner, bounds.bottom() - edge),
                egui::pos2(bounds.right() - corner, bounds.bottom()),
            ),
            (
                West,
                egui::CursorIcon::ResizeHorizontal,
                egui::pos2(bounds.left(), bounds.top() + corner),
                egui::pos2(bounds.left() + edge, bounds.bottom() - corner),
            ),
            (
                East,
                egui::CursorIcon::ResizeHorizontal,
                egui::pos2(bounds.right() - edge, bounds.top() + corner),
                egui::pos2(bounds.right(), bounds.bottom() - corner),
            ),
            (
                NorthWest,
                egui::CursorIcon::ResizeNwSe,
                bounds.left_top(),
                bounds.left_top() + egui::vec2(corner, corner),
            ),
            (
                NorthEast,
                egui::CursorIcon::ResizeNeSw,
                bounds.right_top() - egui::vec2(corner, 0.),
                bounds.right_top() + egui::vec2(0., corner),
            ),
            (
                SouthWest,
                egui::CursorIcon::ResizeNeSw,
                bounds.left_bottom() - egui::vec2(0., corner),
                bounds.left_bottom() + egui::vec2(corner, 0.),
            ),
            (
                SouthEast,
                egui::CursorIcon::ResizeNwSe,
                bounds.right_bottom() - egui::vec2(corner, corner),
                bounds.right_bottom(),
            ),
        ] {
            if egui::Rect::from_min_max(min, max).contains(pointer) {
                ctx.set_cursor_icon(cursor);
                if pressed {
                    ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
                }
                break;
            }
        }
    }

    fn search_bar(&mut self, ctx: &egui::Context) {
        // Match modifiers on the key event itself; a fast Ctrl+K can release Ctrl within the same frame.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::K)) {
            ctx.memory_mut(|m| m.request_focus(egui::Id::new("search-query")));
        }
        let mut anchor = None;
        self.update_banner(ctx);
        for name in std::mem::take(&mut self.artist_pic_queue) {
            self.send(&format!("artistpic/{name}"), "job.submit", json!({"operation":"music.search","params":{"query":name,"filter":"artists"}}));
        }
        egui::TopBottomPanel::top("search_bar")
            .frame(
                egui::Frame::new()
                    .fill(Self::chrome(ctx))
                    .inner_margin(egui::Margin {
                        left: 12,
                        right: 16,
                        top: 8,
                        bottom: 8,
                    }),
            )
            .show(ctx, |ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), 42.),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let edge = ui.cursor().left();
                        self.app_menu(ui);
                        ui.label(
                            RichText::new("ROY MUSIC")
                                .size(13.)
                                .bold()
                                .extra_letter_spacing(2.),
                        );
                        #[cfg(windows)]
                        let caption_left = ui.cursor().left() + 5.;
                        let field_width = (ctx.content_rect().width() * 0.34).clamp(310., 435.);
                        let group_width = field_width + 64. + 48.;
                        let start = (ctx.content_rect().center().x - group_width / 2.)
                            .max(edge + Self::LIBRARY_WIDTH);
                        ui.add_space((start - ui.cursor().left()).max(0.));
                        self.friends_button(ui);
                        self.home_button(ui);
                        let pill = egui::Rect::from_min_size(
                            egui::pos2(ui.cursor().left(), ui.max_rect().center().y - 20.),
                            egui::vec2(field_width + 14., 40.),
                        );
                        anchor = Some(pill);
                        let mut pill_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .id_salt("search-pill")
                                .max_rect(pill)
                                .layout(egui::Layout::top_down(egui::Align::Min)),
                        );
                        egui::Frame::new()
                            .fill(ui.visuals().extreme_bg_color)
                            .stroke(egui::Stroke::new(1.0_f32, Color32::from_white_alpha(16)))
                            .corner_radius(22)
                            .inner_margin(egui::Margin::symmetric(6, 2))
                            .show(&mut pill_ui, |ui| {
                                ui.set_width(field_width);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.;
                                    if Self::icon_button(ui, "🔍", "Search", false).clicked() {
                                        self.submit_search();
                                    }
                                    // Reserve the trailing slots up front so spinner/clear never move the text.
                                    let trailing = 70.;
                                    let response = ui.add(
                                        egui::TextEdit::singleline(&mut self.query)
                                            .id(egui::Id::new("search-query"))
                                            .frame(false)
                                            .desired_width(
                                                (ui.available_width() - trailing).max(60.),
                                            )
                                            .hint_text("Search music"),
                                    );
                                    if response.changed() {
                                        if self.page != Page::Search {
                                            self.search_filter = "all".into();
                                            self.search_source = 0;
                                        }
                                        self.schedule_live_search();
                                    } else if response.gained_focus()
                                        && self.query.trim().chars().count() >= 2
                                    {
                                        self.search_open = true;
                                    }
                                    if response.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                    {
                                        self.submit_search();
                                    }
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if !self.query.is_empty()
                                                && Self::icon_button(ui, "×", "Clear search", false)
                                                    .clicked()
                                            {
                                                self.query.clear();
                                                self.schedule_live_search();
                                                ui.ctx().memory_mut(|m| {
                                                    m.request_focus(egui::Id::new("search-query"))
                                                });
                                            }
                                            if self.search_pending > 0
                                                || self.live_search_at.is_some()
                                            {
                                                ui.add(egui::Spinner::new().size(14.).color(MUTED));
                                            }
                                        },
                                    );
                                });
                            });
                        #[cfg(windows)]
                        self.window_caption(ui, pill, caption_left);
                    },
                );
                if let Some(deadline) = self.live_search_at {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        self.live_search_at = None;
                        self.search_current();
                    } else {
                        ui.ctx().request_repaint_after(remaining);
                    }
                }
            });
        if self.search_open
            && let Some(anchor) = anchor
        {
            self.search_popup(ctx, anchor);
        }
    }
    fn search_popup(&mut self, ctx: &egui::Context, anchor: egui::Rect) {
        let at = anchor.left_bottom() + egui::vec2(0., 5.);
        let height = (ctx.content_rect().bottom() - at.y - 16.).clamp(100., 334.);
        let loading = self.search_pending > 0 || self.live_search_at.is_some();
        let mut tracks = self.search_matches.clone();
        let query = self.query.trim().to_lowercase();
        tracks.sort_by_cached_key(|track| {
            let title = track.title.to_lowercase();
            let artist = track.artist.to_lowercase();
            if artist == query {
                0
            } else if title == query {
                1
            } else if title.starts_with(&query) {
                2
            } else {
                3
            }
        });
        let items: Vec<_> = self.search_items.iter().take(4).cloned().collect();
        let result_count = tracks.len().min(24) + items.len();
        let list_height = (result_count as f32 * 57.).min(height);
        let popup = egui::Area::new(egui::Id::new("quick-search"))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            .default_width(anchor.width())
            .show(ctx, |ui| {
                ui.set_width(anchor.width());
                egui::Frame::popup(ui.style())
                    .corner_radius(14)
                    .inner_margin(8)
                    .show(ui, |ui| {
                        ui.set_width(anchor.width() - 18.);
                        if tracks.is_empty() && items.is_empty() {
                            ui.label(
                                RichText::new(if loading {
                                    "Searching…"
                                } else {
                                    "No results"
                                })
                                .color(MUTED),
                            );
                            return;
                        }
                        egui::ScrollArea::vertical()
                            .min_scrolled_height(list_height)
                            .max_height(list_height)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                for (index, track) in tracks.iter().take(24).enumerate() {
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(ui.available_width(), 53.),
                                        egui::Sense::hover(),
                                    );
                                    let row_rect = rect.with_max_x(rect.right() - 38.);
                                    let mut row = ui.new_child(
                                        egui::UiBuilder::new()
                                            .max_rect(row_rect)
                                            .layout(egui::Layout::top_down(egui::Align::Min)),
                                    );
                                    if self
                                        .quick_search_row(
                                            &mut row,
                                            &track.title,
                                            &track.artist,
                                            &track.platform,
                                            track.cover.as_deref(),
                                        )
                                        .clicked()
                                    {
                                        self.play(track);
                                        self.search_open = false;
                                    }
                                    let saved = self.library.tracks.contains_key(&track.id);
                                    let label = if saved {
                                        "In your library"
                                    } else {
                                        "Add to library"
                                    };
                                    let action_rect = egui::Rect::from_center_size(
                                        rect.right_center() - egui::vec2(18., 0.),
                                        egui::vec2(34., 34.),
                                    );
                                    let mut action =
                                        ui.new_child(egui::UiBuilder::new().max_rect(action_rect));
                                    let button = Self::icon_button(
                                        &mut action,
                                        if saved { "" } else { "+" },
                                        label,
                                        saved,
                                    );
                                    if saved {
                                        let center = button.rect.center() - egui::vec2(0., 2.);
                                        let points = [(-5., 0.), (-1., 4.), (6., -4.)]
                                            .map(|(x, y)| center + egui::vec2(x, y));
                                        action.painter().add(egui::Shape::line(
                                            points.to_vec(),
                                            egui::Stroke::new(
                                                1.6_f32,
                                                ui.visuals().selection.bg_fill,
                                            ),
                                        ));
                                    } else if button.clicked() {
                                        self.send("add", "library.add", json!({"track":track}));
                                    }
                                    if index == 0
                                        && let Some(item) = items.first()
                                    {
                                        self.quick_search_collection(ui, item);
                                    }
                                }
                                for item in items.iter().skip(usize::from(!tracks.is_empty())) {
                                    self.quick_search_collection(ui, item);
                                }
                            });
                    });
            });
        let dismiss = ctx.input_mut(|input| {
            input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                || (input.pointer.any_pressed()
                    && input.pointer.interact_pos().is_some_and(|pos| {
                        !anchor.contains(pos) && !popup.response.rect.contains(pos)
                    }))
        });
        if dismiss {
            self.search_open = false;
        }
    }

    fn quick_search_collection(&mut self, ui: &mut egui::Ui, item: &Value) {
        if self
            .quick_search_row(
                ui,
                item["title"].as_str().unwrap_or("Collection"),
                item["kind"].as_str().unwrap_or("Collection"),
                "ytmusic",
                item["cover"].as_str(),
            )
            .clicked()
        {
            self.open_search_collection(item);
        }
    }

    fn quick_search_row(
        &self,
        ui: &mut egui::Ui,
        title: &str,
        subtitle: &str,
        source: &str,
        cover: Option<&str>,
    ) -> egui::Response {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 53.), egui::Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
        if response.hovered() || response.has_focus() {
            ui.painter()
                .rect_filled(rect, 8, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(egui::vec2(5., 0.)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        self.artwork(&mut row, cover, 40., 1.);
        let width = row.available_width();
        row.vertical(|ui| {
            ui.set_width(width);
            ui.add(
                egui::Label::new(RichText::new(title).bold())
                    .truncate()
                    .selectable(false),
            );
            ui.horizontal(|ui| {
                Self::search_badge(ui, source);
                ui.add(
                    egui::Label::new(RichText::new(subtitle).small().color(MUTED))
                        .truncate()
                        .selectable(false),
                );
            });
        });
        response
    }

    fn search_filters(&mut self, ui: &mut egui::Ui) {
        if self.page == Page::Search {
            ui.add_space(6.);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.;
                for (kind, label) in [
                    ("all", "All"),
                    ("songs", "Songs"),
                    ("artists", "Artists"),
                    ("albums", "Albums"),
                    ("playlists", "Playlists"),
                ] {
                    if Self::filter_chip(ui, label, self.search_filter == kind).clicked()
                        && self.search_filter != kind
                    {
                        self.search_filter = kind.into();
                        if !matches!(kind, "all" | "songs") {
                            self.search_source = 0;
                        }
                        self.search();
                    }
                }
            });
            if matches!(self.search_filter.as_str(), "all" | "songs") {
                ui.add_space(4.);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.;
                    for (index, label) in std::iter::once("All music")
                        .chain(Self::SEARCH_SOURCES)
                        .enumerate()
                    {
                        if Self::filter_chip(ui, label, self.search_source == index).clicked()
                            && self.search_source != index
                        {
                            self.search_source = index;
                            self.search();
                        }
                    }
                });
            }
            ui.add_space(4.);
        }
    }

    fn app_menu(&mut self, ui: &mut egui::Ui) {
        let menu = ui.menu_button(RichText::new("•••").size(18.), |ui| {
            menu_style(ui);
            ui.set_width(166.);
            if self
                .nav_item(ui, "👤", "Roynit ID", self.page == Page::Profile)
                .clicked()
            {
                self.navigate(Page::Profile);
                ui.close();
            }
            if self
                .nav_item(ui, "⚙", "Settings", self.settings_open)
                .clicked()
            {
                self.live_search_at = None;
                self.settings_query.clear();
                self.settings_group = 0;
                self.open_settings();
                ui.close();
            }
            if self.nav_item(ui, "↑", "Check for updates", false).clicked() {
                self.send("update", "updates.check", json!({}));
                ui.close();
            }
            if self
                .nav_item(ui, "+", "Import music", false)
                .clicked()
            {
                self.open_import();
                ui.close();
            }
            if self.nav_item(ui, "🎁", "Orbs shop and quests", self.shop_open).clicked() {
                self.open_shop();
                ui.close();
            }
            if self.nav_item(ui, "📈", "Listening stats", self.stats_open).clicked() {
                self.stats_open = true;
                ui.close();
            }
            ui.separator();
            if self
                .nav_item(ui, "↓", "Downloads", self.page == Page::Downloads)
                .clicked()
            {
                self.navigate(Page::Downloads);
                ui.close();
            }
        });
        menu.response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Roynit menu")
        });
        menu.response.on_hover_text("Roynit menu");
    }

    fn home_button(&mut self, ui: &mut egui::Ui) {
        let active = self.page == Page::Home;
        let (rect, response) = ui.allocate_exact_size(egui::vec2(42., 42.), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, "Home")
        });
        let center = rect.center();
        let accent = ui.visuals().selection.bg_fill;
        let visuals = ui.style().interact(&response);
        let fill = if active {
            accent.gamma_multiply(0.23)
        } else {
            visuals.bg_fill
        };
        let painter = ui.painter();
        painter.circle_filled(center, 20., fill);
        if response.has_focus() {
            painter.circle_stroke(center, 20., egui::Stroke::new(1.5_f32, accent));
        }
        let stroke = egui::Stroke::new(
            1.8_f32,
            if active {
                accent
            } else {
                ui.visuals().text_color()
            },
        );
        let points = [(-9., -1.), (0., -9.), (9., -1.)].map(|(x, y)| center + egui::vec2(x, y));
        painter.add(egui::Shape::line(points.to_vec(), stroke));
        let points = [
            (-6., -3.),
            (-6., 8.),
            (-2., 8.),
            (-2., 2.),
            (2., 2.),
            (2., 8.),
            (6., 8.),
            (6., -3.),
        ]
        .map(|(x, y)| center + egui::vec2(x, y));
        painter.add(egui::Shape::line(points.to_vec(), stroke));
        if response.on_hover_text("Home").clicked() {
            self.navigate(Page::Home);
        }
    }

    fn send_effects(&mut self) {
        self.send("effects", "player.effects", json!(self.effects));
    }
    /// Top of the area dialogs may dim: just below the title bar, which must keep working.
    fn dialog_top(&self) -> f32 {
        if self.card_rect == egui::Rect::NOTHING { 60. } else { self.card_rect.top() - 8. }
    }
    /// A centred dialog over a dimmed app. Returns true when it asked to close (Esc or a click outside).
    fn dialog(
        ctx: &egui::Context,
        top: f32,
        id: &str,
        closable: bool,
        add: impl FnOnce(&mut egui::Ui),
    ) -> bool {
        let frame = egui::Frame::popup(&ctx.style()).inner_margin(24).corner_radius(14);
        Self::dialog_with(ctx, top, id, closable, 165, frame, add)
    }
    fn dialog_with(
        ctx: &egui::Context,
        top: f32,
        id: &str,
        closable: bool,
        dim: u8,
        frame: egui::Frame,
        add: impl FnOnce(&mut egui::Ui),
    ) -> bool {
        let screen = ctx.content_rect();
        let area = egui::Rect::from_min_max(egui::pos2(screen.left(), top), screen.max);
        let mut close = false;
        egui::Area::new(egui::Id::new((id, "backdrop")))
            .order(egui::Order::Middle)
            .fixed_pos(area.min)
            .show(ctx, |ui| {
                ui.painter().rect_filled(area, 0, Color32::from_black_alpha(dim));
                if ui.allocate_rect(area, egui::Sense::click()).clicked() && closable {
                    close = true;
                }
            });
        egui::Area::new(egui::Id::new((id, "dialog")))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0., top / 2.))
            .show(ctx, |ui| {
                frame.show(ui, add);
            });
        if closable && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
        close
    }
    fn settings_dialog(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let screen = ctx.content_rect();
        let top = self.dialog_top();
        let width = (screen.width() - 150.).clamp(560., 1040.);
        let height = (screen.height() - top - 70.).clamp(320., 780.);
        let side = 232.0_f32.min(width * 0.32);
        let content = width - side - 16. - 64.;
        // Translucent glass: the app shows through, dimmed. (No real blur: the renderer has no backdrop filter.)
        let frame = egui::Frame::new()
            .fill(Color32::from_rgba_unmultiplied(20, 20, 23, 222))
            .corner_radius(26)
            .stroke(egui::Stroke::new(1.0_f32, Color32::from_white_alpha(20)))
            .shadow(egui::Shadow { offset: [0, 14], blur: 48, spread: 0, color: Color32::from_black_alpha(150) })
            .inner_margin(20);
        let mut closed = false;
        let mut to_profile = false;
        let asked = Self::dialog_with(ctx, top, "settings", true, 175, frame, |ui| {
            ui.set_min_size(egui::vec2(width, height));
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(side, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(side);
                        ui.set_height(height);
                        egui::Frame::new()
                            .fill(Color32::from_white_alpha(16))
                            .corner_radius(22)
                            .inner_margin(egui::Margin::symmetric(16, 9))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("🔍").color(MUTED));
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.settings_query)
                                            .hint_text("Search settings…")
                                            .frame(false)
                                            .desired_width(ui.available_width()),
                                    );
                                });
                            });
                        ui.add_space(12.);
                        ui.spacing_mut().item_spacing.y = 4.;
                        for (index, (icon, name, _)) in Self::SETTINGS_GROUPS.iter().enumerate() {
                            let active = self.settings_query.trim().is_empty() && self.settings_group == index;
                            if Self::settings_pill(ui, icon, name, active).clicked() {
                                self.settings_group = index;
                                self.settings_query.clear();
                            }
                        }
                        let spare = ui.available_height() - 52.;
                        if spare > 0. {
                            ui.add_space(spare);
                        }
                        ui.horizontal(|ui| {
                            let signed_out = self.roy_account.is_signed_out();
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(side, 48.),
                                egui::Sense::click(),
                            );
                            if response.hovered() {
                                ui.painter().rect_filled(rect, 24, Color32::from_white_alpha(14));
                            }
                            let badge = egui::Rect::from_center_size(
                                rect.left_center() + egui::vec2(26., 0.),
                                egui::vec2(34., 34.),
                            );
                            roy_account_ui::paint_avatar(ui, &self.roy_account, badge);
                            ui.painter().text(
                                rect.left_center() + egui::vec2(52., -9.),
                                egui::Align2::LEFT_CENTER,
                                if signed_out { "Sign in" } else { self.roy_account.name() },
                                egui::FontId::new(14., egui::FontFamily::Name("bold".into())),
                                Color32::WHITE,
                            );
                            ui.painter().text(
                                rect.left_center() + egui::vec2(52., 10.),
                                egui::Align2::LEFT_CENTER,
                                "Account",
                                egui::FontId::proportional(12.),
                                MUTED,
                            );
                            if response.clicked() {
                                to_profile = true;
                            }
                        });
                    },
                );
                ui.add_space(16.);
                ui.allocate_ui_with_layout(
                    egui::vec2(content, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(content);
                        egui::ScrollArea::vertical()
                            .max_height(height)
                            .auto_shrink([false, false])
                            .show(ui, |ui| self.settings_body(ui));
                    },
                );
                ui.vertical_centered(|ui| {
                    ui.set_width(48.);
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(44., 44.), egui::Sense::click());
                    let alpha = if response.hovered() { 255 } else { 190 };
                    let ink = Color32::from_white_alpha(alpha);
                    ui.painter().circle_stroke(rect.center(), 21., egui::Stroke::new(1.5_f32, ink));
                    for (a, b) in [((-6., -6.), (6., 6.)), ((6., -6.), (-6., 6.))] {
                        ui.painter().line_segment(
                            [rect.center() + egui::vec2(a.0, a.1), rect.center() + egui::vec2(b.0, b.1)],
                            egui::Stroke::new(1.8_f32, ink),
                        );
                    }
                    ui.label(RichText::new("ESC").size(11.).color(MUTED));
                    if response.on_hover_text("Close (Esc)").clicked() {
                        closed = true;
                    }
                });
            });
        });
        if asked || closed || to_profile {
            self.settings_open = false;
        }
        if to_profile {
            self.navigate(Page::Profile);
        }
    }
    fn open_settings(&mut self) {
        self.settings_open = true;
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
        egui::Frame::new()
            .fill(Color32::from_white_alpha(11))
            .corner_radius(14)
            .inner_margin(egui::Margin::symmetric(18, 14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if ui.available_width() < 480. {
                    ui.label(RichText::new(label).color(TEXT));
                    if !hint.is_empty() {
                        ui.label(RichText::new(hint).small().color(MUTED));
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
                    });
                } else {
                    ui.horizontal(|ui| {
                        let left = (ui.available_width() * 0.5).max(200.);
                        ui.allocate_ui_with_layout(
                            egui::vec2(left, 0.),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(left);
                                ui.label(RichText::new(label).color(TEXT));
                                if !hint.is_empty() {
                                    ui.add(
                                        egui::Label::new(RichText::new(hint).small().color(MUTED))
                                            .wrap(),
                                    );
                                }
                            },
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
                    });
                }
            });
        ui.add_space(6.);
    }
    fn group_title(ui: &mut egui::Ui, title: &str) {
        ui.add_space(18.);
        ui.label(RichText::new(title).size(20.).bold());
        ui.add_space(2.);
    }
    const SETTINGS_GROUPS: [(&'static str, &'static str, &'static str); 8] = [
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
            "+",
            "Import music",
            "import playlist link spotify youtube soundcloud apple deezer yandex tiktok vk favorites",
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
            "backup restore import export data folder diagnostics report install update version release rollback",
        ),
    ];
    /// The settings rows for the chosen group, or for every group that matches the search.
    fn settings_body(&mut self, ui: &mut egui::Ui) {
        let query = self.settings_query.clone();
        let searching = !query.trim().is_empty();
        let mut shown = 0;
        for (index, (_, name, keys)) in Self::SETTINGS_GROUPS.iter().enumerate() {
            let visible = if searching {
                Self::matches(&query, &[name, keys])
            } else {
                index == self.settings_group
            };
            if !visible {
                continue;
            }
            shown += 1;
            if searching {
                Self::group_title(ui, name);
            }
            let rows = if Self::matches(&query, &[name]) { String::new() } else { query.clone() };
            let q = rows.as_str();
            ui.push_id(name, |ui| match index {
                0 => self.settings_playback(ui, q),
                1 => self.settings_storage(ui, q),
                2 => self.settings_appearance(ui, q),
                3 => self.settings_accounts(ui, q),
                4 => self.settings_import(ui, q),
                5 => self.settings_network(ui, q),
                6 => self.settings_extensions(ui, q),
                _ => self.settings_data(ui, q),
            });
        }
        if shown == 0 {
            ui.add_space(24.);
            ui.label(RichText::new("No matching settings").size(18.));
        }
    }
    /// A full-width rounded navigation pill; the active one is white with dark text.
    fn settings_pill(ui: &mut egui::Ui, icon: &str, label: &str, active: bool) -> egui::Response {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.), egui::Sense::click());
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, label));
        let t = ui.ctx().animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);
        let fill = if active { Color32::WHITE } else { Color32::from_white_alpha((16. * t) as u8) };
        let ink = if active { Color32::BLACK } else { Color32::WHITE };
        let painter = ui.painter();
        painter.rect_filled(rect, 22, fill);
        painter.text(
            egui::pos2(rect.left() + 26., rect.center().y),
            egui::Align2::CENTER_CENTER,
            icon,
            egui::FontId::proportional(16.),
            ink,
        );
        painter.text(
            egui::pos2(rect.left() + 50., rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            if active {
                egui::FontId::new(15., egui::FontFamily::Name("bold".into()))
            } else {
                egui::FontId::proportional(15.)
            },
            ink,
        );
        response
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
                    menu_style(ui);
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
            let seconds = self.effects.crossfade_seconds;
            let (_, new) = Self::thin_bar(ui, 200., (seconds / 12.) as f32, true, "Crossfade", 0.5 / 12.);
            if let Some(fraction) = new {
                self.effects.crossfade_seconds = f64::from((fraction * 24.).round()) / 2.;
                self.effects_pending = Some(Instant::now());
            }
            ui.label(RichText::new(format!("{seconds:.1} s")).color(MUTED));
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
        if self.startup_supported {
            self.refresh_startup_state();
            let code = self.startup_state.load(Ordering::Relaxed);
            let mut on = code == 1;
            let hint = if code == 2 {
                "Turned off in Task Manager. Turn it on here to start with Windows again"
            } else {
                "Opens quietly in the tray when you sign in to Windows"
            };
            Self::row(ui, q, "Start with Windows", hint, "startup boot login task manager autostart", |ui| {
                if Self::toggle(ui, &mut on, "Start with Windows").changed() {
                    let state = self.startup_state.clone();
                    state.store(u8::from(on), Ordering::Relaxed);
                    self.startup_checked = Instant::now();
                    std::thread::spawn(move || {
                        if on {
                            if let Ok(exe) = std::env::current_exe() {
                                let _ = riff_core::startup::enable(&exe);
                            }
                        } else {
                            riff_core::startup::disable();
                        }
                        state.store(Self::startup_code(riff_core::startup::status()), Ordering::Relaxed);
                    });
                }
            });
        }
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
                self.show_fullscreen_lyrics(ui.ctx(), true);
            }
        });
        Self::row(
            ui,
            q,
            "Media engine",
            "FFmpeg decodes your music; Roy Music downloads a checked copy",
            "ffmpeg ffprobe install codec",
            |ui| {
                if ui.button("Install or update").clicked() {
                    self.job("tools", "tools.install", json!({"name":"ffmpeg"}));
                }
            },
        );
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
                        menu_style(ui);
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
                    menu_style(ui);
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
        Self::row(ui, q, "Visualizer", "", "wave waveform spectrum hidden", |ui| {
            let before = self.appearance.visualizer.clone();
            egui::ComboBox::from_id_salt("visualizer")
                .selected_text(match self.appearance.visualizer.as_str() {
                    "hidden" => "Hidden",
                    _ => "Wave",
                })
                .show_ui(ui, |ui| {
                    menu_style(ui);
                    for (value, label) in
                        [("waveform", "Wave"), ("hidden", "Hidden")]
                    {
                        ui.selectable_value(&mut self.appearance.visualizer, value.into(), label);
                    }
                });
            // Picking a visualizer is saved at once, so it is still there after a restart.
            if self.appearance.visualizer != before {
                self.send("appearance", "appearance.set", json!(self.appearance));
            }
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
                "Sign in with Google too; likes and playlists shape Mixes",
            )
        } else {
            ("spotify", "Spotify", "Import liked songs and playlists")
        };
        let linked = roy_account_ui::Provider::parse(key);
        let known = &self.account_status[key];
        let status = if known["signed_in"] == true {
            "Connected"
        } else if known["pending"] == true || linked.is_some_and(|p| self.roy_account.is_linking(p)) {
            "Waiting for sign-in"
        } else if known.is_null() {
            "Checking…"
        } else if linked.is_some_and(|p| self.roy_account.linked(p).is_some()) {
            "Linked · renewing"
        } else {
            "Not connected"
        };
        Self::row(ui, q, name, hint, "account sign in login oauth", |ui| {
            ui.menu_button("•••", |ui| {
                menu_style(ui);
                if ui.button("Check connection").clicked() {
                    self.send("account-status", "oauth.status", json!({"provider":key}));
                    ui.close();
                }
                if ui.button("Refresh session").clicked() {
                    match roy_account_ui::Provider::parse(key) {
                        Some(p) if self.roy_account.linked(p).is_some() => self.roy_account.connect(p),
                        _ => self.send("auth", "oauth.refresh", json!({"provider":key})),
                    }
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
                    match roy_account_ui::Provider::parse(key) {
                        // Unlinking drops the local token through the same path, so it cannot come back on its own.
                        Some(p) if self.roy_account.linked(p).is_some() => self.roy_account.unlink(p),
                        _ => self.send("logout", "oauth.logout", json!({"provider":key})),
                    }
                    ui.close();
                }
            });
            if status != "Connected"
                && ui
                    .button("Connect")
                    .on_hover_text("Links through your Roynit ID, or uses the client ID under Advanced")
                    .clicked()
            {
                self.oauth_provider = provider;
                if !self.oauth_client.trim().is_empty() {
                    self.login();
                } else if let Some(p) = roy_account_ui::Provider::parse(key)
                    && !self.roy_account.is_signed_out()
                {
                    self.roy_account.connect(p);
                } else {
                    self.status = "Sign in to Roynit ID first, then link your account here.".into();
                    self.navigate(Page::Profile);
                }
            }
            ui.label(
                RichText::new(status)
                    .small()
                    .color(if status == "Connected" { AQUA } else { MUTED }),
            );
        });
    }
    fn settings_accounts(&mut self, ui: &mut egui::Ui, q: &str) {
        if !self.roy_account.is_signed_out() {
            let username = format!("@{}", self.roy_account.username());
            Self::row(ui, q, "Username", &username, "username name change id", |ui| {
                if ui.button("Change…").clicked() {
                    self.roy_account.open_security();
                }
            });
            let has_password = self.roy_account.has_password();
            let hint = if has_password {
                "Sign in with your email or username and password"
            } else {
                "Not set. You sign in with Google"
            };
            Self::row(ui, q, "Password", hint, "password sign in security", |ui| {
                if ui.button(if has_password { "Change…" } else { "Set password" }).clicked() {
                    self.roy_account.open_security();
                }
            });
        }
        Self::row(
            ui,
            q,
            "YouTube Music session",
            "Optional cookie file for age-restricted or members-only tracks. Google / YouTube below does not need it",
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
                    menu_style(ui);
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
                            menu_style(ui);
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
                        menu_style(ui);
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
            "Import a Roy Music theme file",
            "theme import export",
            |ui| {
                if ui.button("Export").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .set_file_name("roy-music-theme.json")
                        .save_file()
                {
                    let value = json!({"id":"exported-theme","name":"My theme","tokens":self.appearance});
                    if let Err(e) = riff_core::store::write_json(&path, &value) {
                        self.error = Some(format!("Couldn't export theme: {e}"));
                    }
                }
                if ui.button("Import…").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("Roy Music theme", &["json"])
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
                    menu_style(ui);
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
                && let Some(path) = rfd::FileDialog::new().add_filter("Roy Music backup", &["json"]).pick_file()
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
                        .set_file_name("roy-music-library.json")
                        .save_file()
                {
                    self.send("backup", "backup.export", json!({"path":path}));
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
                    .set_file_name("roy-music-diagnostics.json")
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
                    menu_style(ui);
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
        let mut install = self.library.settings.get("auto_update_install") != Some(&json!(false));
        Self::row(
            ui,
            q,
            "Install updates automatically",
            "Downloads a new version in the background and installs it the next time you open Roy Music",
            "automatic update install",
            |ui| {
                if Self::toggle(ui, &mut install, "Install updates automatically").changed() {
                    self.send("preferences", "settings.set", json!({"auto_update_install":install}));
                }
            },
        );
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
        self.analysis.store(self.appearance.visualizer != "hidden", Ordering::Relaxed);
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
        style.spacing.menu_margin = egui::Margin::same(6);
        style.spacing.menu_spacing = 2.;
        style.animation_time = if self.appearance.reduced_motion {
            0.
        } else {
            0.18
        };
        for widget in [
            &mut style.visuals.widgets.inactive,
            &mut style.visuals.widgets.hovered,
            &mut style.visuals.widgets.active,
            &mut style.visuals.widgets.open,
        ] {
            widget.corner_radius = 6.into();
            widget.expansion = 0.;
        }
        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::NONE;
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
    /// A soft wave behind the whole player bar that swells with the music and settles flat when paused.
    fn wave(&mut self, ui: &egui::Ui, full: egui::Rect) {
        const POINTS: usize = 128;
        let ctx = ui.ctx().clone();
        let dt = ctx.input(|i| i.stable_dt).clamp(0.001, 0.1);
        let playing = self.player["paused"] == false;
        let bins: Vec<f32> = self.player["analysis"]["spectrum"]
            .as_array()
            .map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.).max(0.).sqrt().min(1.) as f32).collect())
            .unwrap_or_default();
        if self.viz_levels.len() != POINTS {
            self.viz_levels = vec![0.; POINTS];
        }
        let goal = if playing { 1. } else { 0. };
        self.viz_presence += (goal - self.viz_presence) * (1. - (-dt * 4.).exp());
        let mut target = vec![0_f32; POINTS];
        if playing && !bins.is_empty() {
            // Bass on the left. Upper bands carry less energy, so they are lifted before levelling.
            let raw: Vec<f32> = (0..POINTS)
                .map(|i| {
                    let position = i as f32 / (POINTS - 1) as f32;
                    let at = position.powf(1.7) * 0.9 * (bins.len() - 1) as f32;
                    let (low, high) = (at.floor() as usize, (at.ceil() as usize).min(bins.len() - 1));
                    (bins[low] + (bins[high] - bins[low]) * (at - low as f32)) * (1. + position * 2.6)
                })
                .collect();
            // Automatic gain: follow the loudest recent band so quiet and loud songs both fill the wave.
            let loudest = raw.iter().copied().fold(0., f32::max);
            self.viz_peak = (self.viz_peak * (1. - dt * 0.35)).max(loudest).max(0.04);
            for (value, level) in target.iter_mut().zip(&raw) {
                *value = (level / self.viz_peak).clamp(0., 1.).powf(1.35);
            }
            for _ in 0..4 {
                let copy = target.clone();
                for i in 0..POINTS {
                    let (a, b) = (copy[i.saturating_sub(1)], copy[(i + 1).min(POINTS - 1)]);
                    target[i] = (a + 2. * copy[i] + b) / 4.;
                }
            }
        }
        let mut moving = false;
        for (level, goal) in self.viz_levels.iter_mut().zip(&target) {
            let rate = if goal > level { 30. } else { 8. };
            *level += (goal - *level) * (1. - (-dt * rate).exp());
            moving |= (goal - *level).abs() > 0.003;
        }
        if playing || moving || (goal - self.viz_presence).abs() > 0.003 {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
        let ink = if ui.visuals().dark_mode { 255 } else { 0 };
        let (width, height) = (full.width(), full.height());
        let top_of = |i: usize| full.bottom() - (0.16 + 0.3 * self.viz_presence + 0.52 * self.viz_levels[i]) * height;
        let mut mesh = egui::epaint::Mesh::default();
        let mut edge = Vec::with_capacity(POINTS);
        for i in 0..POINTS {
            let x = full.left() + i as f32 / (POINTS - 1) as f32 * width;
            let y = top_of(i);
            edge.push(egui::pos2(x, y));
            mesh.colored_vertex(egui::pos2(x, y), Color32::from_rgba_unmultiplied(ink, ink, ink, 40));
            mesh.colored_vertex(egui::pos2(x, full.bottom()), Color32::from_rgba_unmultiplied(ink, ink, ink, 6));
        }
        for i in 0..POINTS as u32 - 1 {
            let k = i * 2;
            mesh.add_triangle(k, k + 1, k + 2);
            mesh.add_triangle(k + 1, k + 3, k + 2);
        }
        let painter = ui.painter().with_clip_rect(full);
        painter.add(egui::Shape::mesh(mesh));
        painter.add(egui::Shape::line(
            edge,
            egui::Stroke::new(1.2_f32, Color32::from_rgba_unmultiplied(ink, ink, ink, 50)),
        ));
    }
    fn reset_mixes(&mut self) {
        self.mix_generation = self.mix_generation.wrapping_add(1);
        self.mix_signature = None;
        self.mix_loading = false;
        self.mix_error = false;
        self.mixes.clear();
        self.suggested.clear();
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

    fn greeting() -> &'static str {
        use chrono::Timelike;
        match chrono::Local::now().hour() {
            5..=11 => "Good morning",
            12..=17 => "Good afternoon",
            _ => "Good evening",
        }
    }
    /// Colour wash behind the top of the page, like Spotify's: it follows what you hover and scrolls away.
    fn header_gradient(&self, ui: &egui::Ui) {
        if !ui.visuals().dark_mode || self.card_rect == egui::Rect::NOTHING {
            return;
        }
        let ctx = ui.ctx();
        let id = egui::Id::new("header-tint");
        let time = self.motion() * 2.;
        let channel = |n: u8, v: u8| ctx.animate_value_with_time(id.with(n), v as f32, time) as u8;
        let tint = Color32::from_rgb(
            channel(0, self.home_tint.r()),
            channel(1, self.home_tint.g()),
            channel(2, self.home_tint.b()),
        );
        let top = ui.min_rect().top();
        let painter = ui.painter().with_clip_rect(self.card_rect);
        let (left, right) = (self.card_rect.left(), self.card_rect.right());
        // One mesh, four colour stops (alpha 150 → 0 over 340 px): smooth, with no seams between bands.
        let mut mesh = egui::epaint::Mesh::default();
        for (offset, alpha) in [(0., 150), (110., 96), (230., 40), (340., 0)] {
            let color = Color32::from_rgba_unmultiplied(tint.r(), tint.g(), tint.b(), alpha);
            mesh.colored_vertex(egui::pos2(left, top + offset), color);
            mesh.colored_vertex(egui::pos2(right, top + offset), color);
        }
        for row in 0..3u32 {
            let i = row * 2;
            mesh.add_triangle(i, i + 1, i + 2);
            mesh.add_triangle(i + 1, i + 3, i + 2);
        }
        painter.add(egui::Shape::mesh(mesh));
        // The wash is square; black wedges give it the card's rounded top corners back.
        let r = PANEL_RADIUS as f32;
        for (corner, sx) in [
            (egui::pos2(left, self.card_rect.top()), 1.),
            (egui::pos2(right, self.card_rect.top()), -1.),
        ] {
            let center = corner + egui::vec2(sx * r, r);
            let mut wedge = vec![corner];
            for step in 0..=10 {
                let angle = -std::f32::consts::FRAC_PI_2 * (1. + step as f32 / 10.);
                wedge.push(center + egui::vec2(sx * r * angle.cos(), r * angle.sin()));
            }
            painter.add(egui::Shape::convex_polygon(
                wedge,
                Self::chrome(ui.ctx()),
                egui::Stroke::NONE,
            ));
        }
    }
    /// The jump-back-in grid: liked songs, your mixes and what you played last. One click starts it.
    fn quick_picks(&mut self, ui: &mut egui::Ui) {
        #[derive(Clone)]
        enum Go {
            Page(Page),
            Mix(usize),
            Track(Box<Track>),
        }
        struct Pick {
            title: String,
            cover: Option<String>,
            go: Go,
        }
        let mut picks: Vec<Pick> = Vec::new();
        if !self.library.favorites.is_empty() {
            picks.push(Pick {
                title: "Liked songs".into(),
                cover: None,
                go: Go::Page(Page::Favorites),
            });
        }
        for (i, mix) in self.mixes.iter().enumerate().take(3) {
            picks.push(Pick {
                title: mix.name.clone(),
                cover: mix.tracks.iter().find_map(|t| t.cover.clone()),
                go: Go::Mix(i),
            });
        }
        let mut seen = std::collections::HashSet::new();
        for entry in self.library.history.iter().rev() {
            if picks.len() >= 8 {
                break;
            }
            if let Some(track) = self.library.tracks.get(&entry.track_id)
                && seen.insert(entry.track_id.clone())
            {
                picks.push(Pick {
                    title: track.title.clone(),
                    cover: track.cover.clone(),
                    go: Go::Track(Box::new(track.clone())),
                });
            }
        }
        if picks.is_empty() {
            return;
        }
        let ctx = ui.ctx().clone();
        let width = ui.available_width() - 12.;
        let columns = (((width + 12.) / 252.).floor() as usize).clamp(2, 4);
        picks.truncate(if columns == 2 { 6 } else { 8 });
        let cell = (width - 12. * (columns as f32 - 1.)) / columns as f32;
        let mut hovered = None;
        let mut chosen = None;
        for row in picks.chunks(columns) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.;
                for pick in row {
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(cell, 56.), egui::Sense::click());
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            format!("Play {}", pick.title),
                        )
                    });
                    let play_id = response.id.with("play");
                    let hover = ctx.animate_bool_with_time(
                        response.id.with("h"),
                        response.hovered()
                            || response.has_focus()
                            || ctx.memory(|m| m.has_focus(play_id)),
                        self.motion(),
                    );
                    ui.painter().rect_filled(
                        rect,
                        6.,
                        Color32::from_white_alpha((20. + 22. * hover) as u8),
                    );
                    let art = egui::Rect::from_min_size(rect.min, egui::vec2(56., 56.));
                    let left_round = egui::CornerRadius {
                        nw: 6,
                        sw: 6,
                        ne: 0,
                        se: 0,
                    };
                    match &pick.cover {
                        Some(cover) => Self::paint_cover(ui, art, cover, left_round),
                        None => {
                            ui.painter().rect_filled(
                                art,
                                left_round,
                                Color32::from_rgb(80, 60, 190),
                            );
                            ui.painter().text(
                                art.center(),
                                egui::Align2::CENTER_CENTER,
                                "♥",
                                egui::FontId::proportional(22.),
                                Color32::WHITE,
                            );
                        }
                    }
                    {
                        let text = egui::Rect::from_min_max(
                            egui::pos2(art.right() + 12., rect.top()),
                            egui::pos2(rect.right() - 56., rect.bottom()),
                        );
                        let ui = &mut ui.new_child(
                            egui::UiBuilder::new()
                                .id_salt(("quick-title", &pick.title))
                                .max_rect(text)
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        );
                        ui.add(
                            egui::Label::new(RichText::new(&pick.title).bold().size(14.5))
                                .truncate()
                                .selectable(false),
                        );
                    }
                    let center = egui::pos2(rect.right() - 30., rect.center().y);
                    let mut go = response.clicked();
                    if !matches!(pick.go, Go::Page(_)) {
                        go |= self.reveal_play(
                            ui,
                            play_id,
                            center,
                            36.,
                            hover,
                            &format!("Play {}", pick.title),
                        );
                    }
                    if response.hovered() {
                        hovered = pick.cover.as_deref().and_then(|c| self.cover_glow(&ctx, c));
                    }
                    if go {
                        chosen = Some(pick.go.clone());
                    }
                }
            });
            ui.add_space(12.);
        }
        // The wash behind the page follows the hovered tile, or settles on the first one.
        let resting = picks
            .first()
            .and_then(|p| p.cover.as_deref())
            .and_then(|c| self.cover_glow(&ctx, c));
        self.home_tint = hovered
            .or(resting)
            .map_or(Color32::from_rgb(34, 74, 54), |c| {
                c.lerp_to_gamma(Color32::BLACK, 0.5)
            });
        match chosen {
            Some(Go::Page(page)) => self.navigate(page),
            Some(Go::Mix(i)) => {
                if let Some(mix) = self.mixes.get(i).cloned() {
                    self.send(
                        "play",
                        "mix.apply",
                        json!({"name":mix.name,"tracks":mix.tracks,"play":true}),
                    );
                }
            }
            Some(Go::Track(track)) => self.play(&track),
            None => {}
        }
        ui.add_space(8.);
    }

    fn personal_mixes(&mut self, ui: &mut egui::Ui) {
        self.load_mixes_if_needed();
        if !self.mixes.is_empty() {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Mixes for you").size(22.).bold());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.mix_loading {
                        ui.spinner();
                    } else if Self::icon_button(ui, "↻", "Refresh mixes", false).clicked() {
                        self.mix_signature = None;
                        ui.ctx().request_repaint();
                    }
                });
            });
        }
        let mixes = self.mixes.clone();
        let (columns, side) = Self::card_layout(ui.available_width() - 12., mixes.len(), 3);
        for chunk in mixes.chunks(columns) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.;
                for mix in chunk {
                    // Made-for-you and Discover often share a top song; Discover shows a later one so the cards differ.
                    let skip = if mix.name == riff_core::mixes::DISCOVER { 2 } else { 0 };
                    let cover = mix.tracks.iter().skip(skip).find_map(|t| t.cover.as_deref());
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
                        egui::pos2(rect.right() - 50., rect.top() + 18.),
                        egui::vec2(32., 28.),
                    );
                    let mut save = false;
                    let mut view = open;
                    let saved = self.library.playlists.values().any(|p| p.name == mix.name);
                    {
                        // A visible "add to my playlists" next to the options menu.
                        let plus = egui::Rect::from_min_size(
                            egui::pos2(rect.right() - 90., rect.top() + 18.),
                            egui::vec2(34., 28.),
                        );
                        let ui = &mut ui.new_child(egui::UiBuilder::new().max_rect(plus));
                        let label = if saved { "Already in your playlists" } else { "Add to your playlists" };
                        let button = ui.add_enabled(
                            !saved,
                            egui::Button::new(
                                RichText::new(if saved { "✓" } else { "+" }).size(18.).color(Color32::WHITE),
                            )
                            .fill(Color32::from_black_alpha(190))
                            .corner_radius(8)
                            .min_size(egui::vec2(34., 28.)),
                        );
                        if button.on_hover_text(label).on_disabled_hover_text(label).clicked() {
                            save = true;
                        }
                    }
                    {
                        let ui = &mut ui.new_child(egui::UiBuilder::new().max_rect(corner));
                        let menu =
                            ui.menu_button(RichText::new("•••").color(Color32::WHITE), |ui| {
                                menu_style(ui);
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
                    }
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
            ui.label(RichText::new("Recommended for you").size(22.).bold());
            self.rows(ui, self.suggested.clone());
        }
        ui.add_space(12.);
    }

    fn home_feed_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("From YouTube Music").size(22.).bold());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let menu = ui.menu_button("•••", |ui| {
                    menu_style(ui);
                    if ui.button("Charts").clicked() {
                        self.send("home", "music.charts", json!({}));
                        ui.close();
                    }
                    for (label, kind) in [
                        ("Radio from favorites", "favorites"),
                        ("Radio from listening history", "history"),
                    ] {
                        if ui.button(label).clicked() {
                            self.send("radio", "radio.personal", json!({"kind":kind}));
                            ui.close();
                        }
                    }
                    let warnings = self.mix_warnings.join("\n");
                    let hint = if self.mix_error {
                        "Mixes could not load. Try again."
                    } else {
                        warnings.as_str()
                    };
                    if ui.button("Refresh mixes").on_hover_text(hint).clicked() && !self.mix_loading
                    {
                        self.mix_signature = None;
                        ui.ctx().request_repaint();
                        ui.close();
                    }
                });
                menu.response.on_hover_text("Home options");
                if Self::icon_button(ui, "↻", "Refresh home", false).clicked() {
                    self.send("home", "music.home", json!({}));
                }
            });
        });
    }

    fn autoplay_toggle(&mut self, ui: &mut egui::Ui) {
        let mut autoplay = self.library.settings.get("autoplay") != Some(&json!(false));
        if Self::autoplay_switch(ui, &mut autoplay)
            .on_hover_text("Autoplay: add similar songs when the queue ends")
            .changed()
        {
            self.send("preferences", "settings.set", json!({"autoplay":autoplay}));
        }
    }

    fn search_badge(ui: &mut egui::Ui, platform: &str) {
        let (label, hint, color) = match platform {
            "ytmusic" => ("YT", "YouTube Music", Color32::from_rgb(255, 109, 121)),
            "youtube" => ("YT", "YouTube", Color32::from_rgb(255, 109, 121)),
            "spotify" => (
                "SP",
                "Spotify · plays a matched recording",
                Color32::from_rgb(92, 222, 146),
            ),
            "soundcloud" => ("SC", "SoundCloud", Color32::from_rgb(255, 155, 95)),
            _ => return,
        };
        ui.label(RichText::new(label).size(10.).bold().color(color))
            .on_hover_text(hint);
    }

    fn search_feature(&mut self, ui: &mut egui::Ui, top: &Track) {
        ui.label(RichText::new("Top result").size(22.).bold());
        ui.add_space(4.);
        egui::Frame::new()
            .fill(ui.visuals().widgets.inactive.bg_fill)
            .corner_radius(16)
            .inner_margin(13)
            .show(ui, |ui| {
                ui.set_width(ui.available_width().max(90.));
                self.artwork(ui, top.cover.as_deref(), 90., 1.);
                ui.add_space(4.);
                ui.add(egui::Label::new(RichText::new(&top.title).size(23.).bold()).truncate());
                ui.horizontal(|ui| {
                    Self::search_badge(ui, &top.platform);
                    ui.add(egui::Label::new(RichText::new(&top.artist).color(MUTED)).truncate());
                });
                if Self::icon_button(ui, "▶", &format!("Play {}", top.title), true).clicked() {
                    self.play(top);
                }
            });
    }

    fn search_preview_songs(&mut self, ui: &mut egui::Ui, tracks: &[Track]) {
        ui.label(RichText::new("Songs").size(22.).bold());
        ui.add_space(4.);
        for track in tracks {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 53.),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2((ui.available_width() - 45.).max(80.), 53.),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            if self
                                .quick_search_row(
                                    ui,
                                    &track.title,
                                    &track.artist,
                                    &track.platform,
                                    track.cover.as_deref(),
                                )
                                .clicked()
                            {
                                self.play(track);
                            }
                        },
                    );
                    ui.label(
                        RichText::new(if track.duration > 0. {
                            time(track.duration)
                        } else {
                            "—".into()
                        })
                        .small()
                        .color(MUTED),
                    );
                },
            );
        }
    }

    fn search_collection_shelf(&mut self, ui: &mut egui::Ui, title: &str, items: &[Value]) {
        ui.add_space(12.);
        ui.label(RichText::new(title).size(22.).bold());
        ui.add_space(4.);
        egui::ScrollArea::horizontal()
            .id_salt(("collection-shelf", title))
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    for item in items {
                        let title = item["title"].as_str().unwrap_or("Collection");
                        ui.allocate_ui_with_layout(
                            egui::vec2(140., 186.),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(140.);
                                let start = ui.cursor().min;
                                self.artwork(ui, item["cover"].as_str(), 140., 1.);
                                ui.add(egui::Label::new(RichText::new(title).bold()).truncate());
                                if let Some(subtitle) =
                                    item["subtitle"].as_str().filter(|text| !text.is_empty())
                                {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(subtitle).small().color(MUTED),
                                        )
                                        .truncate(),
                                    );
                                }
                                let response = ui.interact(
                                    egui::Rect::from_min_max(start, ui.min_rect().max),
                                    ui.id().with(item["id"].as_str()),
                                    egui::Sense::click(),
                                );
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title)
                                });
                                if response.on_hover_text(title).clicked() {
                                    self.open_search_collection(item);
                                }
                            },
                        );
                    }
                });
            });
    }

    fn search_content(&mut self, ui: &mut egui::Ui) {
        if self.query.trim().is_empty() {
            self.empty_state(ui);
            return;
        }
        let loading = self.search_pending > 0 || self.live_search_at.is_some();
        if self.search_matches.is_empty() && self.search_items.is_empty() {
            let text = if loading {
                "Searching…"
            } else if !self.search_failed.is_empty() {
                "No results from available sources"
            } else {
                "No results"
            };
            ui.label(RichText::new(text).color(MUTED));
        }
        let mut tracks = self.search_matches.clone();
        let query = self.query.trim().to_lowercase();
        let terms: Vec<_> = query.split_whitespace().collect();
        let relevance = |track: &Track| {
            let title = track.title.to_lowercase();
            let artist = track.artist.to_lowercase();
            if title == query || artist == query {
                0
            } else if title.contains(&query) || artist.contains(&query) {
                1
            } else if terms
                .iter()
                .all(|term| title.contains(term) || artist.contains(term))
            {
                2
            } else {
                3
            }
        };
        if tracks
            .iter()
            .filter(|track| relevance(track) < 3)
            .take(3)
            .count()
            == 3
        {
            tracks.retain(|track| relevance(track) < 3);
        }
        tracks.sort_by_cached_key(relevance);
        if self.search_filter == "all" && !tracks.is_empty() {
            let artist_query = tracks
                .iter()
                .filter(|track| track.artist.to_lowercase() == query)
                .count()
                >= 2;
            let top = tracks
                .iter()
                .min_by_key(|track| {
                    if artist_query && track.artist.to_lowercase() == query {
                        0
                    } else if track.title.to_lowercase() == query && !track.artist.is_empty() {
                        1
                    } else if track.title.to_lowercase() == query {
                        2
                    } else if track.duration < 900.
                        && track.title.to_lowercase().starts_with(&query)
                    {
                        3
                    } else {
                        4
                    }
                })
                .unwrap_or(&tracks[0]);
            let preview = tracks.len().min(4);
            if ui.available_width() >= 485. {
                let left = (ui.available_width() * 0.40).min(350.);
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(left, 240.),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| self.search_feature(ui, top),
                    );
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), 240.),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| self.search_preview_songs(ui, &tracks[..preview]),
                    );
                });
            } else {
                self.search_feature(ui, top);
                ui.add_space(12.);
                self.search_preview_songs(ui, &tracks[..preview]);
            }
            if preview < tracks.len() {
                ui.add_space(12.);
                self.rows(ui, tracks[preview..].to_vec());
            }
        } else if !tracks.is_empty() {
            ui.label(RichText::new("Songs").size(22.).bold());
            self.rows(ui, tracks);
        }
        for (kind, label) in [
            ("album", "Albums"),
            ("playlist", "Playlists"),
            ("podcast", "Podcasts"),
            ("artist", "Artists"),
        ] {
            let items: Vec<_> = self
                .search_items
                .iter()
                .filter(|item| item["kind"] == kind)
                .take(16)
                .cloned()
                .collect();
            if !items.is_empty() {
                self.search_collection_shelf(ui, label, &items);
            }
        }
        if !loading && !self.search_failed.is_empty() {
            let names = self
                .search_failed
                .iter()
                .map(|index| Self::SEARCH_SOURCES[*index])
                .collect::<Vec<_>>();
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!("Unavailable · {}", names.join(", ")))
                        .small()
                        .color(MUTED),
                );
                if ui.small_button("Retry").clicked() {
                    self.search_current();
                }
            });
        }
        if !loading
            && matches!(self.search_source, 3 | 4)
            && self.search_matches.len() >= self.search_limit
            && self.search_limit < 100
            && ui.button("Load more").clicked()
        {
            self.search_limit = (self.search_limit + 20).min(100);
            self.search_current();
        }
    }

    fn content(&mut self, ui: &mut egui::Ui) {
        if self.page == Page::Profile && !self.roy_account.is_signed_out() {
            self.home_tint = roy_account_ui::tint_for(self.roy_account.name());
        }
        if matches!(self.page, Page::Home | Page::Browse | Page::Profile) {
            self.header_gradient(ui);
        }
        ui.add_space(12.);
        if !matches!(self.page, Page::Browse | Page::Search | Page::Profile | Page::Friends) {
            let title = if self.page == Page::Home {
                Self::greeting()
            } else {
                self.page.title()
            };
            ui.label(RichText::new(title).size(32.).bold());
            ui.add_space(6.);
        }
        match self.page {
            Page::Profile => self.roynit_profile(ui),
            Page::Friends => self.friends_page(ui),
            Page::Search => self.search_content(ui),
            Page::Home | Page::Browse => {
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
                    self.quick_picks(ui);
                    self.personal_mixes(ui);
                    if self.page != Page::Home {
                        return;
                    }
                    self.home_feed_header(ui);
                }
                self.catalog(ui, self.items.iter().take(12).cloned().collect());
                if self.page != Page::Home || !self.tracks.is_empty() {
                    self.rows(ui, self.tracks.clone());
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
                    ui.label(RichText::new(label).size(18.).bold());
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
                if self
                    .active_playlist
                    .as_ref()
                    .is_none_or(|id| !self.library.playlists.contains_key(id))
                {
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
                            self.save_new_playlist();
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.import_url)
                                .id(egui::Id::new("playlist-import-url"))
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
                }
                let playlists = self.library.playlists.values().cloned().collect::<Vec<_>>();
                if playlists.is_empty() {
                    ui.add_space(16.);
                    ui.label(RichText::new("No playlists yet").size(20.));
                }
                ui.add_space(6.);
                ui.add_space(8.);
                if let Some(playlist) = self
                    .active_playlist
                    .as_ref()
                    .and_then(|id| self.library.playlists.get(id))
                    .cloned()
                {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&playlist.name).size(20.).bold());
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
                                    Self::primary_button(RichText::new("▶  Play"), accent, on_accent),
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
                            menu_style(ui);
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
                    self.lyric_mode(ui);
                    egui::ComboBox::from_id_salt("lyric-source")
                        .selected_text(format!("Source: {}", self.lyric_source))
                        .show_ui(ui, |ui| {
                            menu_style(ui);
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
                    self.lyric_lines.clear();
                    self.lyric_text.clear();
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
                            .bold(),
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
            Page::Settings => {}
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // eframe shows the window after its first frame; a quiet start hides it again straight away.
        if self.quiet_frames > 0 {
            self.quiet_frames -= 1;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        if self.quit_for_update {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
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
        self.roy_account.tick(&self.library);
        for event in self.roy_account.take_events() {
            match event {
                roy_account_ui::Event::Adopt { provider, token, expires_in, fresh } => self.send(
                    if fresh { "auth" } else { "adopted" },
                    "oauth.adopt",
                    json!({"provider":provider.key(),"access_token":token,"expires_in":expires_in}),
                ),
                roy_account_ui::Event::Drop(provider) => {
                    self.send("logout", "oauth.logout", json!({"provider":provider.key()}))
                }
                roy_account_ui::Event::Reset => {
                    self.send("account-reset", "account.music.reset", json!({}))
                }
                roy_account_ui::Event::PlaylistCover { id, version } => {
                    self.send("playlist", "playlist.cover", json!({"id":id,"cover":version}))
                }
                roy_account_ui::Event::Merge(snapshot) => self.send(
                    "account-merged",
                    "account.music.merge",
                    json!({"snapshot":snapshot}),
                ),
            }
        }
        self.roynit_gate(ctx);
        self.security_dialog(ctx);
        if self.roy_account.is_editing() {
            self.profile_dialog(ctx);
        }
        if !self.roy_account.needs_sign_in() {
            self.settings_dialog(ctx);
            self.import_dialog(ctx);
            self.stats_dialog(ctx);
            self.shop_dialog(ctx);
            self.birthday_dialog(ctx);
        }
        if let Some((message, failed)) = self.roy_account.take_link_notice() {
            if failed {
                self.error = Some(message);
            } else {
                self.status = message;
            }
        }
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
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let f11 = ctx.input(|i| i.key_pressed(egui::Key::F11));
        if self.fullscreen_lyrics && (escape || f11) {
            self.show_fullscreen_lyrics(ctx, false);
        } else if f11 || (escape && self.fullscreen) {
            self.fullscreen = !self.fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
        }
        if !ctx.wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.send("toggle", "player.toggle", json!({}));
        }
        ctx.request_repaint_after(Duration::from_millis(80));
        if self.fullscreen_lyrics {
            self.fullscreen_lyrics_view(ctx);
            return;
        }
        if self.compact {
            if escape && !ctx.wants_keyboard_input() {
                Self::set_mini(ctx, &mut self.compact, false);
            }
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
                            width - 112.,
                            52.,
                        );
                        if ui.button("Full player").on_hover_text("Back to the full window (Esc)").clicked() {
                            Self::set_mini(ctx, &mut self.compact, false);
                        }
                    });
                    self.transport(ui, width, accent, on_accent);
                    self.timeline(ui, width);
                });
            return;
        }
        self.friends_tick();
        self.notify_tick();
        image_cache::prefetch(ctx, self.social.friends.iter().filter_map(|f| self.social.avatar_uri(&f.person)).chain(self.social.suggestions.iter().filter_map(|p| self.social.avatar_uri(p))));
        self.player_bar(ctx);
        self.search_bar(ctx);
        if self.page == Page::Friends {
            self.friends_side_panel(ctx);
        } else {
            self.side_panel(ctx);
        }
        self.navigation(ctx);
        let right: i8 = if self.side_visible { 20 } else { 24 };
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Self::chrome(ctx))
                    .inner_margin(egui::Margin {
                        left: 20,
                        right,
                        top: 0,
                        bottom: 4,
                    }),
            )
            .show(ctx, |ui| {
                // The card is the panel grown back over its padding, so content keeps a 16 px gutter.
                let card = ui.max_rect().expand2(egui::vec2(16., 0.));
                self.card_rect = card;
                // The Friends space is pure black, like the window chrome around it.
                let fill = if self.page == Page::Friends { Color32::BLACK } else { ui.visuals().panel_fill };
                ui.painter().rect_filled(card, PANEL_RADIUS, fill);
                // Pages ease in: a short fade and rise tells the eye the content changed.
                if self.page_seen != self.page as u8 {
                    self.page_seen = self.page as u8;
                    self.page_since = Instant::now();
                }
                let reveal = if self.motion() == 0. {
                    1.
                } else {
                    (self.page_since.elapsed().as_secs_f32() / 0.2).min(1.)
                };
                if reveal < 1. {
                    ctx.request_repaint();
                    let eased = 1. - (1. - reveal).powi(3);
                    ui.set_opacity(0.25 + 0.75 * eased);
                    ui.add_space((1. - eased) * 10.);
                }
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
                                        // A missing FFmpeg is the one error the app can fix on the spot.
                                        let missing_media = error.contains("Cannot launch")
                                            && (error.contains("ffmpeg")
                                                || error.contains("ffprobe"));
                                        if missing_media
                                            && ui.button("Install media engine").clicked()
                                        {
                                            self.error = None;
                                            self.status = "Installing the media engine…".into();
                                            self.job(
                                                "tools",
                                                "tools.install",
                                                json!({"name":"ffmpeg"}),
                                            );
                                        }
                                        if Self::icon_button(ui, "×", "Dismiss error", false)
                                            .clicked()
                                        {
                                            self.error = None;
                                        }
                                    });
                                });
                        });
                }
                if self.busy && !matches!(self.page, Page::Search | Page::Home) {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new(&self.status)
                                .small()
                                .color(ui.visuals().weak_text_color()),
                        );
                    });
                }
                self.search_filters(ui);
                let scroll_id = if self.page == Page::Search {
                    egui::Id::new((
                        "search-content",
                        self.query.trim(),
                        &self.search_filter,
                        self.search_source,
                    ))
                } else {
                    egui::Id::new(("content", self.page as u8))
                };
                egui::ScrollArea::vertical()
                    .id_salt(scroll_id)
                    .show(ui, |ui| self.content(ui));
            });
        #[cfg(windows)]
        Self::window_resize_edges(ctx);
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
/// Covers are asked for at the size they are drawn: Google's picture servers resize on request, and a 400 px copy
/// is a fraction of the original, so lists and cards fill in quickly. Every place that shows a cover uses this same
/// address, so each picture is downloaded once.
fn thumb(url: &str) -> String {
    if url.contains("googleusercontent.com") || url.contains("ggpht.com") {
        let slash = url.rfind('/').unwrap_or(0);
        let base = match url[slash..].rfind('=') {
            Some(at) if url[slash + at + 1..].starts_with(['w', 's']) => &url[..slash + at],
            _ => url,
        };
        return format!("{base}=w400-h400-l90-rj");
    }
    if url.contains("ytimg.com") {
        return url.replace("maxresdefault", "hqdefault").replace("sddefault", "hqdefault");
    }
    url.to_owned()
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
// Spotify's dark system: pure black chrome, #121212 panels floating on it, #181818 cards,
// #1F1F1F controls, white text with #B3B3B3 secondary and one green accent.
const GRAPHITE: Color32 = Color32::from_gray(0x12);
const ELEVATED: Color32 = Color32::from_gray(0x18);
const SURFACE: Color32 = Color32::from_gray(0x1F);
const TEXT: Color32 = Color32::WHITE;
const MUTED: Color32 = Color32::from_gray(0xB3);
const DEFAULT_ACCENT: Color32 = Color32::from_rgb(30, 215, 96);
const GREEN_HOVER: Color32 = Color32::from_rgb(59, 228, 119);
const PANEL_RADIUS: u8 = 8;
/// Real weights: egui's own `strong()` only changes colour, and Spotify's hierarchy is built on bold.
trait Weight {
    fn bold(self) -> Self;
}
impl Weight for RichText {
    fn bold(self) -> Self {
        self.family(egui::FontFamily::Name("bold".into()))
    }
}
const AQUA: Color32 = Color32::from_rgb(0x78, 0xDD, 0xDA);
const ARTWORK_FADE: f32 = 0.18;
/// Below this window width the side panel only opens on request from the player bar.
const SIDE_PANEL_MIN_WINDOW: f32 = 1080.;
/// Roomier rows for every menu and drop-down: a comfortable width, taller items and a little air between them.
pub(crate) fn menu_style(ui: &mut egui::Ui) {
    ui.set_min_width(200.);
    let spacing = ui.spacing_mut();
    spacing.button_padding = egui::vec2(12., 8.);
    spacing.item_spacing.y = 2.;
}
fn graphite_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.panel_fill = GRAPHITE;
    v.window_fill = SURFACE;
    v.faint_bg_color = Color32::from_gray(0x17);
    v.extreme_bg_color = Color32::from_gray(0x2A);
    v.code_bg_color = SURFACE;
    v.weak_text_color = Some(MUTED);
    v.window_stroke = egui::Stroke::new(1.0_f32, Color32::from_white_alpha(18));
    v.window_corner_radius = 14.into();
    v.menu_corner_radius = 10.into();
    // Menus and pop-ups float: a soft shadow and a faint edge lift them off the page.
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(150) };
    v.window_shadow = egui::epaint::Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha(160) };
    let line = egui::Stroke::new(1.0_f32, Color32::from_white_alpha(14));
    for (w, fill) in [
        (&mut v.widgets.noninteractive, GRAPHITE),
        (&mut v.widgets.inactive, SURFACE),
        (&mut v.widgets.hovered, Color32::from_gray(0x2A)),
        (&mut v.widgets.active, Color32::from_gray(0x33)),
        (&mut v.widgets.open, SURFACE),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.fg_stroke.color = TEXT;
        w.corner_radius = 6.into();
    }
    v.widgets.noninteractive.fg_stroke.color = TEXT;
    v.widgets.noninteractive.bg_stroke = line;
    v.widgets.inactive.bg_stroke = egui::Stroke::NONE;
    v.widgets.hovered.bg_stroke = egui::Stroke::NONE;
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
    let setup_download = std::env::current_exe().ok().is_some_and(|path| {
        path.file_stem()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                let name = name.to_ascii_lowercase();
                name.starts_with("roy-music-") && name.contains("-setup")
            })
    });
    if setup_download || arguments.iter().any(|a| a == "--install") {
        let pid = arguments
            .iter()
            .position(|a| a == "--wait-pid")
            .and_then(|at| arguments.get(at + 1))
            .and_then(|p| p.parse::<u32>().ok())
            .unwrap_or(0);
        let result = (|| -> anyhow::Result<()> {
            riff_core::installation::wait_for(pid)?;
            let installed = riff_core::installation::install(&std::env::current_exe()?)?;
            if setup_download && let Err(error) = riff_core::tools::ensure_ytdlp() {
                rfd::MessageDialog::new()
                        .set_title("Roy Music installed")
                        .set_description(format!(
                            "Roy Music was installed. The online playback helper could not be downloaded. Open Settings → Audio tools to try again.\n\n{error:#}"
                        ))
                        .set_level(rfd::MessageLevel::Warning)
                        .show();
            }
            if setup_download || arguments.iter().any(|a| a == "--launch") {
                let mut launch =
                    riff_core::tools::command(installed["path"].as_str().unwrap_or(""));
                if let Some(root) = arguments
                    .iter()
                    .position(|a| a == "--data-dir")
                    .and_then(|at| arguments.get(at + 1))
                {
                    launch.arg("--data-dir").arg(root);
                }
                if arguments.iter().any(|a| a == "--startup") {
                    launch.arg("--startup");
                }
                let _ = launch.spawn()?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("Installation failed: {error:#}");
            rfd::MessageDialog::new()
                .set_title("Roy Music installation failed")
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
    // One copy per data folder: a second launch wakes the first and leaves quietly. When Windows started the
    // app (`--startup`) there is nobody to show anything to, so it just leaves.
    let background = arguments.iter().any(|a| a == "--startup");
    OFFSCREEN.store(background, Ordering::Relaxed);
    let instance_key = riff_core::instance::key_for(&root);
    if riff_core::instance::claim(&instance_key) == riff_core::instance::Role::Second {
        if !background {
            riff_core::instance::wake_first(&instance_key);
        }
        return Ok(());
    }
    let image = image::load_from_memory(include_bytes!("../assets/icon-app.png"))
        .expect("embedded Roy Music icon")
        .into_rgba8();
    let (width, height) = image.dimensions();
    let icon = egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_icon(icon)
            .with_inner_size([1120., 700.])
            .with_decorations(!cfg!(windows))
            .with_visible(!background)
            .with_position(if background { [-32_000., -32_000.] } else { [20., 20.] })
            .with_min_inner_size([1000., 600.]),
        ..Default::default()
    };
    eframe::run_native(
        "Roy Music",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, root, background)))),
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
