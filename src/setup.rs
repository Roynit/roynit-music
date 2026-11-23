#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Roy Music Setup: checks what the PC lacks, installs the app and fetches missing helpers.
//! Run with `--pack <out> <files…>` to build a Setup from this program, `--uninstall` to remove.
use eframe::egui::{
    self, Align2, Color32, FontFamily, FontId, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2,
};
use riff_core::{
    installation, payload,
    prerequisites::{self, Check, Id, State},
    tools::Phase,
};
use std::{
    io::Write,
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

// Palette: the same near-black surfaces and single green accent as the player.
const BG: Color32 = Color32::from_rgb(0x12, 0x12, 0x12);
const ROW: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x1A);
const GREEN: Color32 = Color32::from_rgb(0x1E, 0xD7, 0x60);
const WHITE: Color32 = Color32::WHITE;
const MUTED: Color32 = Color32::from_rgb(0xB3, 0xB3, 0xB3);
const AMBER: Color32 = Color32::from_rgb(0xFF, 0xB8, 0x4D);
const RED: Color32 = Color32::from_rgb(0xF3, 0x72, 0x7F);
const WINDOW: Vec2 = vec2(640., 600.);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    App,
    Media,
    Online,
    Audio,
    Network,
    Disk,
}
impl Item {
    fn title(self) -> &'static str {
        match self {
            Item::App => "Roy Music",
            Item::Media => "Media engine",
            Item::Online => "Online playback",
            Item::Audio => "Audio output",
            Item::Network => "Internet connection",
            Item::Disk => "Disk space",
        }
    }
    fn blurb(self) -> &'static str {
        match self {
            Item::App => "Player, library and settings. No admin rights needed",
            Item::Media => "FFmpeg decodes and plays your music",
            Item::Online => "yt-dlp finds streams for YouTube Music",
            Item::Audio => "Speakers or headphones to play through",
            Item::Network => "Needed to download what is missing",
            Item::Disk => "Room for the media engine and your cache",
        }
    }
    fn id(self) -> Option<Id> {
        match self {
            Item::Media => Some(Id::MediaEngine),
            Item::Online => Some(Id::OnlinePlayback),
            Item::Audio => Some(Id::AudioOutput),
            Item::Network => Some(Id::Network),
            Item::Disk => Some(Id::DiskSpace),
            Item::App => None,
        }
    }
    fn from_id(id: Id) -> Item {
        match id {
            Id::MediaEngine => Item::Media,
            Id::OnlinePlayback => Item::Online,
            Id::AudioOutput => Item::Audio,
            Id::Network => Item::Network,
            Id::DiskSpace => Item::Disk,
        }
    }
    /// Share of the overall progress bar.
    fn weight(self) -> f32 {
        match self {
            Item::App => 0.15,
            Item::Media => 0.65,
            _ => 0.2,
        }
    }
}

#[derive(Clone, Debug)]
enum RowState {
    Waiting,
    Checking,
    Ready(String),
    Will(String),
    Missing(u32),
    Warning(String),
    Working { frac: Option<f32>, note: String },
    Done(String),
    Failed(String),
}

enum Event {
    Checked(Check),
    ChecksDone,
    Progress(Item, Option<f32>, String),
    Finished(Item, Result<String, String>),
    AllDone,
}

#[derive(Clone, Default)]
struct Args {
    uninstall: bool,
    silent: bool,
    log: Option<PathBuf>,
    app: Option<PathBuf>,
    desktop_shortcut: Option<bool>,
    no_launch: bool,
    /// Start as soon as the checks finish (unattended installs and UI tests).
    yes: bool,
    /// Update mode, as the in-app updater calls it: wait for the app to exit, then relaunch it.
    wait_pid: Option<u32>,
    launch: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Checking,
    Review,
    Installing,
    Done,
}

#[derive(Clone)]
struct Row {
    item: Item,
    state: RowState,
    appeared: Option<f64>,
    changed: f64,
}

struct Setup {
    args: Args,
    stage: Stage,
    stage_since: f64,
    rows: Vec<Row>,
    rx: mpsc::Receiver<Event>,
    tx: mpsc::Sender<Event>,
    desktop_shortcut: bool,
    remove_data: bool,
    installed: Option<String>,
    dir: PathBuf,
    animations: bool,
    started: Option<f64>,
    plan: Vec<Item>,
    launched: bool,
    last_check_at: f64,
}

/// Windows "Show animations" setting; setup follows it like any other app.
fn animations_enabled() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::SystemParametersInfoW;
        let mut on: i32 = 1;
        // SPI_GETCLIENTAREAANIMATION
        let ok = unsafe { SystemParametersInfoW(0x1042, 0, &mut on as *mut i32 as *mut _, 0) };
        return ok == 0 || on != 0;
    }
    #[allow(unreachable_code)]
    true
}

fn ease_out_cubic(t: f32) -> f32 {
    1. - (1. - t.clamp(0., 1.)).powi(3)
}
fn ease_out_back(t: f32) -> f32 {
    let t = t.clamp(0., 1.) - 1.;
    let c = 1.70158;
    1. + (c + 1.) * t.powi(3) + c * t.powi(2)
}
fn fade(c: Color32, a: f32) -> Color32 {
    c.gamma_multiply(a.clamp(0., 1.))
}
fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}
fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

fn stroke(width: f32, color: Color32) -> Stroke {
    Stroke::new(width, color)
}

fn arc(p: &egui::Painter, center: Pos2, r: f32, start: f32, sweep: f32, stroke: Stroke) {
    let n = ((sweep / 0.12).ceil() as usize).max(2);
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let a = start + sweep * i as f32 / n as f32;
            center + r * vec2(a.cos(), a.sin())
        })
        .collect();
    p.add(egui::Shape::line(pts, stroke));
}

