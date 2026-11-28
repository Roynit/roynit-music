//! The Import tab: pick a service, paste a link, choose where the songs go.
use super::{App, MUTED, Weight, roy_account_ui};
use eframe::egui::{self, Color32, RichText};
use serde_json::json;

pub(super) struct Source {
    pub(super) name: &'static str,
    /// Hosts a link from this service may have; anything else is refused before any request is made.
    hosts: &'static [&'static str],
    /// Offers a "Log in" tab (likes and playlists from the linked account).
    account: bool,
}

const SOURCES: [Source; 8] = [
    Source {
        name: "Spotify",
        hosts: &["spotify.com"],
        account: false,
    },
    Source {
        name: "YouTube Music",
        hosts: &["youtube.com", "youtu.be"],
        account: true,
    },
    Source {
        name: "SoundCloud",
        hosts: &["soundcloud.com"],
        account: false,
    },
    Source {
        name: "Apple Music",
        hosts: &["apple.com"],
        account: false,
    },
    Source {
        name: "Deezer",
        hosts: &["deezer.com", "deezer.page.link"],
        account: false,
    },
    Source {
        name: "Yandex Music",
        hosts: &["yandex."],
        account: false,
    },
    Source {
        name: "TikTok",
        hosts: &["tiktok.com"],
        account: false,
    },
    Source {
        name: "VK Music",
        hosts: &["vk.com", "vk.ru"],
        account: false,
    },
];

/// Which engine call turns a link into a playlist (catalog services match songs later, others read them directly).
fn operation(url: &str) -> &'static str {
    let host = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_lowercase))
        .unwrap_or_default();
    let catalog = url.starts_with("spotify:playlist:")
        || [
            "spotify.com",
            "deezer.com",
            "apple.com",
            "yandex.",
            "vk.com",
            "vk.ru",
            "tiktok.com",
        ]
        .iter()
        .any(|h| host.contains(h));
    if catalog {
        "catalog.import"
    } else {
        "collection.import"
    }
}

fn belongs_to(source: &Source, url: &str) -> bool {
    if source.name == "Spotify" && url.starts_with("spotify:playlist:") {
        return true;
    }
    url::Url::parse(url).ok().is_some_and(|u| {
        u.scheme() == "https"
            && u.host_str()
                .is_some_and(|host| source.hosts.iter().any(|h| host.to_lowercase().contains(h)))
    })
}

/// A pasted link, tidied: spaces trimmed and "https://" added when it was left off.
fn clean(url: &str) -> String {
    let url = url.trim();
    if url.starts_with("spotify:") || url.contains("://") { url.to_owned() } else { format!("https://{url}") }
}

/// Which service a link comes from.
fn detect(url: &str) -> Option<usize> {
    SOURCES.iter().position(|source| belongs_to(source, url))
}

/// Whether the person just pasted into this field.
fn pasted(ui: &egui::Ui, field: &egui::Response) -> bool {
    field.has_focus() && ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Paste(_))))
}

impl App {
    /// Starts importing a link from whichever service it belongs to.
    pub(super) fn import_link(&mut self, url: &str) -> Result<(), String> {
        self.import_to_favorites = false;
        self.start_import(url)
    }

    fn start_import(&mut self, url: &str) -> Result<(), String> {
        let url = clean(url);
        let source = detect(&url).ok_or_else(|| "That link is not from Spotify, YouTube, SoundCloud, Apple Music, Deezer, Yandex Music, TikTok or VK.".to_owned())?;
        self.import_pending_fav = self.import_to_favorites;
        self.job("import", operation(&url), json!({"url":url}));
        self.status = format!("Importing from {}…", SOURCES[source].name);
        self.import_error = None;
        self.import_url.clear();
        Ok(())
    }

