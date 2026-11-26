//! The Friends space: a friend list with what everyone is listening to, direct messages and profiles.
//! It takes over the sidebar and the right panel while it is open, the way Discord's home does.
use super::friends::{Friend, Listening, Person, Playing};
use super::*;
use chrono::{Local, TimeZone};

const GREEN: Color32 = Color32::from_rgb(0x3B, 0xA5, 0x5C);
const OFFLINE: Color32 = Color32::from_gray(0x6B);
const RED: Color32 = Color32::from_rgb(0xED, 0x42, 0x45);

#[derive(Clone, Copy, PartialEq, Default)]
pub(super) enum FriendsTab {
    #[default]
    Online,
    All,
    Pending,
    Add,
}

fn initials(name: &str) -> String {
    let letters: String = name
        .split_whitespace()
        .take(2)
        .filter_map(|word| word.chars().next())
        .flat_map(char::to_uppercase)
        .collect();
    if letters.is_empty() { "?".into() } else { letters }
}

/// A round picture with initials underneath (shown until the picture loads) and an optional status dot.
fn paint_person(ui: &egui::Ui, rect: egui::Rect, name: &str, uri: Option<String>, online: Option<bool>, ring: Color32) {
    let painter = ui.painter();
    painter.circle_filled(rect.center(), rect.width() / 2., roy_account_ui::tint_for(name));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initials(name),
        egui::FontId::proportional(rect.width() * 0.38),
        Color32::from_white_alpha(230),
    );
    if let Some(uri) = uri {
        App::paint_cover(ui, rect, &uri, (rect.width() / 2.) as u8);
    }
    if let Some(online) = online {
        let at = rect.right_bottom() - egui::vec2(rect.width() * 0.14, rect.width() * 0.14);
        let radius = rect.width() * 0.17;
        painter.circle_filled(at, radius + 2.5, ring);
        painter.circle_filled(at, radius, if online { GREEN } else { OFFLINE });
    }
}

fn clock(seconds: i64) -> String {
    Local
        .timestamp_opt(seconds, 0)
        .single()
        .map_or_else(String::new, |t| t.format("%H:%M").to_string())
}

fn day_label(seconds: i64) -> String {
    let Some(day) = Local.timestamp_opt(seconds, 0).single().map(|t| t.date_naive()) else {
        return String::new();
    };
    let today = Local::now().date_naive();
    if day == today {
        "Today".into()
    } else if today.pred_opt() == Some(day) {
        "Yesterday".into()
    } else {
        day.format("%b %e, %Y").to_string()
    }
}

fn last_seen(seconds: Option<i64>) -> String {
    let Some(seconds) = seconds else {
        return "Offline".into();
    };
    let ago = (chrono::Utc::now().timestamp() - seconds).max(0);
    match ago {
        0..=89 => "Just went offline".into(),
        90..=3599 => format!("Last seen {} min ago", ago / 60),
        3600..=86_399 => format!("Last seen {} h ago", ago / 3600),
        _ => format!("Last seen {} d ago", ago / 86_400),
    }
}

impl App {
    /// Called once a frame: keeps the friends service in step with the account and with what is playing.
    pub(super) fn friends_tick(&mut self) {
        let (endpoint, token) = self.roy_account.session();
        self.social.follow(endpoint, token);
        let playing = self.player["track_id"]
            .as_str()
            .and_then(|id| self.library.tracks.get(id))
            .map(|track| {
                let duration = self.player["duration"].as_f64().filter(|d| *d > 0.).unwrap_or(track.duration);
                (
                    Playing {
                        title: track.title.clone(),
                        artist: track.artist.clone(),
                        cover: track.cover.clone().filter(|c| c.starts_with("https://")),
                        duration,
                        playing: self.player["paused"] == false,
                    },
                    self.player["position"].as_f64().unwrap_or(0.),
                )
            });
        let open = self.open_chat.clone();
        let watching = self.page == Page::Friends;
        self.social.tick(watching, open.as_deref(), playing);
        if let Some((text, error)) = self.social.notice.take() {
            self.friends_notice = Some((text, error, Instant::now()));
        }
        if self.friends_notice.as_ref().is_some_and(|n| n.2.elapsed() > Duration::from_secs(6)) {
            self.friends_notice = None;
        }
        if let Some(id) = &self.open_chat
            && self.social.loaded
            && self.social.friend(id).is_none()
        {
            self.open_chat = None;
        }
        // A friend's songs arrived for Blend: mix them with yours into a playlist.
        if let Some((name, theirs)) = self.social.taste.take() {
            match riff_core::mixes::blend_with_friend(&self.library, &theirs) {
                Some((tracks, shared, percent)) => {
                    self.send("mix-saved", "mix.apply", json!({"name":format!("Blend: You & {name}"),"tracks":tracks,"play":false}));
                    self.friends_notice = Some((format!("You and {name}: {percent}% match. {shared} songs you both have, {} in all, saved to your playlists.", tracks.len()), false, Instant::now()));
                }
                None => {
                    self.friends_notice = Some(("Like or play a few songs first, so there is something to blend.".into(), true, Instant::now()));
                }
            }
        }
    }

    fn badge_count(&self) -> usize {
        self.social.requests() + self.social.unread_total() as usize
    }