/// Check mark that draws itself: `t` runs 0..1.
fn check_mark(p: &egui::Painter, c: Pos2, size: f32, t: f32, color: Color32) {
    let a = c + vec2(-0.36, 0.02) * size;
    let b = c + vec2(-0.1, 0.28) * size;
    let d = c + vec2(0.4, -0.26) * size;
    let stroke = stroke(size * 0.16, color);
    let first = (t / 0.4).clamp(0., 1.);
    p.line_segment([a, a + (b - a) * first], stroke);
    if t > 0.4 {
        let second = ((t - 0.4) / 0.6).clamp(0., 1.);
        p.line_segment([b, b + (d - b) * second], stroke);
    }
}

impl Setup {
    fn new(cc: &eframe::CreationContext<'_>, args: Args) -> Self {
        let ctx = &cc.egui_ctx;
        egui_extras::install_image_loaders(ctx);
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
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, "system".into());
        }
        let bold_first: Vec<String> = ["bold", "system"]
            .iter()
            .filter(|k| fonts.font_data.contains_key(**k))
            .map(|k| k.to_string())
            .chain(fonts.families[&FontFamily::Proportional].iter().cloned())
            .collect();
        fonts
            .families
            .insert(FontFamily::Name("bold".into()), bold_first);
        ctx.set_fonts(fonts);
        let (tx, rx) = mpsc::channel();
        let dir = installation::install_directory().unwrap_or_default();
        let installed = installation::installed_version(&dir);
        let mut app = Self {
            desktop_shortcut: args.desktop_shortcut.unwrap_or(true),
            remove_data: false,
            args,
            stage: Stage::Checking,
            stage_since: 0.,
            rows: Vec::new(),
            rx,
            tx,
            installed,
            dir,
            animations: animations_enabled(),
            started: None,
            plan: Vec::new(),
            launched: false,
            last_check_at: 0.,
        };
        app.begin();
        app
    }

    fn begin(&mut self) {
        self.rows.clear();
        let order: &[Item] = if self.args.uninstall {
            &[Item::App]
        } else {
            &[
                Item::App,
                Item::Media,
                Item::Online,
                Item::Audio,
                Item::Network,
            ]
        };
        for item in order {
            self.rows.push(Row {
                item: *item,
                state: RowState::Waiting,
                appeared: None,
                changed: 0.,
            });
        }
        self.stage = Stage::Checking;
        self.stage_since = 0.;
        let tx = self.tx.clone();
        let dir = self.dir.clone();
        let uninstall = self.args.uninstall;
        let wait_pid = self.args.wait_pid;
        std::thread::spawn(move || {
            if let Some(pid) = wait_pid {
                // The updating app is still shutting down; its files are locked until it exits.
                let _ = installation::wait_for(pid);
            }
            if !uninstall {
                for check in prerequisites::check_all(&dir) {
                    let _ = tx.send(Event::Checked(check));
                    // Long enough for the spinner to register; nothing is slower on purpose after this.
                    std::thread::sleep(Duration::from_millis(240));
                }
            }
            let _ = tx.send(Event::ChecksDone);
        });
    }

    fn row_mut(&mut self, item: Item) -> Option<&mut Row> {
        self.rows.iter_mut().find(|r| r.item == item)
    }
    fn set(&mut self, item: Item, state: RowState, now: f64) {
        if let Some(r) = self.row_mut(item) {
            r.state = state;
            r.changed = now;
            r.appeared.get_or_insert(now);
        }
    }
    fn state(&self, item: Item) -> Option<&RowState> {
        self.rows.iter().find(|r| r.item == item).map(|r| &r.state)
    }

    fn pump(&mut self, now: f64) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Checked(check) => {
                    let item = Item::from_id(check.id);
                    let state = match check.state {
                        State::Ready(v) => RowState::Ready(v),
                        State::Missing { download_mb } => RowState::Missing(download_mb),
                        State::Warning(w) => RowState::Warning(w),
                    };
                    // Disk space only earns a row when there is something to say.
                    if item == Item::Disk {
                        if matches!(state, RowState::Warning(_)) {
                            self.rows.push(Row {
                                item,
                                state,
                                appeared: Some(now),
                                changed: now,
                            });
                        }
                    } else {
                        self.set(item, state, now);
                    }
                    // The next row starts spinning right away.
                    if let Some(next) = self
                        .rows
                        .iter()
                        .position(|r| matches!(r.state, RowState::Waiting))
                    {
                        self.rows[next].state = RowState::Checking;
                        self.rows[next].appeared.get_or_insert(now);
                    }
                    self.last_check_at = now;
                }
                Event::ChecksDone => {
                    let app_state = if self.args.uninstall {
                        RowState::Will("Remove".into())
                    } else {
                        match &self.installed {
                            Some(v) if v == env!("CARGO_PKG_VERSION") => {
                                RowState::Will("Repair".into())
                            }
                            Some(v) => RowState::Will(format!(
                                "Update {v} → {}",
                                env!("CARGO_PKG_VERSION")
                            )),
                            None => {
                                RowState::Will(format!("Install {}", env!("CARGO_PKG_VERSION")))
                            }
                        }
                    };
                    self.set(Item::App, app_state, now);
                    self.stage = Stage::Review;
                    self.stage_since = now;
                }
                Event::Progress(item, frac, note) => {
                    self.set(item, RowState::Working { frac, note }, now);
                }
                Event::Finished(item, result) => {
                    let state = match result {
                        Ok(s) => RowState::Done(s),
                        Err(e) => RowState::Failed(e),
                    };
                    self.set(item, state, now);
                }
                Event::AllDone => {
                    self.stage = Stage::Done;
                    self.stage_since = now;
                }
            }
        }
    }

    fn missing(&self) -> Vec<(Item, u32)> {
        let online = !matches!(self.state(Item::Network), Some(RowState::Warning(_)));
        self.rows
            .iter()
            .filter_map(|r| match r.state {
                RowState::Missing(mb) if online => Some((r.item, mb)),
                _ => None,
            })
            .collect()
    }
    fn failed(&self) -> Vec<Item> {
        self.rows
            .iter()
            .filter(|r| matches!(r.state, RowState::Failed(_)))
            .map(|r| r.item)
            .collect()
    }

    fn start_install(&mut self, items: Vec<Item>, now: f64) {
        self.plan = items.clone();
        self.stage = Stage::Installing;
        self.stage_since = now;
        self.started.get_or_insert(now);
        for item in &items {
            self.set(
                *item,
                RowState::Working {
                    frac: Some(0.),
                    note: "Waiting".into(),
                },
                now,
            );
        }
        let tx = self.tx.clone();
        let args = self.args.clone();
        let shortcut = self.desktop_shortcut;
        let remove_data = self.remove_data;
        std::thread::spawn(move || {
            for item in items {
                let tx2 = tx.clone();
                let result = run_item(item, &args, shortcut, remove_data, &mut |frac, note| {
                    let _ = tx2.send(Event::Progress(item, frac, note));
                });
                let _ = tx.send(Event::Finished(item, result.map_err(|e| format!("{e:#}"))));
            }
            let _ = tx.send(Event::AllDone);
        });
    }

    fn overall(&self) -> f32 {
        let total: f32 = self.plan.iter().map(|i| i.weight()).sum();
        if total == 0. {
            return 0.;
        }
        let done: f32 = self
            .plan
            .iter()
            .map(|i| {
                let f = match self.state(*i) {
                    Some(RowState::Working { frac, .. }) => frac.unwrap_or(0.),
                    Some(RowState::Done(_)) | Some(RowState::Failed(_)) => 1.,
                    _ => 0.,
                };
                f * i.weight()
            })
            .sum();
        (done / total).clamp(0., 1.)
    }

    fn headline(&self) -> (String, String) {
        let version = env!("CARGO_PKG_VERSION");
        if self.args.uninstall {
            return match self.stage {
                Stage::Checking => ("Roy Music".into(), "Getting ready…".into()),
                Stage::Review => (
                    "Remove Roy Music?".into(),
                    "Your music library stays unless you choose otherwise.".into(),
                ),
                Stage::Installing => ("Removing…".into(), "Closing the app and cleaning up".into()),
                Stage::Done => {
                    if self.failed().is_empty() {
                        (
                            "Roy Music was removed".into(),
                            "Thanks for listening.".into(),
                        )
                    } else {
                        (
                            "Something is still in the way".into(),
                            "Close Roy Music and try again.".into(),
                        )
                    }
                }
            };
        }
        match self.stage {
            Stage::Checking => (
                "Roy Music".into(),
                "Checking what your PC already has…".into(),
            ),
            Stage::Review => {
                let missing = self.missing();
                let mb: u32 = missing.iter().map(|m| m.1).sum();
                let head = match &self.installed {
                    Some(v) if v == version => "Repair Roy Music".to_owned(),
                    Some(_) => format!("Update to {version}"),
                    None => "Ready to install".to_owned(),
                };
                let sub = if missing.is_empty() {
                    "Your PC already has everything Roy Music needs.".to_owned()
                } else {
                    let names: Vec<&str> = missing.iter().map(|m| m.0.title()).collect();
                    format!(
                        "We'll also set up {} · about {mb} MB to download",
                        names.join(" and ").to_lowercase()
                    )
                };
                (head, sub)
            }
            Stage::Installing => {
                let note = self
                    .rows
                    .iter()
                    .find_map(|r| match &r.state {
                        RowState::Working { note, .. } if !note.is_empty() && note != "Waiting" => {
                            Some(note.clone())
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| "Starting…".into());
                ("Installing Roy Music".into(), note)
            }
            Stage::Done => {
                if self.failed().is_empty() {
                    (
                        "You're all set".into(),
                        "Roy Music is installed and ready to play.".into(),
                    )
                } else {
                    (
                        "Installed, with one thing left".into(),
                        "You can retry now, or finish it later in Settings.".into(),
                    )
                }
            }
        }
    }
}

/// Everything one installer step does, off the UI thread. `note` reports progress.
fn run_item(
    item: Item,
    args: &Args,
    desktop_shortcut: bool,
    remove_data: bool,
    note: &mut dyn FnMut(Option<f32>, String),
) -> anyhow::Result<String> {
    match item {
        Item::App if args.uninstall => {
            let dir = installation::install_directory()?;
            note(Some(0.2), "Closing Roy Music".into());
            let data = remove_data
                .then(|| riff_core::Engine::default_root().ok())
                .flatten();
            let data = if installation::test_root().is_some() {
                None
            } else {
                data
            };
            note(Some(0.6), "Removing files and shortcuts".into());
            installation::uninstall(&dir, data.as_deref())?;
            Ok("Removed".into())
        }
        Item::App => {
            let dir = installation::install_directory()?;
            note(Some(0.08), "Unpacking files".into());
            let staging = tempfile::tempdir()?;
            let own = std::env::current_exe()?;
            let entries = payload::entries(&mut std::fs::File::open(&own)?)?;
            let app = if let Some(path) = &args.app {
                path.clone()
            } else {
                anyhow::ensure!(
                    entries.iter().any(|(n, _)| n == "roy-music.exe"),
                    "This Setup has no program files inside. Use the packaged Roy-Music Setup"
                );
                for (name, bytes) in &entries {
                    std::fs::write(staging.path().join(name), bytes)?;
                }
                staging.path().join("roy-music.exe")
            };
            note(Some(0.35), "Closing Roy Music if it is open".into());
            installation::close_instances(&dir)?;
            note(Some(0.6), "Copying files".into());
            let uninstaller = payload::program_bytes(&mut std::fs::File::open(&own)?)?;
            let options = installation::Options {
                desktop_shortcut,
                uninstaller: Some(uninstaller),
            };
            note(Some(0.85), "Creating shortcuts".into());
            installation::install_with(&app, &options)?;
            Ok("Installed".into())
        }
        Item::Media | Item::Online => {
            let id = item.id().expect("installable");
            let started = Instant::now();
            let version = prerequisites::install(id, &mut |phase, done, total| match phase {
                Phase::Resolving => note(None, "Finding the latest build".into()),
                Phase::Downloading => {
                    let frac = total
                        .filter(|t| *t > 0)
                        .map(|t| 0.05 + 0.85 * done as f32 / t as f32);
                    let speed = done as f64 / started.elapsed().as_secs_f64().max(0.2) / 1_048_576.;
                    let text = match total {
                        Some(t) => format!(
                            "Downloading {} · {} of {} MB · {speed:.1} MB/s",
                            item.title().to_lowercase(),
                            done / 1_048_576,
                            t / 1_048_576
                        ),
                        None => format!("Downloading {}", item.title().to_lowercase()),
                    };
                    note(frac.or(Some(0.05)), text);
                }
                Phase::Unpacking => note(Some(0.92), "Unpacking".into()),
                Phase::Verifying => note(Some(0.98), "Checking it works".into()),
            })?;
            Ok(if version.is_empty() {
                "Installed".into()
            } else {
                format!("Installed {version}")
            })
        }
        _ => anyhow::bail!("Nothing to install"),
    }
}

fn pill(
    ui: &mut egui::Ui,
    rect: Rect,
    label: &str,
    primary: bool,
    enabled: bool,
    alpha: f32,
) -> egui::Response {
    let id = ui.id().with(label);
    let response = ui.interact(
        rect,
        id,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let hover = ui
        .ctx()
        .animate_bool_with_time(id.with("h"), response.hovered() && enabled, 0.12);
    let press =
        ui.ctx()
            .animate_bool_with_time(id.with("p"), response.is_pointer_button_down_on(), 0.06);
    let grow = 1. + 0.035 * hover - 0.03 * press;
    let r = Rect::from_center_size(rect.center(), rect.size() * grow);
    let p = ui.painter();
    let (fill, text) = if primary {
        (
            GREEN.lerp_to_gamma(Color32::from_rgb(0x3B, 0xE4, 0x77), hover),
            Color32::BLACK,
        )
    } else {
        (Color32::from_white_alpha((14. + 20. * hover) as u8), WHITE)
    };
    let (fill, text) = if enabled {
        (fill, text)
    } else {
        (Color32::from_white_alpha(18), MUTED)
    };
    p.rect_filled(r, r.height() / 2., fade(fill, alpha));
    if response.has_focus() {
        p.rect_stroke(
            r.expand(3.),
            r.height() / 2. + 3.,
            stroke(2., WHITE),
            egui::StrokeKind::Outside,
        );
    }
    p.text(
        r.center(),
        Align2::CENTER_CENTER,
        label,
        bold(15.),
        fade(text, alpha),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    response
}

fn checkbox(ui: &mut egui::Ui, at: Pos2, label: &str, on: &mut bool, alpha: f32) {
    let rect = Rect::from_min_size(at, vec2(220., 28.));
    let id = ui.id().with(label);
    let response = ui.interact(rect, id, Sense::click());
    if response.clicked() {
        *on = !*on;
    }
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *on, label));
    let t = ui.ctx().animate_bool_with_time(id.with("on"), *on, 0.14);
    let p = ui.painter();
    let box_rect = Rect::from_min_size(at + vec2(0., 4.), vec2(20., 20.));
    p.rect_filled(
        box_rect,
        5.,
        fade(Color32::from_white_alpha(28).lerp_to_gamma(GREEN, t), alpha),
    );
    if t > 0.01 {
        check_mark(p, box_rect.center(), 14., t, fade(Color32::BLACK, alpha));
    }
    if response.has_focus() {
        p.rect_stroke(
            box_rect.expand(2.),
            6.,
            stroke(1.5, WHITE),
            egui::StrokeKind::Outside,
        );
    }
    p.text(
        at + vec2(30., 14.),
        Align2::LEFT_CENTER,
        label,
        regular(14.),
        fade(MUTED, alpha),
    );
}

impl Setup {
    fn draw_icon(&self, p: &egui::Painter, c: Pos2, row: &Row, now: f64, alpha: f32) {
        let since = ((now - row.changed) as f32 / 0.35).clamp(0., 1.);
        let pop = if self.animations {
            ease_out_back(since)
        } else {
            1.
        };
        let r = 13.;
        match &row.state {
            RowState::Waiting => {
                p.circle_filled(c, 3., fade(Color32::from_white_alpha(50), alpha));
            }
            RowState::Checking | RowState::Working { .. } => {
                p.circle_stroke(
                    c,
                    r - 2.,
                    stroke(2.5, fade(Color32::from_white_alpha(28), alpha)),
                );
                let spin = if self.animations {
                    now as f32 * 5.5
                } else {
                    0.
                };
                arc(p, c, r - 2., spin, 1.7, stroke(2.5, fade(GREEN, alpha)));
            }
            RowState::Ready(_) | RowState::Done(_) => {
                p.circle_filled(c, r * pop, fade(GREEN, alpha));
                check_mark(
                    p,
                    c,
                    15. * pop,
                    if self.animations { since } else { 1. },
                    fade(Color32::BLACK, alpha),
                );
            }
            RowState::Will(_) | RowState::Missing(_) => {
                p.circle_stroke(c, r - 1., stroke(2., fade(WHITE, alpha * 0.9)));
                let s = stroke(2., fade(WHITE, alpha));
                p.line_segment([c + vec2(0., -5.), c + vec2(0., 4.)], s);
                p.line_segment([c + vec2(-4., 0.), c + vec2(0., 4.5)], s);
                p.line_segment([c + vec2(4., 0.), c + vec2(0., 4.5)], s);
            }
            RowState::Warning(_) => {
                p.circle_filled(c, r * pop, fade(AMBER, alpha));
                p.text(
                    c + vec2(0., 0.5),
                    Align2::CENTER_CENTER,
                    "!",
                    bold(16.),
                    fade(Color32::BLACK, alpha),
                );
            }
            RowState::Failed(_) => {
                p.circle_filled(c, r * pop, fade(RED, alpha));
                let s = stroke(2.4, fade(Color32::BLACK, alpha));
                p.line_segment([c + vec2(-4.5, -4.5), c + vec2(4.5, 4.5)], s);
                p.line_segment([c + vec2(-4.5, 4.5), c + vec2(4.5, -4.5)], s);
            }
        }
    }

    fn status_text(&self, row: &Row) -> (String, Color32) {
        let (text, color) = self.status_text_raw(row);
        let short: String = if text.chars().count() > 34 {
            text.chars().take(33).chain(['…']).collect()
        } else {
            text
        };
        (short, color)
    }
    fn status_text_raw(&self, row: &Row) -> (String, Color32) {
        match &row.state {
            RowState::Waiting => (String::new(), MUTED),
            RowState::Checking => ("Checking…".into(), MUTED),
            RowState::Ready(v) => (
                if v.is_empty() {
                    "Found".into()
                } else {
                    format!("Found · {v}")
                },
                MUTED,
            ),
            RowState::Will(s) => (s.clone(), WHITE),
            RowState::Missing(mb) => (format!("Will download · {mb} MB"), WHITE),
            RowState::Warning(w) => (w.clone(), AMBER),
            RowState::Working { frac, .. } => (
                frac.map(|f| format!("{:.0}%", f * 100.))
                    .unwrap_or_default(),
                WHITE,
            ),
            RowState::Done(s) => (s.clone(), GREEN),
            RowState::Failed(_) => ("Couldn't finish".into(), RED),
        }
    }

    fn draw_rows(
        &self,
        ui: &mut egui::Ui,
        top: f32,
        left: f32,
        width: f32,
        now: f64,
        page_alpha: f32,
    ) {
        let ctx = ui.ctx().clone();
        let uninstall = self.args.uninstall;
        let height = 52.;
        let gap = 6.;
        for (index, row) in self.rows.iter().cloned().enumerate() {
            let Some(appeared) = row.appeared else {
                continue;
            };
            let t = if self.animations {
                ease_out_cubic(((now - appeared) as f32 / 0.38).clamp(0., 1.))
            } else {
                1.
            };
            let y = top + index as f32 * (height + gap);
            let rect = Rect::from_min_size(pos2(left + (1. - t) * 18., y), vec2(width, height));
            let alpha = t * page_alpha;
            let p = ui.painter();
            p.rect_filled(rect, 10., fade(ROW, alpha));
            self.draw_icon(p, rect.left_center() + vec2(26., 0.), &row, now, alpha);
            let tx = rect.left() + 54.;
            let (status, color) = self.status_text(&row);
            let blurb = match &row.state {
                RowState::Failed(e) => (e.chars().take(70).collect::<String>(), RED),
                RowState::Working { note, .. } if !note.is_empty() && note != "Waiting" => {
                    (note.chars().take(62).collect::<String>(), MUTED)
                }
                _ => (row.item.blurb().to_owned(), MUTED),
            };
            let blurb_text = if uninstall && row.item == Item::App {
                "Program files, shortcuts and the Apps entry".to_owned()
            } else {
                blurb.0
            };
            p.text(
                pos2(tx, rect.top() + 18.),
                Align2::LEFT_CENTER,
                row.item.title(),
                bold(15.),
                fade(WHITE, alpha),
            );
            p.text(
                pos2(tx, rect.top() + 36.),
                Align2::LEFT_CENTER,
                blurb_text,
                regular(12.5),
                fade(blurb.1, alpha),
            );
            p.text(
                pos2(rect.right() - 16., rect.center().y),
                Align2::RIGHT_CENTER,
                status,
                regular(13.),
                fade(color, alpha),
            );
            // Slim progress line along the bottom edge of the row that is working.
            if let RowState::Working { frac, .. } = &row.state {
                let target = frac.unwrap_or(0.04);
                let shown = ctx.animate_value_with_time(
                    ui.id().with(("row", index)),
                    target,
                    if self.animations { 0.3 } else { 0. },
                );
                let track = Rect::from_min_max(
                    pos2(rect.left() + 12., rect.bottom() - 5.),
                    pos2(rect.right() - 12., rect.bottom() - 2.),
                );
                p.rect_filled(track, 2., fade(Color32::from_white_alpha(22), alpha));
                let mut fill = track;
                fill.set_right(track.left() + track.width() * shown.clamp(0., 1.));
                p.rect_filled(fill, 2., fade(GREEN, alpha));
                if frac.is_none() && self.animations {
                    // Unknown length: a short bar sweeps along the track.
                    let sweep = ((now * 0.9) % 1.) as f32;
                    let w = track.width() * 0.25;
                    let x = track.left() - w + (track.width() + w) * sweep;
                    let bar = Rect::from_min_max(
                        pos2(x.max(track.left()), track.top()),
                        pos2((x + w).min(track.right()), track.bottom()),
                    );
                    if bar.width() > 0. {
                        p.rect_filled(bar, 2., fade(GREEN, alpha));
                    }
                }
            }
        }
    }
}

impl eframe::App for Setup {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0., 0., 0., 0.]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);
        self.pump(now);
        if ctx.input(|i| i.viewport().close_requested()) && self.stage == Stage::Installing {
            let quit = rfd::MessageDialog::new()
                .set_title("Roy Music Setup")
                .set_description("Setup is still working. Quit anyway?")
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
                == rfd::MessageDialogResult::Yes;
            if !quit {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.stage != Stage::Installing {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint_after(Duration::from_millis(16));
        if self.stage == Stage::Done
            && self.args.launch
            && !self.args.uninstall
            && self.failed().is_empty()
            && !self.launched
        {
            self.launch(ctx);
        }
        if self.stage_since == 0. {
            self.stage_since = now;
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let full = ui.max_rect();
                let p = ui.painter().clone();
                p.rect_filled(full, 18., BG);
                p.rect_stroke(
                    full.shrink(0.5),
                    18.,
                    stroke(1., Color32::from_white_alpha(22)),
                    egui::StrokeKind::Inside,
                );
                let intro = if self.animations {
                    ((now - 0.05) as f32).clamp(0., 1.)
                } else {
                    1.
                };
                let page = if self.animations {
                    ease_out_cubic(((now - self.stage_since) as f32 / 0.3).clamp(0., 1.))
                } else {
                    1.
                };
                let page = if self.stage == Stage::Checking {
                    intro
                } else {
                    page.max(0.35)
                };
                let cx = full.center().x;

                // Title bar: drag anywhere along the top; minimise and close on the right.
                let bar = Rect::from_min_size(full.min, vec2(full.width(), 44.));
                if ui
                    .interact(
                        bar.with_max_x(full.right() - 96.),
                        ui.id().with("drag"),
                        Sense::drag(),
                    )
                    .drag_started()
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                for (i, kind) in ["min", "close"].iter().enumerate() {
                    let r = Rect::from_center_size(
                        pos2(full.right() - 62. + i as f32 * 40., full.top() + 24.),
                        vec2(32., 28.),
                    );
                    let id = ui.id().with(kind);
                    let resp = ui.interact(r, id, Sense::click());
                    let h = ctx.animate_bool_with_time(id.with("h"), resp.hovered(), 0.1);
                    if h > 0. {
                        p.rect_filled(
                            r,
                            8.,
                            if *kind == "close" {
                                fade(RED, 0.9 * h)
                            } else {
                                Color32::from_white_alpha((24. * h) as u8)
                            },
                        );
                    }
                    let s = stroke(
                        1.6,
                        if h > 0.5 && *kind == "close" {
                            Color32::BLACK
                        } else {
                            MUTED
                        },
                    );
                    let c = r.center();
                    if *kind == "min" {
                        p.line_segment([c + vec2(-5., 0.), c + vec2(5., 0.)], s);
                    } else {
                        p.line_segment([c + vec2(-5., -5.), c + vec2(5., 5.)], s);
                        p.line_segment([c + vec2(-5., 5.), c + vec2(5., -5.)], s);
                    }
                    resp.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            if *kind == "min" { "Minimize" } else { "Close" },
                        )
                    });
                    if resp.clicked() {
                        ctx.send_viewport_cmd(if *kind == "min" {
                            egui::ViewportCommand::Minimized(true)
                        } else {
                            egui::ViewportCommand::Close
                        });
                    }
                }

                // Logo with a soft glow that breathes, and a ring that pulses once when everything is done.
                let logo_c = pos2(cx, full.top() + 86.);
                let scale = if self.animations {
                    0.55 + 0.45 * ease_out_back(((now - 0.05) as f32 / 0.7).clamp(0., 1.))
                } else {
                    1.
                };
                let breathe = if self.animations {
                    1. + 0.06 * (now as f32 * 1.6).sin()
                } else {
                    1.
                };
                let glow_alpha = if self.stage == Stage::Done && self.failed().is_empty() {
                    1.5
                } else {
                    1.
                };
                for i in 0..14 {
                    let t = i as f32 / 13.;
                    p.circle_filled(
                        logo_c,
                        (46. + 120. * (1. - t)) * breathe,
                        fade(GREEN, (0.011 + 0.002 * t) * glow_alpha * intro),
                    );
                }
                if self.stage == Stage::Done && self.animations && self.failed().is_empty() {
                    let t = ((now - self.stage_since) as f32 / 1.1).clamp(0., 1.);
                    p.circle_stroke(
                        logo_c,
                        44. + 70. * ease_out_cubic(t),
                        stroke(2., fade(GREEN, (1. - t) * 0.8)),
                    );
                }
                p.circle_filled(logo_c, 44. * scale, fade(GREEN, intro));
                let logo_rect = Rect::from_center_size(logo_c, vec2(54., 54.) * scale);
                egui::Image::new(egui::include_image!("../assets/icon.png"))
                    .tint(fade(Color32::BLACK, intro))
                    .paint_at(ui, logo_rect);

                let (head, sub) = self.headline();
                let rise = if self.animations {
                    (1. - intro) * 10.
                } else {
                    0.
                };
                p.text(
                    pos2(cx, full.top() + 150. + rise),
                    Align2::CENTER_CENTER,
                    head,
                    bold(26.),
                    fade(WHITE, intro),
                );
                p.text(
                    pos2(cx, full.top() + 178. + rise),
                    Align2::CENTER_CENTER,
                    sub,
                    regular(14.),
                    fade(MUTED, intro),
                );

                // Overall progress while installing.
                let list_top = full.top() + 208.;
                let list_left = full.left() + 60.;
                let list_w = full.width() - 120.;
                if self.stage == Stage::Installing
                    || (self.stage == Stage::Done && self.started.is_some())
                {
                    let target = if self.stage == Stage::Done {
                        1.
                    } else {
                        self.overall()
                    };
                    let shown = ctx.animate_value_with_time(
                        ui.id().with("overall"),
                        target,
                        if self.animations { 0.4 } else { 0. },
                    );
                    let track =
                        Rect::from_min_size(pos2(list_left, full.top() + 194.), vec2(list_w, 4.));
                    p.rect_filled(track, 2., fade(Color32::from_white_alpha(24), page));
                    let mut fill = track;
                    fill.set_right(track.left() + track.width() * shown);
                    p.rect_filled(fill, 2., fade(GREEN, page));
                    if self.animations && self.stage == Stage::Installing && fill.width() > 6. {
                        let x = fill.left() + fill.width() * ((now * 0.8) % 1.) as f32;
                        p.circle_filled(pos2(x, track.center().y), 3.5, fade(WHITE, 0.55 * page));
                    }
                }
                self.draw_rows(ui, list_top + 12., list_left, list_w, now, page);

                // Footer: options on the left, actions on the right, flush with the list.
                let right = list_left + list_w;
                let foot = full.bottom() - 54.;
                let mut want: Option<&str> = None;
                match self.stage {
                    Stage::Checking => {
                        pill(
                            ui,
                            Rect::from_center_size(pos2(right - 75., foot), vec2(150., 44.)),
                            "Install",
                            true,
                            false,
                            page,
                        );
                    }
                    Stage::Review => {
                        if !self.args.uninstall {
                            checkbox(
                                ui,
                                pos2(list_left, foot - 14.),
                                "Desktop shortcut",
                                &mut self.desktop_shortcut,
                                page,
                            );
                        } else {
                            checkbox(
                                ui,
                                pos2(list_left, foot - 14.),
                                "Also delete my library and settings",
                                &mut self.remove_data,
                                page,
                            );
                        }
                        let go = if self.args.uninstall {
                            "Uninstall"
                        } else if self.installed.is_some() {
                            "Update"
                        } else {
                            "Install"
                        };
                        if pill(
                            ui,
                            Rect::from_center_size(pos2(right - 75., foot), vec2(150., 44.)),
                            go,
                            true,
                            true,
                            page,
                        )
                        .clicked()
                        {
                            want = Some("go");
                        }
                        if pill(
                            ui,
                            Rect::from_center_size(pos2(right - 210., foot), vec2(100., 44.)),
                            "Cancel",
                            false,
                            true,
                            page,
                        )
                        .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                    Stage::Installing => {
                        pill(
                            ui,
                            Rect::from_center_size(pos2(right - 75., foot), vec2(150., 44.)),
                            "Installing…",
                            true,
                            false,
                            page,
                        );
                    }
                    Stage::Done => {
                        let failed = !self.failed().is_empty();
                        if failed {
                            if pill(
                                ui,
                                Rect::from_center_size(pos2(right - 75., foot), vec2(150., 44.)),
                                "Try again",
                                true,
                                true,
                                page,
                            )
                            .clicked()
                            {
                                want = Some("retry");
                            }
                            if !self.args.uninstall
                                && pill(
                                    ui,
                                    Rect::from_center_size(
                                        pos2(right - 255., foot),
                                        vec2(190., 44.),
                                    ),
                                    "Launch Roy Music",
                                    false,
                                    true,
                                    page,
                                )
                                .clicked()
                            {
                                want = Some("launch");
                            }
                        } else if self.args.uninstall {
                            if pill(
                                ui,
                                Rect::from_center_size(pos2(right - 75., foot), vec2(150., 44.)),
                                "Close",
                                true,
                                true,
                                page,
                            )
                            .clicked()
                            {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        } else {
                            if pill(
                                ui,
                                Rect::from_center_size(pos2(right - 105., foot), vec2(210., 44.)),
                                "Launch Roy Music",
                                true,
                                true,
                                page,
                            )
                            .clicked()
                            {
                                want = Some("launch");
                            }
                            if pill(
                                ui,
                                Rect::from_center_size(pos2(right - 265., foot), vec2(90., 44.)),
                                "Close",
                                false,
                                true,
                                page,
                            )
                            .clicked()
                            {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                    }
                }
                // Deleting the library cannot be undone, so it takes a second, explicit yes.
            if want == Some("go")
                && self.args.uninstall
                && self.remove_data
                && rfd::MessageDialog::new()
                    .set_title("Delete your library?")
                    .set_description(
                        "Your library, playlists, settings and downloaded tools will be permanently deleted.",
                    )
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show()
                    != rfd::MessageDialogResult::Yes
            {
                want = None;
                self.remove_data = false;
            }
            if self.args.yes && self.stage == Stage::Review {
                    want = Some("go");
                }
                match want {
                    Some("go") => {
                        let mut items = vec![Item::App];
                        items.extend(self.missing().into_iter().map(|m| m.0));
                        // Offline: leave the downloads for later instead of failing them.
                        let skipped: Vec<Item> = self
                            .rows
                            .iter()
                            .filter(|r| {
                                matches!(r.state, RowState::Missing(_)) && !items.contains(&r.item)
                            })
                            .map(|r| r.item)
                            .collect();
                        for item in skipped {
                            self.set(
                                item,
                                RowState::Warning("Skipped · you are offline".into()),
                                now,
                            );
                        }
                        self.start_install(items, now);
                    }
                    Some("retry") => {
                        let items = self.failed();
                        self.start_install(items, now);
                    }
                    Some("launch") => self.launch(ctx),
                    _ => {}
                }
            });
    }
}

impl Setup {
    fn launch(&mut self, ctx: &egui::Context) {
        if self.launched {
            return;
        }
        self.launched = true;
        if !self.args.no_launch && installation::test_root().is_none() {
            let exe = self.dir.join(installation::APP_EXE);
            let _ = riff_core::tools::command(exe.to_str().unwrap_or("")).spawn();
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

/// Install without a window (`--silent`), logging one line per step.
fn run_silent(args: &Args) -> i32 {
    let mut log: Box<dyn Write> = match &args.log {
        Some(path) => std::fs::File::create(path)
            .map(|f| Box::new(f) as Box<dyn Write>)
            .unwrap_or_else(|_| Box::new(std::io::sink())),
        None => Box::new(std::io::stdout()),
    };
    let dir = installation::install_directory().unwrap_or_default();
    let mut failures = 0;
    let mut items = vec![Item::App];
    if !args.uninstall {
        let online = prerequisites::network();
        let _ = writeln!(log, "network: {:?}", online.state);
        for check in prerequisites::check_all(&dir) {
            let _ = writeln!(log, "check {:?}: {:?}", check.id, check.state);
            if let (State::Missing { .. }, true) =
                (&check.state, matches!(online.state, State::Ready(_)))
            {
                items.push(Item::from_id(check.id));
            }
        }
    }
    let log = std::cell::RefCell::new(log);
    for item in items {
        let mut last = String::new();
        let result = run_item(
            item,
            args,
            args.desktop_shortcut.unwrap_or(false),
            false,
            &mut |_, note| {
                // Counters change every block; log a line only when the step itself changes.
                let step = note.split(" · ").next().unwrap_or("").to_owned();
                if step != last {
                    let _ = writeln!(log.borrow_mut(), "{}: {step}", item.title());
                    last = step;
                }
            },
        );
        match result {
            Ok(done) => {
                let _ = writeln!(log.borrow_mut(), "{}: {done}", item.title());
            }
            Err(error) => {
                failures += 1;
                let _ = writeln!(log.borrow_mut(), "{}: FAILED {error:#}", item.title());
            }
        }
    }
    if failures == 0 { 0 } else { 1 }
}

fn parse() -> Args {
    let mut args = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--uninstall" => args.uninstall = true,
            "--silent" | "/S" | "/s" => args.silent = true,
            "--no-launch" => args.no_launch = true,
            "--yes" | "-y" => args.yes = true,
            // The in-app updater runs the downloaded Setup as `--install --wait-pid N --launch`.
            "--install" => args.yes = true,
            "--launch" => args.launch = true,
            "--wait-pid" => args.wait_pid = it.next().and_then(|p| p.parse().ok()),
            "--desktop-shortcut" => args.desktop_shortcut = Some(true),
            "--no-desktop-shortcut" => args.desktop_shortcut = Some(false),
            "--log" => args.log = it.next().map(PathBuf::from),
            "--app" => args.app = it.next().map(PathBuf::from),
            "--test-root" => {
                if let Some(root) = it.next() {
                    // Everything below this folder: app, tools. Nothing outside is touched.
                    // SAFETY: set before any thread starts.
                    unsafe {
                        std::env::set_var("ROY_SETUP_TEST_ROOT", &root);
                        std::env::set_var("RIFF_TOOLS_DIR", PathBuf::from(&root).join("tools"));
                    }
                }
            }
            _ => {}
        }
    }
    args
}

fn main() -> eframe::Result {
    let raw: Vec<String> = std::env::args().collect();
    if raw.get(1).map(String::as_str) == Some("--pack") {
        let (Some(out), files) = (raw.get(2), &raw[3.min(raw.len())..]) else {
            eprintln!("usage: roy-setup --pack <output.exe> <file>...");
            std::process::exit(2);
        };
        let paths: Vec<&std::path::Path> = files.iter().map(std::path::Path::new).collect();
        match payload::pack(
            &std::env::current_exe().expect("current exe"),
            std::path::Path::new(out),
            &paths,
        ) {
            Ok(()) => println!("packed {} files into {out}", paths.len()),
            Err(e) => {
                eprintln!("pack failed: {e:#}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    let args = parse();
    if args.silent {
        std::process::exit(run_silent(&args));
    }
    let icon = image::load_from_memory(include_bytes!("../assets/icon-app.png"))
        .expect("embedded icon")
        .into_rgba8();
    let (width, height) = icon.dimensions();
    let options = eframe::NativeOptions {
        centered: true,
        viewport: egui::ViewportBuilder::default()
            .with_title(if args.uninstall {
                "Uninstall Roy Music"
            } else {
                "Roy Music Setup"
            })
            .with_icon(egui::IconData {
                rgba: icon.into_raw(),
                width,
                height,
            })
            .with_inner_size(WINDOW)
            .with_resizable(false)
            .with_maximize_button(false)
            .with_decorations(false)
            .with_transparent(true),
        ..Default::default()
    };
    eframe::run_native(
        "Roy Music Setup",
        options,
        Box::new(move |cc| Ok(Box::new(Setup::new(cc, args)))),
    )
}