    /// Settings → Import music: paste any link at the top (the service is found from the link), or pick a service.
    pub(super) fn settings_import(&mut self, ui: &mut egui::Ui, q: &str) {
        if q.is_empty() {
            ui.label(RichText::new("Paste a playlist link from any service: it is recognised and imported at once.").color(MUTED));
            ui.add_space(6.);
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.import_url)
                    .id(egui::Id::new("import-any-link"))
                    .hint_text("Paste a link: YouTube, Spotify, SoundCloud, Apple Music, Deezer…")
                    .desired_width(ui.available_width())
                    .margin(egui::vec2(12., 10.)),
            );
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (enter || pasted(ui, &field)) && !self.import_url.trim().is_empty() {
                let url = self.import_url.clone();
                if let Err(error) = self.start_import(&url) {
                    self.import_error = Some(error);
                }
            }
            if self.import_source.is_none()
                && let Some(error) = &self.import_error
            {
                ui.label(RichText::new(error).color(Color32::from_rgb(244, 155, 158)));
            }
            ui.add_space(10.);
        }
        for (index, source) in SOURCES.iter().enumerate() {
            Self::row(
                ui,
                q,
                source.name,
                "",
                "import playlist link favorites",
                |ui| {
                    let open = ui.add(
                        egui::Button::new(RichText::new("Import").bold().color(Color32::BLACK))
                            .fill(Color32::from_gray(0xE6))
                            .corner_radius(18)
                            .min_size(egui::vec2(96., 34.)),
                    );
                    if open.clicked() {
                        self.import_source = Some(index);
                        self.import_tab = 0;
                        self.import_url.clear();
                        self.import_return = true;
                        self.settings_open = false;
                    }
                },
            );
        }
    }

    pub(super) fn import_dialog(&mut self, ctx: &egui::Context) {
        let Some(index) = self.import_source else {
            return;
        };
        let source = &SOURCES[index];
        let top = self.dialog_top();
        let mut close = false;
        let mut start = false;
        let mut started = false;
        close |= Self::dialog(ctx, top, "import", true, |ui| {
            ui.set_width(440.);
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new(RichText::new("←").size(18.)).frame(false))
                    .on_hover_text("Back")
                    .clicked()
                {
                    close = true;
                }
                ui.label(
                    RichText::new(format!("Import from {}", source.name))
                        .size(24.)
                        .bold(),
                );
            });
            ui.add_space(12.);
            if source.account {
                ui.horizontal(|ui| {
                    for (tab, label) in ["Playlist link", "Log in"].into_iter().enumerate() {
                        if App::filter_chip(ui, label, self.import_tab == tab as u8).clicked() {
                            self.import_tab = tab as u8;
                        }
                    }
                });
                ui.add_space(10.);
            }
            if source.account && self.import_tab == 1 {
                self.import_account_tab(ui);
                return;
            }
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.import_url)
                    .hint_text(format!("Paste a {} link", source.name))
                    .desired_width(ui.available_width())
                    .margin(egui::vec2(12., 10.)),
            );
            // A link from another service is fine: the dialog follows it.
            if let Some(found) = detect(&clean(&self.import_url)) {
                self.import_source = Some(found);
            }
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (enter || (pasted(ui, &field) && detect(&clean(&self.import_url)).is_some())) && !self.import_url.trim().is_empty() {
                start = true;
            }
            ui.add_space(12.);
            ui.label(RichText::new("Import into").bold());
            egui::ComboBox::from_id_salt("import-into")
                .width(ui.available_width())
                .selected_text(if self.import_to_favorites {
                    "Add directly to Favorites"
                } else {
                    "Create new playlist"
                })
                .show_ui(ui, |ui| {
                    super::menu_style(ui);
                    ui.selectable_value(
                        &mut self.import_to_favorites,
                        false,
                        "Create new playlist",
                    );
                    ui.selectable_value(
                        &mut self.import_to_favorites,
                        true,
                        "Add directly to Favorites",
                    );
                });
            if let Some(error) = &self.import_error {
                ui.add_space(6.);
                ui.label(RichText::new(error).color(Color32::from_rgb(244, 155, 158)));
            }
            ui.add_space(14.);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ready = !self.import_url.trim().is_empty();
                if ui
                    .add_enabled(
                        ready,
                        egui::Button::new(RichText::new("Import").bold().color(Color32::BLACK))
                            .fill(Color32::from_gray(0xE6))
                            .corner_radius(18)
                            .min_size(egui::vec2(104., 36.)),
                    )
                    .clicked()
                {
                    start = true;
                }
                if ui.add(egui::Button::new("Cancel").frame(false)).clicked() {
                    close = true;
                }
            });
        });
        if start {
            let url = self.import_url.clone();
            match self.start_import(&url) {
                Ok(()) => {
                    self.import_return = false;
                    started = true;
                    close = true;
                }
                Err(error) => self.import_error = Some(error),
            }
        }
        if close {
            self.import_source = None;
            self.import_error = None;
            // Back or Cancel returns to Settings → Import music, where the dialog was opened.
            if !started && std::mem::take(&mut self.import_return) {
                self.settings_open = true;
                self.settings_group = 4;
            }
        }
    }

    /// YouTube Music through the linked Google account: no link to paste.
    fn import_account_tab(&mut self, ui: &mut egui::Ui) {
        use roy_account_ui::Provider;
        match self.roy_account.linked(Provider::Google) {
            None => {
                ui.add(
                    egui::Label::new(
                        RichText::new("Link your Google account once to import your liked videos and playlists.").color(MUTED),
                    )
                    .wrap(),
                );
                ui.add_space(10.);
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Link Google").bold().color(Color32::BLACK),
                        )
                        .fill(Color32::WHITE)
                        .corner_radius(18)
                        .min_size(egui::vec2(120., 36.)),
                    )
                    .clicked()
                {
                    self.roy_account.connect(Provider::Google);
                }
            }
            Some(label) => {
                ui.label(RichText::new(format!("Linked as {label}")).color(MUTED));
                ui.add_space(8.);
                ui.horizontal(|ui| {
                    if ui.button("Import liked videos").clicked() {
                        self.import_pending_fav = false;
                        self.send("import", "google.import", json!({"kind":"likes"}));
                        self.import_source = None;
                    }
                    if ui.button("Load my playlists").clicked() {
                        self.job(
                            "google-playlists",
                            "google.library",
                            json!({"kind":"playlists"}),
                        );
                    }
                });
                egui::ScrollArea::vertical().max_height(220.).show(ui, |ui| {
                    for playlist in self.google_playlists.clone() {
                        let name = playlist["snippet"]["title"].as_str().unwrap_or("YouTube playlist").to_owned();
                        ui.horizontal(|ui| {
                            ui.add(egui::Label::new(&name).truncate());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("Import").clicked() {
                                    self.import_pending_fav = false;
                                    self.send(
                                        "import",
                                        "google.import",
                                        json!({"kind":"tracks","playlist_id":playlist["id"],"name":name}),
                                    );
                                    self.import_source = None;
                                }
                            });
                        });
                    }
                });
            }
        }
    }

    pub(super) fn open_import(&mut self) {
        self.import_source = None;
        self.settings_query.clear();
        self.settings_group = 4;
        self.open_settings();
    }
}