    /// The title-bar button, to the left of Home, that opens the Friends space.
    pub(super) fn friends_button(&mut self, ui: &mut egui::Ui) {
        let active = self.page == Page::Friends;
        let (rect, response) = ui.allocate_exact_size(egui::vec2(42., 42.), egui::Sense::click());
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, "Friends"));
        let accent = ui.visuals().selection.bg_fill;
        let fill = if active { accent.gamma_multiply(0.23) } else { ui.style().interact(&response).bg_fill };
        let center = rect.center();
        let painter = ui.painter();
        painter.circle_filled(center, 20., fill);
        if response.has_focus() {
            painter.circle_stroke(center, 20., egui::Stroke::new(1.5_f32, accent));
        }
        let ink = if active { accent } else { ui.visuals().text_color() };
        let stroke = egui::Stroke::new(1.8_f32, ink);
        // Two people: a front one with a round head and shoulders, a smaller one behind.
        painter.circle_stroke(center + egui::vec2(-3., -5.), 4.2, stroke);
        painter.add(egui::Shape::line(
            [(-10., 9.), (-9., 4.), (-3., 2.), (3., 4.), (4., 9.)]
                .map(|(x, y)| center + egui::vec2(x, y))
                .to_vec(),
            stroke,
        ));
        painter.add(egui::Shape::line(
            [(4., -8.), (7., -7.), (8.5, -3.5), (7., -1.), (10., 3.), (10.5, 9.)]
                .map(|(x, y)| center + egui::vec2(x, y))
                .to_vec(),
            egui::Stroke::new(1.5_f32, ink.gamma_multiply(0.8)),
        ));
        let count = self.badge_count();
        if count > 0 {
            let at = center + egui::vec2(13., -13.);
            painter.circle_filled(at, 8.5, Self::chrome(ui.ctx()));
            painter.circle_filled(at, 7., RED);
            painter.text(
                at,
                egui::Align2::CENTER_CENTER,
                if count > 9 { "9+".to_owned() } else { count.to_string() },
                egui::FontId::proportional(9.5),
                Color32::WHITE,
            );
        }
        if response.on_hover_text("Friends").clicked() {
            self.navigate(Page::Friends);
        }
    }

    fn friend_line(&self, friend: &Friend) -> (String, Color32) {
        match (&friend.listening, friend.online) {
            (Some(l), true) => (format!("♫ {} · {}", l.title, l.artist), GREEN),
            (None, true) => ("Online".into(), MUTED),
            _ => (last_seen(friend.last_seen), MUTED.gamma_multiply(0.7)),
        }
    }

    /// One row of people: picture, name and a second line. Returns the row and where it was clicked.
    #[allow(clippy::too_many_arguments)]
    fn person_row(
        &self,
        ui: &mut egui::Ui,
        key: &str,
        person: &Person,
        online: Option<bool>,
        line: (String, Color32),
        active: bool,
        height: f32,
        badge: u32,
    ) -> (egui::Rect, egui::Response) {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click());
        let hover = ui.ctx().animate_bool_with_time(response.id.with(("row", key)), response.hovered(), self.motion());
        let fill = if active {
            Color32::from_white_alpha(22)
        } else {
            Color32::from_white_alpha((hover * 16.) as u8)
        };
        ui.painter().rect_filled(rect, 8., fill);
        shop_ui::paint_plate(ui, rect, &person.equipped);
        let size = height - 16.;
        let art = egui::Rect::from_min_size(rect.left_top() + egui::vec2(8., 8.), egui::vec2(size, size));
        paint_person(ui, art, &person.display_name, self.social.avatar_uri(person), online, Color32::from_gray(0x1D));
        shop_ui::paint_decoration(ui, art, &person.equipped);
        let text_left = art.right() + 10.;
        let width = (rect.right() - text_left - if badge > 0 { 34. } else { 8. }).max(20.);
        let mut text = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("person-text", key))
                .max_rect(egui::Rect::from_min_size(egui::pos2(text_left, rect.top()), egui::vec2(width, height)))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        text.set_clip_rect(rect.intersect(text.clip_rect()));
        text.spacing_mut().item_spacing.y = 0.;
        text.add_space((height - 34.) / 2.);
        text.add(egui::Label::new(RichText::new(&person.display_name).bold()).truncate().selectable(false));
        text.add(egui::Label::new(RichText::new(line.0).small().color(line.1)).truncate().selectable(false));
        if badge > 0 {
            let at = egui::pos2(rect.right() - 18., rect.center().y);
            ui.painter().circle_filled(at, 9., RED);
            ui.painter().text(
                at,
                egui::Align2::CENTER_CENTER,
                if badge > 99 { "99+".to_owned() } else { badge.to_string() },
                egui::FontId::proportional(10.5),
                Color32::WHITE,
            );
        }
        (rect, response)
    }

    fn open_friend(&mut self, id: &str) {
        self.open_chat = Some(id.to_owned());
        self.social.open_chat(id);
        self.navigate(Page::Friends);
    }

    /// Left panel while Friends is open: the friend list as conversations, and your own status below.
    pub(super) fn friends_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("navigation")
            .exact_width(Self::LIBRARY_WIDTH)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(Self::chrome(ctx))
                    .inner_margin(egui::Margin { left: 8, right: 4, top: 0, bottom: 4 }),
            )
            .show(ctx, |ui| {
                ui.visuals_mut().panel_fill = Color32::BLACK;
                Self::panel_card(ui, 10, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.;
                    let requests = self.social.requests() as u32;
                    let home = Person { display_name: "Friends".into(), ..Person::default() };
                    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.), egui::Sense::click());
                    let selected = self.open_chat.is_none();
                    let hover = ui.ctx().animate_bool_with_time(response.id.with("home"), response.hovered(), self.motion());
                    ui.painter().rect_filled(
                        rect,
                        8.,
                        Color32::from_white_alpha(if selected { 22 } else { (hover * 16.) as u8 }),
                    );
                    ui.painter().text(
                        rect.left_center() + egui::vec2(14., 0.),
                        egui::Align2::LEFT_CENTER,
                        home.display_name,
                        egui::FontId::proportional(16.),
                        ui.visuals().text_color(),
                    );
                    if requests > 0 {
                        let at = egui::pos2(rect.right() - 18., rect.center().y);
                        ui.painter().circle_filled(at, 9., RED);
                        ui.painter().text(at, egui::Align2::CENTER_CENTER, requests.to_string(), egui::FontId::proportional(10.5), Color32::WHITE);
                    }
                    if response.clicked() {
                        self.open_chat = None;
                    }
                    ui.add_space(8.);
                    ui.label(RichText::new("DIRECT MESSAGES").small().bold().color(MUTED));
                    ui.add_space(2.);
                    let mut friends = self.social.friends.clone();
                    friends.sort_by_key(|f| (!f.online, f.listening.is_none(), f.person.display_name.to_lowercase()));
                    let room = (ui.available_height() - 70.).max(60.);
                    egui::ScrollArea::vertical().id_salt("dm-list").max_height(room).auto_shrink([false, false]).show(ui, |ui| {
                        if friends.is_empty() {
                            ui.add_space(6.);
                            ui.label(RichText::new("Add friends to chat and see what they are listening to.").small().color(MUTED));
                        }
                        for friend in &friends {
                            let id = friend.person.id.clone();
                            let unread = self.social.unread.get(&id).copied().unwrap_or(0);
                            let line = self.friend_line(friend);
                            let active = self.open_chat.as_deref() == Some(id.as_str());
                            let (_, response) = self.person_row(ui, &id, &friend.person, Some(friend.online), line, active, 52., unread);
                            if response.clicked() {
                                self.open_friend(&id);
                            }
                        }
                    });
                    ui.add_space(4.);
                    self.own_status(ui);
                });
            });
    }

    /// Your own picture and status, pinned to the bottom of the Friends sidebar.
    fn own_status(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 54.), egui::Sense::click());
        ui.painter().rect_filled(rect, 10., Color32::from_white_alpha(if response.hovered() { 24 } else { 14 }));
        let art = egui::Rect::from_min_size(rect.left_top() + egui::vec2(8., 9.), egui::vec2(36., 36.));
        roy_account_ui::paint_avatar(ui, &self.roy_account, art);
        shop_ui::paint_decoration(ui, art, &self.roy_account.equipped());
        let hidden = self.social.invisible;
        let at = art.right_bottom() - egui::vec2(5., 5.);
        ui.painter().circle_filled(at, 7.5, Color32::from_gray(0x2A));
        ui.painter().circle_filled(at, 5., if hidden { OFFLINE } else { GREEN });
        let mut text = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("own-status-text")
                .max_rect(egui::Rect::from_min_max(egui::pos2(art.right() + 10., rect.top()), rect.right_bottom() - egui::vec2(8., 0.)))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        text.spacing_mut().item_spacing.y = 0.;
        text.add_space(9.);
        text.add(egui::Label::new(RichText::new(self.roy_account.name()).bold()).truncate().selectable(false));
        text.add(egui::Label::new(RichText::new(if hidden { "Invisible" } else { "Online" }).small().color(MUTED)).selectable(false));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Your status"));
        egui::Popup::menu(&response)
            .align(egui::RectAlign::TOP_START)
            .show(|ui| self.own_profile_popover(ui));
    }

    /// Discord-style profile that opens from your status card: cover, name, bio, what you are playing,
    /// then Edit profile, your status and your Orbs.
    fn own_profile_popover(&mut self, ui: &mut egui::Ui) {
        // As wide as the Direct Messages column it opens from.
        ui.set_width(190.);
        ui.spacing_mut().item_spacing.y = 4.;
        let (banner, _) = ui.allocate_exact_size(egui::vec2(190., 64.), egui::Sense::hover());
        let top = egui::CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 };
        let tint = roy_account_ui::tint_for(self.roy_account.name());
        ui.painter().rect_filled(
            banner,
            top,
            Color32::from_rgb(
                (tint.r() as f32 * 1.35) as u8,
                (tint.g() as f32 * 1.35) as u8,
                (tint.b() as f32 * 1.35) as u8,
            ),
        );
        if let Some(uri) = self.roy_account.banner_uri() {
            Self::paint_cover(ui, banner, &uri, top);
        }
        let ring = ui.visuals().window_fill;
        let art = egui::Rect::from_min_size(banner.left_bottom() + egui::vec2(12., -28.), egui::vec2(60., 60.));
        ui.painter().circle_filled(art.center(), 33., ring);
        roy_account_ui::paint_avatar(ui, &self.roy_account, art);
        let hidden = self.social.invisible;
        let dot = art.right_bottom() - egui::vec2(8., 8.);
        ui.painter().circle_filled(dot, 9., ring);
        ui.painter().circle_filled(dot, 6.5, if hidden { OFFLINE } else { GREEN });
        ui.add_space(36.);
        egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 0)).show(ui, |ui| {
            ui.set_width(170.);
            ui.add(egui::Label::new(RichText::new(self.roy_account.name()).size(18.).bold()).truncate());
            ui.add(egui::Label::new(RichText::new(format!("@{}", self.roy_account.username())).color(MUTED)).truncate());
            let bio = self.roy_account.bio().to_owned();
            if !bio.is_empty() {
                ui.add_space(4.);
                ui.add(egui::Label::new(RichText::new(bio)).wrap());
            }
            let playing = self.player["track_id"].as_str().and_then(|id| self.library.tracks.get(id)).cloned();
            if let Some(track) = playing {
                ui.add_space(8.);
                let position = self.player["position"].as_f64().unwrap_or(0.);
                let listening = Listening {
                    title: track.title.clone(),
                    artist: track.artist.clone(),
                    cover: track.cover.clone(),
                    duration: self.player["duration"].as_f64().filter(|d| *d > 0.).unwrap_or(track.duration),
                    position,
                    playing: self.player["paused"] == false,
                    at: 0,
                };
                egui::Frame::new()
                    .fill(Color32::from_white_alpha(10))
                    .corner_radius(10)
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new("LISTENING TO").small().bold().color(MUTED));
                        ui.add_space(6.);
                        self.listen_card(ui, &listening, position);
                    });
            }
            ui.add_space(8.);
            egui::Frame::new()
                .fill(Color32::from_white_alpha(10))
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(4, 4))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 2.;
                    let row = |ui: &mut egui::Ui, text: &str| {
                        let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.), egui::Sense::click());
                        if response.hovered() {
                            ui.painter().rect_filled(rect, 8., Color32::from_white_alpha(14));
                        }
                        ui.painter().text(
                            rect.left_center() + egui::vec2(10., 0.),
                            egui::Align2::LEFT_CENTER,
                            text,
                            egui::FontId::proportional(14.),
                            ui.visuals().text_color(),
                        );
                        response
                    };
                    if row(ui, "Edit profile").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        self.roy_account.start_editing();
                        ui.close();
                    }
                    // One tap switches between Online and Invisible.
                    let status = row(ui, if hidden { "      Invisible" } else { "      Online" });
                    let centre = status.rect.left_center() + egui::vec2(19., 0.);
                    ui.painter().circle_filled(centre, 5.5, if hidden { OFFLINE } else { GREEN });
                    if status
                        .on_hover_text("Tap to switch between Online and Invisible")
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        self.social.set_invisible(!hidden);
                    }
                    let orbs = self.roy_account.orbs();
                    let response = row(ui, &format!("      {orbs} Orbs"));
                    // A small diamond stands in for the Orbs icon.
                    let c = response.rect.left_center() + egui::vec2(20., 0.);
                    let points = [(0., -7.), (7., 0.), (0., 7.), (-7., 0.)].map(|(x, y)| c + egui::vec2(x, y)).to_vec();
                    ui.painter().add(egui::Shape::convex_polygon(
                        points,
                        Color32::from_rgb(0x4F, 0xD1, 0xC5).gamma_multiply(0.25),
                        egui::Stroke::new(1.6_f32, Color32::from_rgb(0x4F, 0xD1, 0xC5)),
                    ));
                    if response.on_hover_text("Open the shop and daily quests").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        self.open_shop();
                        ui.close();
                    }
                });
            ui.add_space(8.);
        });
        let card = egui::Rect::from_min_max(banner.min, egui::pos2(banner.right(), ui.min_rect().bottom()));
        let worn = self.roy_account.equipped();
        shop_ui::paint_effect(ui, card, &worn);
        shop_ui::paint_frame(ui, card, &worn, 40.);
    }

    /// Right panel while Friends is open: who is listening now, or the profile of the open chat.
    pub(super) fn friends_side_panel(&mut self, ctx: &egui::Context) {
        self.side_visible = true;
        egui::SidePanel::right("now-panel")
            .exact_width(300.)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(Self::chrome(ctx))
                    .inner_margin(egui::Margin { left: 4, right: 8, top: 0, bottom: 4 }),
            )
            .show(ctx, |ui| {
                ui.visuals_mut().panel_fill = Color32::BLACK;
                Self::panel_card(ui, 0, |ui| {
                    let chosen = self.open_chat.as_deref().and_then(|id| self.social.friend(id)).cloned();
                    egui::ScrollArea::vertical().id_salt("friends-side").auto_shrink([false, false]).show(ui, |ui| {
                        if let Some(friend) = chosen {
                            self.profile_card(ui, &friend);
                        } else {
                            ui.add_space(16.);
                            ui.horizontal(|ui| {
                                ui.add_space(16.);
                                ui.label(RichText::new("Active now").size(20.).bold());
                            });
                            ui.add_space(8.);
                            let active: Vec<Friend> = self.social.friends.iter().filter(|f| f.online && f.listening.is_some()).cloned().collect();
                            if active.is_empty() {
                                ui.add_space(8.);
                                ui.horizontal(|ui| {
                                    ui.add_space(16.);
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new("Nobody is listening right now. When a friend plays something, it shows up here.")
                                                .color(MUTED),
                                        )
                                        .wrap(),
                                    );
                                });
                            }
                            for friend in active {
                                egui::Frame::new()
                                    .fill(Color32::from_white_alpha(10))
                                    .corner_radius(10)
                                    .inner_margin(egui::Margin::same(12))
                                    .outer_margin(egui::Margin::symmetric(12, 4))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            let (rect, _) = ui.allocate_exact_size(egui::vec2(28., 28.), egui::Sense::hover());
                                            paint_person(ui, rect, &friend.person.display_name, self.social.avatar_uri(&friend.person), Some(true), Color32::from_gray(0x25));
                                            ui.add(egui::Label::new(RichText::new(&friend.person.display_name).bold()).truncate().selectable(false));
                                        });
                                        ui.add_space(6.);
                                        if let Some(listening) = &friend.listening {
                                            self.listen_card(ui, listening, self.social.position_of(listening));
                                        }
                                    })
                                    .response
                                    .interact(egui::Sense::click())
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                    .then(|| self.open_friend(&friend.person.id));
                            }
                        }
                    });
                });
            });
    }

    /// The song a friend is playing: cover, title, artist and a moving progress line.
    fn listen_card(&self, ui: &mut egui::Ui, listening: &Listening, position: f64) {
        ui.horizontal(|ui| {
            self.artwork(ui, listening.cover.as_deref(), 52., 1.);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.;
                ui.add(egui::Label::new(RichText::new(&listening.title).bold()).truncate().selectable(false));
                ui.add(egui::Label::new(RichText::new(&listening.artist).small().color(MUTED)).truncate().selectable(false));
                ui.label(
                    RichText::new(if listening.playing { "Listening on Roy Music" } else { "Paused" })
                        .small()
                        .color(if listening.playing { GREEN } else { MUTED }),
                );
            });
        });
        if listening.duration > 0. {
            ui.add_space(6.);
            let (bar, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 4.), egui::Sense::hover());
            ui.painter().rect_filled(bar, 2., Color32::from_white_alpha(28));
            let done = (position / listening.duration).clamp(0., 1.) as f32;
            ui.painter().rect_filled(
                egui::Rect::from_min_size(bar.min, egui::vec2(bar.width() * done, 4.)),
                2.,
                ui.visuals().selection.bg_fill,
            );
            ui.horizontal(|ui| {
                ui.label(RichText::new(time(position)).monospace().size(11.).color(MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(time(listening.duration)).monospace().size(11.).color(MUTED));
                });
            });
            if listening.playing {
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            }
        }
    }

    /// Discord-style profile: coloured banner, round picture over it, name, bio and what they are playing.
    fn profile_card(&mut self, ui: &mut egui::Ui, friend: &Friend) {
        let person = &friend.person;
        let width = ui.available_width();
        let (banner, _) = ui.allocate_exact_size(egui::vec2(width, 96.), egui::Sense::hover());
        let tint = roy_account_ui::tint_for(&person.display_name);
        let top = egui::CornerRadius { nw: PANEL_RADIUS, ne: PANEL_RADIUS, sw: 0, se: 0 };
        ui.painter().rect_filled(
            banner,
            top,
            Color32::from_rgb(
                (tint.r() as f32 * 1.35) as u8,
                (tint.g() as f32 * 1.35) as u8,
                (tint.b() as f32 * 1.35) as u8,
            ),
        );
        if let Some(uri) = self.social.banner_uri(person) {
            Self::paint_cover(ui, banner, &uri, top);
        }
        let art = egui::Rect::from_min_size(banner.left_bottom() + egui::vec2(16., -36.), egui::vec2(76., 76.));
        ui.painter().circle_filled(art.center(), 42., ui.visuals().panel_fill);
        paint_person(ui, art, &person.display_name, self.social.avatar_uri(person), Some(friend.online), ui.visuals().panel_fill);
        shop_ui::paint_decoration(ui, art, &person.equipped);
        ui.add_space(46.);
        egui::Frame::new().inner_margin(egui::Margin::symmetric(16, 0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(&person.display_name).size(20.).bold());
            ui.label(RichText::new(format!("@{}", person.username)).color(MUTED));
            ui.add_space(2.);
            let (line, color) = self.friend_line(friend);
            if friend.listening.is_none() {
                ui.label(RichText::new(line).small().color(color));
            }
            if !person.bio.is_empty() {
                ui.add_space(8.);
                ui.add(egui::Label::new(RichText::new(&person.bio)).wrap());
            }
            self.social.want_person(&person.id);
            if let Some(info) = self.social.people.get(&person.id).cloned() {
                let mutual: Vec<String> = info["mutual"].as_array().map(|list| list.iter().filter_map(|p| p["display_name"].as_str().map(str::to_owned)).collect()).unwrap_or_default();
                if !mutual.is_empty() {
                    ui.add_space(8.);
                    let shown = mutual.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
                    let more = if mutual.len() > 3 { format!(" +{}", mutual.len() - 3) } else { String::new() };
                    ui.label(RichText::new(format!("{} mutual friend{}", mutual.len(), if mutual.len() == 1 { "" } else { "s" })).small().bold());
                    ui.label(RichText::new(format!("{shown}{more}")).small().color(MUTED));
                }
                let date = |key: &str| info[key].as_i64().and_then(|s| Local.timestamp_opt(s, 0).single()).map(|d| d.format("%d %B %Y").to_string());
                ui.add_space(8.);
                if let Some(since) = date("friends_since") {
                    ui.label(RichText::new("FRIENDS SINCE").small().bold().color(MUTED));
                    ui.label(RichText::new(since).small());
                }
                if let Some(joined) = date("joined") {
                    ui.add_space(4.);
                    ui.label(RichText::new("MEMBER SINCE").small().bold().color(MUTED));
                    ui.label(RichText::new(joined).small());
                }
            }
            ui.add_space(12.);
            egui::Frame::new()
                .fill(Color32::from_white_alpha(10))
                .corner_radius(10)
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("LISTENING TO").small().bold().color(MUTED));
                    ui.add_space(6.);
                    match &friend.listening {
                        Some(listening) => {
                            self.listen_card(ui, listening, self.social.position_of(listening));
                            ui.add_space(8.);
                            if ui.button("Find this song").clicked() {
                                self.query = format!("{} {}", listening.title, listening.artist);
                                self.search_source = 0;
                                self.submit_search();
                            }
                        }
                        None => {
                            ui.label(RichText::new("Not playing anything right now.").color(MUTED));
                        }
                    }
                });
            ui.add_space(12.);
            ui.menu_button("More", |ui| {
                super::menu_style(ui);
                if ui.button("Blend our music").clicked() {
                    self.social.fetch_taste(&person.id, &person.display_name);
                    self.friends_notice = Some((format!("Blending with {}…", person.display_name), false, Instant::now()));
                    ui.close();
                }
                if ui.button("Remove friend").clicked() {
                    let sure = rfd::MessageDialog::new()
                        .set_title("Remove friend?")
                        .set_description(format!("{} will be removed from your friends and your chat with them is deleted.", person.display_name))
                        .set_buttons(rfd::MessageButtons::YesNo)
                        .show()
                        == rfd::MessageDialogResult::Yes;
                    if sure {
                        self.social.remove(&person.id);
                        self.open_chat = None;
                    }
                    ui.close();
                }
            });
        });
        // What they wear on the whole card: an effect drifting over it, then the frame around it.
        let card = egui::Rect::from_min_max(banner.min, egui::pos2(banner.right(), ui.min_rect().bottom()));
        shop_ui::paint_effect(ui, card, &person.equipped);
        shop_ui::paint_frame(ui, card, &person.equipped, 40.);
    }

    /// The main area of the Friends space: the lists, or the open conversation.
    pub(super) fn friends_page(&mut self, ui: &mut egui::Ui) {
        ui.visuals_mut().panel_fill = Color32::BLACK;
        if !self.social.signed_in() {
            ui.add_space(40.);
            ui.label(RichText::new("Sign in with your Roynit ID to add friends and chat.").size(18.).color(MUTED));
            return;
        }
        if let Some(id) = self.open_chat.clone()
            && let Some(friend) = self.social.friend(&id).cloned()
        {
            self.chat_view(ui, &friend);
            return;
        }
        self.friends_header(ui);
        if let Some((text, error, _)) = self.friends_notice.clone() {
            ui.add_space(6.);
            ui.label(RichText::new(text).color(if error { Color32::from_rgb(255, 170, 178) } else { GREEN }));
        }
        ui.add_space(8.);
        match self.friends_tab {
            FriendsTab::Online | FriendsTab::All => self.friend_list(ui),
            FriendsTab::Pending => self.pending_list(ui),
            FriendsTab::Add => self.add_friend(ui),
        }
    }

    fn friends_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 18.;
            ui.label(RichText::new("Friends").size(26.).bold());
            let pending = self.social.requests();
            for (tab, label) in [
                (FriendsTab::Online, "Online".to_owned()),
                (FriendsTab::All, "All".to_owned()),
                (FriendsTab::Pending, if pending > 0 { format!("Pending ({pending})") } else { "Pending".to_owned() }),
            ] {
                if Self::tab(ui, &label, self.friends_tab == tab, 16.).clicked() {
                    self.friends_tab = tab;
                }
            }
            let accent = ui.visuals().selection.bg_fill;
            let on_accent = ui.visuals().selection.stroke.color;
            let adding = self.friends_tab == FriendsTab::Add;
            let button = egui::Button::new(RichText::new("Add friend").bold().color(if adding { accent } else { on_accent }))
                .fill(if adding { accent.gamma_multiply(0.2) } else { accent })
                .corner_radius(8);
            if ui.add(button).clicked() {
                self.friends_tab = FriendsTab::Add;
            }
        });
    }

    fn friend_list(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(ui.visuals().extreme_bg_color)
            .stroke(egui::Stroke::new(1.0_f32, Color32::from_white_alpha(16)))
            .corner_radius(22)
            .inner_margin(egui::Margin::symmetric(14, 4))
            .show(ui, |ui| {
                ui.set_width(ui.available_width().min(420.));
                ui.horizontal(|ui| {
                    ui.label(RichText::new("🔍").color(MUTED));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.friend_filter)
                            .frame(false)
                            .hint_text("Search friends")
                            .desired_width(f32::INFINITY),
                    );
                });
            });
        ui.add_space(8.);
        let needle = self.friend_filter.trim().to_lowercase();
        let only_online = self.friends_tab == FriendsTab::Online;
        let mut friends: Vec<Friend> = self
            .social
            .friends
            .iter()
            .filter(|f| (!only_online || f.online) && (needle.is_empty() || format!("{} {}", f.person.display_name, f.person.username).to_lowercase().contains(&needle)))
            .cloned()
            .collect();
        friends.sort_by_key(|f| (!f.online, f.listening.is_none(), f.person.display_name.to_lowercase()));
        if friends.is_empty() {
            ui.add_space(30.);
            let message = if !self.social.loaded {
                "Loading your friends…"
            } else if self.social.friends.is_empty() {
                "No friends yet. Add someone with their Roynit username."
            } else if only_online {
                "Nobody is online right now."
            } else {
                "No friends match your search."
            };
            ui.label(RichText::new(message).size(16.).color(MUTED));
            if self.social.loaded && self.social.friends.is_empty() {
                ui.add_space(6.);
                let outline = egui::Button::new(RichText::new("Add a friend").bold())
                    .fill(Color32::TRANSPARENT)
                    .stroke(egui::Stroke::new(1.0_f32, Color32::from_gray(0x7C)))
                    .corner_radius(18)
                    .min_size(egui::vec2(130., 36.));
                if ui.add(outline).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    self.friends_tab = FriendsTab::Add;
                }
            }
            ui.add_space(18.);
            self.suggestions_block(ui);
            return;
        }
        let word = if only_online { "Online" } else { "All friends" };
        ui.label(RichText::new(format!("{word} — {}", friends.len())).small().bold().color(MUTED));
        ui.add_space(4.);
        egui::ScrollArea::vertical().id_salt("friend-rows").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.;
            for friend in &friends {
                let id = friend.person.id.clone();
                let line = self.friend_line(friend);
                let (rect, response) = self.person_row(ui, &id, &friend.person, Some(friend.online), line, false, 60., 0);
                let mut actions = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt(("friend-actions", &id))
                        .max_rect(rect.shrink2(egui::vec2(10., 0.)))
                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                );
                actions.menu_button(RichText::new("•••").color(MUTED), |ui| {
                    super::menu_style(ui);
                    if ui.button("Message").clicked() {
                        self.open_friend(&id);
                        ui.close();
                    }
                    if ui.button("Blend our music").clicked() {
                        self.social.fetch_taste(&id, &friend.person.display_name);
                        self.friends_notice = Some((format!("Blending with {}…", friend.person.display_name), false, Instant::now()));
                        ui.close();
                    }
                    if ui.button("Remove friend").clicked() {
                        let sure = rfd::MessageDialog::new()
                            .set_title("Remove friend?")
                            .set_description(format!("{} will be removed and your chat with them is deleted.", friend.person.display_name))
                            .set_buttons(rfd::MessageButtons::YesNo)
                            .show()
                            == rfd::MessageDialogResult::Yes;
                        if sure {
                            self.social.remove(&id);
                        }
                        ui.close();
                    }
                });
                if actions.button("Message").clicked() || response.clicked() {
                    self.open_friend(&id);
                }
            }
            ui.add_space(14.);
            self.suggestions_block(ui);
        });
    }

    fn pending_list(&mut self, ui: &mut egui::Ui) {
        let (incoming, outgoing) = (self.social.incoming.clone(), self.social.outgoing.clone());
        if incoming.is_empty() && outgoing.is_empty() {
            ui.add_space(30.);
            ui.label(RichText::new("No pending requests.").size(16.).color(MUTED));
            return;
        }
        egui::ScrollArea::vertical().id_salt("pending-rows").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.;
            for (title, people, mine) in [("Incoming", &incoming, false), ("Sent", &outgoing, true)] {
                if people.is_empty() {
                    continue;
                }
                ui.label(RichText::new(format!("{title} — {}", people.len())).small().bold().color(MUTED));
                for person in people {
                    let line = (format!("@{}", person.username), MUTED);
                    let (rect, _) = self.person_row(ui, &person.id, person, None, line, false, 60., 0);
                    let mut actions = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt(("pending-actions", &person.id))
                            .max_rect(rect.shrink2(egui::vec2(10., 0.)))
                            .layout(egui::Layout::right_to_left(egui::Align::Center)),
                    );
                    if actions.button(if mine { "Cancel" } else { "Decline" }).clicked() {
                        self.social.answer(&person.id, false);
                    }
                    if !mine && actions.button(RichText::new("Accept").bold()).clicked() {
                        self.social.answer(&person.id, true);
                    }
                }
                ui.add_space(10.);
            }
        });
    }

    /// "People you may know": friends of your friends, with how many you share and a one-click Add.
    fn suggestions_block(&mut self, ui: &mut egui::Ui) {
        let people: Vec<Person> = self.social.suggestions.clone();
        if people.is_empty() {
            return;
        }
        ui.label(RichText::new(format!("People you may know — {}", people.len())).small().bold().color(MUTED));
        ui.add_space(4.);
        ui.spacing_mut().item_spacing.y = 2.;
        for person in &people {
            let mutual = if person.mutual == 1 { "1 mutual friend".to_owned() } else { format!("{} mutual friends", person.mutual) };
            let line = (format!("@{}  •  {mutual}", person.username), MUTED);
            let (rect, _) = self.person_row(ui, &format!("suggest-{}", person.id), person, None, line, false, 60., 0);
            let mut actions = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(("suggest-actions", &person.id))
                    .max_rect(rect.shrink2(egui::vec2(10., 0.)))
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            let accent = actions.visuals().selection.bg_fill;
            let on_accent = actions.visuals().selection.stroke.color;
            let add = egui::Button::new(RichText::new("Add").bold().color(on_accent)).fill(accent).corner_radius(16).min_size(egui::vec2(70., 30.));
            if actions.add(add).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.social.add_friend(&person.username);
                self.social.suggestions.retain(|p| p.id != person.id);
            }
        }
        ui.add_space(10.);
    }

    fn add_friend(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Add friend").size(20.).bold());
        ui.label(RichText::new("Add a friend with their Roynit username.").color(MUTED));
        ui.add_space(10.);
        let mut submit = false;
        egui::Frame::new()
            .fill(ui.visuals().extreme_bg_color)
            .stroke(egui::Stroke::new(1.0_f32, Color32::from_white_alpha(20)))
            .corner_radius(26)
            .inner_margin(egui::Margin { left: 18, right: 6, top: 6, bottom: 6 })
            .show(ui, |ui| {
                ui.set_width(ui.available_width().min(560.));
                ui.horizontal(|ui| {
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut self.friend_input)
                            .frame(false)
                            .hint_text("Enter a username")
                            .char_limit(25)
                            .margin(egui::vec2(0., 10.))
                            .desired_width(ui.available_width() - 160.),
                    );
                    submit |= field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let accent = ui.visuals().selection.bg_fill;
                        let on_accent = ui.visuals().selection.stroke.color;
                        let ready = !self.friend_input.trim().is_empty();
                        // Grey and quiet until there is a name to send, then the accent colour.
                        let (fill, ink) = if ready {
                            (accent, on_accent)
                        } else {
                            (Color32::from_white_alpha(18), MUTED)
                        };
                        let button = egui::Button::new(RichText::new("Send request").bold().color(ink))
                            .fill(fill)
                            .corner_radius(20)
                            .min_size(egui::vec2(130., 38.));
                        let response = ui.add(button);
                        submit |= ready && response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
                    });
                });
            });
        if submit && !self.friend_input.trim().is_empty() {
            let name = std::mem::take(&mut self.friend_input);
            self.social.add_friend(name.trim());
        }
        ui.add_space(14.);
        let me = self.roy_account.username();
        if !me.is_empty() {
            ui.label(RichText::new(format!("Your username is @{me}. Share it so friends can find you.")).color(MUTED));
        }
        ui.add_space(18.);
        self.suggestions_block(ui);
    }

    fn chat_view(&mut self, ui: &mut egui::Ui, friend: &Friend) {
        const QUICK: [&str; 6] = ["❤", "😆", "😮", "😢", "😡", "👍"];
        let id = friend.person.id.clone();
        ui.horizontal(|ui| {
            if Self::icon_button(ui, "←", "Back to friends", false).clicked() {
                self.open_chat = None;
            }
            let (rect, _) = ui.allocate_exact_size(egui::vec2(36., 36.), egui::Sense::hover());
            paint_person(ui, rect, &friend.person.display_name, self.social.avatar_uri(&friend.person), Some(friend.online), ui.visuals().panel_fill);
            shop_ui::paint_decoration(ui, rect, &friend.person.equipped);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.;
                ui.label(RichText::new(&friend.person.display_name).size(18.).bold());
                let (line, color) = self.friend_line(friend);
                ui.label(RichText::new(line).small().color(color));
            });
        });
        ui.add_space(6.);
        ui.separator();
        let replying = self.social.chats.get(&id).and_then(|c| c.replying.clone());
        let input_height = 78. + if replying.is_some() { 46. } else { 0. };
        let room = (ui.available_height() - input_height).max(80.);
        let width = ui.available_width();
        let accent = ui.visuals().selection.bg_fill;
        let messages = self.social.chats.get(&id).map(|c| c.messages.clone()).unwrap_or_default();
        let reactions = self.social.chats.get(&id).map(|c| c.reactions.clone()).unwrap_or_default();
        let name = friend.person.display_name.clone();
        egui::ScrollArea::vertical()
            .id_salt(("chat", &id))
            .max_height(room)
            .min_scrolled_height(room)
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 3.;
                if messages.is_empty() {
                    ui.add_space(24.);
                    ui.label(RichText::new(format!("This is the start of your chat with {}.", friend.person.display_name)).color(MUTED));
                }
                let mut day = String::new();
                for (i, message) in messages.iter().enumerate() {
                    let label = day_label(message.at);
                    if label != day {
                        ui.add_space(8.);
                        ui.vertical_centered(|ui| ui.label(RichText::new(&label).small().bold().color(MUTED)));
                        ui.add_space(4.);
                        day = label;
                    }
                    let next = messages.get(i + 1);
                    let last_of_run = next.is_none_or(|n| n.mine != message.mine || n.at - message.at > 300);
                    // A message that is only an emoji is shown big, with no bubble.
                    let emoji_only = !message.body.is_empty() && message.body.chars().count() <= 8 && message.body.chars().all(|c| !c.is_alphanumeric() && !c.is_whitespace());
                    // The emoji font has no selector for "draw it as a picture", so that would show as an empty box.
                    let plain = |text: &str| text.replace('\u{fe0f}', "");
                    let marks: Vec<(String, bool)> = reactions.get(&message.id).map(|l| l.iter().map(|(e, m)| (plain(e), *m)).collect()).unwrap_or_default();
                    let mine_now = marks.iter().find(|(_, mine)| *mine).map(|(e, _)| e.clone());
                    let row = if message.mine { egui::Layout::right_to_left(egui::Align::Min) } else { egui::Layout::left_to_right(egui::Align::Min) };
                    ui.with_layout(row, |ui| {
                        if !message.mine {
                            let (slot, _) = ui.allocate_exact_size(egui::vec2(28., 28.), egui::Sense::hover());
                            if last_of_run {
                                paint_person(ui, slot, &friend.person.display_name, self.social.avatar_uri(&friend.person), None, ui.visuals().panel_fill);
                            }
                        }
                        let column = if message.mine { egui::Layout::top_down(egui::Align::Max) } else { egui::Layout::top_down(egui::Align::Min) };
                        ui.with_layout(column, |ui| {
                            ui.spacing_mut().item_spacing.y = 2.;
                            if let Some(quote) = &message.reply {
                                let who = if message.mine { "You" } else { name.as_str() };
                                let target = match (quote.mine, message.mine) {
                                    (true, true) => "yourself".to_owned(),
                                    (true, false) => "you".to_owned(),
                                    (false, true) => name.clone(),
                                    (false, false) => "themselves".to_owned(),
                                };
                                ui.label(RichText::new(format!("↩ {who} replied to {target}")).small().color(MUTED));
                                egui::Frame::new().fill(Color32::from_white_alpha(10)).corner_radius(12).inner_margin(egui::Margin::symmetric(10, 5)).show(ui, |ui| {
                                    ui.set_max_width((width * 0.6).max(140.));
                                    ui.add(egui::Label::new(RichText::new(plain(&quote.body)).small().color(MUTED)).truncate());
                                });
                            }
                            let frame = if emoji_only {
                                egui::Frame::new().inner_margin(egui::Margin::symmetric(2, 0))
                            } else {
                                egui::Frame::new()
                                    .fill(if message.mine { accent.gamma_multiply(0.38) } else { Color32::from_white_alpha(18) })
                                    .corner_radius(14)
                                    .inner_margin(egui::Margin::symmetric(12, 7))
                            };
                            let bubble = frame
                                .show(ui, |ui| {
                                    ui.set_max_width((width * 0.7).max(160.));
                                    let text = RichText::new(plain(&message.body));
                                    ui.add(egui::Label::new(if emoji_only { text.size(34.) } else { text }).wrap().selectable(false));
                                })
                                .response
                                .interact(egui::Sense::click())
                                .on_hover_text("Right-click to react or reply");
                            bubble.context_menu(|ui| {
                                ui.horizontal(|ui| {
                                    for e in QUICK {
                                        let chosen = mine_now.as_deref() == Some(e);
                                        let button = egui::Button::new(RichText::new(e).size(22.)).frame(chosen).min_size(egui::vec2(34., 34.));
                                        if ui.add(button).clicked() {
                                            self.social.react(&id, message.id, if chosen { None } else { Some(e) });
                                            ui.close();
                                        }
                                    }
                                });
                                ui.separator();
                                if ui.button("↩  Reply").clicked() {
                                    if let Some(chat) = self.social.chats.get_mut(&id) {
                                        chat.replying = Some(message.clone());
                                    }
                                    ui.close();
                                }
                                if ui.button("Copy").clicked() {
                                    ui.ctx().copy_text(message.body.clone());
                                    ui.close();
                                }
                            });
                            if !marks.is_empty() {
                                let mut shown: Vec<&str> = marks.iter().map(|(e, _)| e.as_str()).collect();
                                shown.dedup();
                                let text = format!("{}{}", shown.join(""), if marks.len() > 1 { format!(" {}", marks.len()) } else { String::new() });
                                egui::Frame::new().fill(Color32::from_white_alpha(14)).corner_radius(10).inner_margin(egui::Margin::symmetric(6, 1)).show(ui, |ui| {
                                    ui.label(RichText::new(text).size(13.));
                                });
                            }
                            if last_of_run {
                                ui.label(RichText::new(clock(message.at)).size(10.5).color(MUTED.gamma_multiply(0.8)));
                                ui.add_space(4.);
                            }
                        });
                    });
                }
            });
        if let Some(target) = &replying {
            ui.add_space(4.);
            egui::Frame::new().fill(Color32::from_white_alpha(10)).corner_radius(10).inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.;
                        let who = if target.mine { "yourself".to_owned() } else { friend.person.display_name.clone() };
                        ui.label(RichText::new(format!("Replying to {who}")).small().bold());
                        ui.add(egui::Label::new(RichText::new(&target.body).small().color(MUTED)).truncate());
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new("✕").frame(false)).clicked() {
                            if let Some(chat) = self.social.chats.get_mut(&id) {
                                chat.replying = None;
                            }
                        }
                    });
                });
            });
        }
        let mut send = false;
        ui.add_space(6.);
        ui.horizontal(|ui| {
            egui::Frame::new()
                .fill(ui.visuals().extreme_bg_color)
                .stroke(egui::Stroke::new(1.0_f32, Color32::from_white_alpha(20)))
                .corner_radius(22)
                .inner_margin(egui::Margin::symmetric(16, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width() - 56.);
                    ui.horizontal(|ui| {
                        let draft = &mut self.social.chats.entry(id.clone()).or_default().draft;
                        let field = ui.add(
                            egui::TextEdit::singleline(draft)
                                .id(egui::Id::new(("chat-input", &id)))
                                .frame(false)
                                .char_limit(1000)
                                .hint_text(format!("Message {}", friend.person.display_name))
                                .desired_width(ui.available_width() - 36.),
                        );
                        if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            send = true;
                            field.request_focus();
                        }
                        egui::containers::menu::MenuButton::new(RichText::new("☺").size(20.).color(MUTED))
                            .config(egui::containers::menu::MenuConfig::default().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside))
                            .ui(ui, |ui| {
                                super::menu_style(ui);
                                ui.set_width(320.);
                                egui::ScrollArea::vertical().max_height(340.).show(ui, |ui| {
                                for (title, set) in EMOJI {
                                    ui.label(RichText::new(*title).small().bold().color(MUTED));
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = egui::vec2(2., 2.);
                                        for e in set.split_whitespace() {
                                            if ui.add(egui::Button::new(RichText::new(e).size(21.)).frame(false).min_size(egui::vec2(30., 30.))).clicked() {
                                                draft.push_str(e);
                                            }
                                        }
                                    });
                                    ui.add_space(4.);
                                }
                                });
                            });
                    });
                });
            let has_text = self.social.chats.get(&id).is_some_and(|c| !c.draft.trim().is_empty());
            let on_accent = ui.visuals().selection.stroke.color;
            if has_text {
                let button = egui::Button::new(RichText::new("Send").bold().color(on_accent)).fill(accent).corner_radius(18);
                send |= ui.add(button).clicked();
            } else {
                const PICKS: [&str; 10] = ["👍", "😆", "😮", "😢", "😡", "🔥", "🙏", "🎵", "💪", "👏"];
                let quick = friend.quick.clone().unwrap_or_else(|| "👍".to_owned());
                let shown = quick.replace('\u{fe0f}', "");
                let button = ui
                    .add(egui::Button::new(RichText::new(shown).size(22.)).frame(false).min_size(egui::vec2(44., 36.)))
                    .on_hover_text("Send your quick icon (right-click to change it for both of you)");
                if button.clicked() {
                    self.social.chats.entry(id.clone()).or_default().draft = quick;
                    send = true;
                }
                button.context_menu(|ui| {
                    ui.label(RichText::new("Quick icon for both of you").small().color(MUTED));
                    ui.horizontal(|ui| {
                        for e in PICKS {
                            if ui.add(egui::Button::new(RichText::new(e).size(22.)).frame(false).min_size(egui::vec2(34., 34.))).clicked() {
                                self.social.set_quick(&id, e);
                                ui.close();
                            }
                        }
                    });
                });
            }
        });
        if send {
            self.social.send(&id);
        }
    }
}

/// Emoji for the picker. The app's emoji font is a few years old, so these stay within what it can draw.
const EMOJI: &[(&str, &str)] = &[
    ("Faces", "😀 😁 😂 😃 😄 😅 😆 😇 😈 😉 😊 😋 😌 😍 😎 😏 😐 😑 😒 😓 😔 😕 😖 😗 😘 😙 😚 😛 😜 😝 😞 😟 😠 😡 😢 😣 😤 😥 😦 😧 😨 😩 😪 😫 😬 😭 😮 😯 😰 😱 😲 😳 😴 😵 😶 😷"),
    ("Hands and hearts", "👍 👎 👊 ✊ ✌ 👌 👈 👉 👆 👇 ☝ ✋ 👋 👏 👐 🙌 🙏 💪 👀 ❤ 💔 💕 💖 💗 💘 💙 💚 💛 💜 💝 💞 💟 💓"),
    ("Animals and food", "🐶 🐱 🐭 🐹 🐰 🐻 🐼 🐨 🐯 🐮 🐷 🐸 🐵 🐔 🐧 🐦 🐢 🐍 🐙 🐠 🐬 🐳 🍎 🍊 🍋 🍌 🍉 🍇 🍓 🍒 🍑 🍍 🍅 🍔 🍕 🍟 🍰 🍩 🍪 ☕ 🍺"),
    ("Music and fun", "🎵 🎶 🎤 🎧 🎸 🎹 🎺 🎻 🎷 🎬 🎮 🎯 ⚽ 🏀 🎉 🎊 🎁 🔥 ✨ ⭐ 🌟 💯 💥 💫 🌙 ☀ ⚡ 🌈"),
];
