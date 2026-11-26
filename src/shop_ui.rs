//! The Orbs shop and the daily quests, and the drawing of what people wear: avatar decorations, nameplates,
//! profile frames and profile effects. The pictures are embedded (assets/shop, drawn by scripts/shop_assets.py).
use super::{App, MUTED, Weight, shop_assets};
use eframe::egui::{self, Color32, RichText};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::HashMap, sync::OnceLock, time::Duration};

#[derive(Deserialize)]
pub struct Variant {
    pub id: String,
    pub label: String,
    pub color: String,
}

#[derive(Deserialize)]
pub struct Item {
    pub id: String,
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub tier: String,
    pub price: u64,
    #[serde(default)]
    pub anim: String,
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub stem: String,
    pub variants: Vec<Variant>,
    #[serde(default)]
    pub sprites: Vec<String>,
    #[serde(default)]
    pub emitter: Value,
    #[serde(default)]
    pub particles: Value,
    #[serde(default)]
    pub fraction: Option<f32>,
    #[serde(default)]
    pub layers: Value,
}

#[derive(Deserialize)]
struct Catalog {
    avatar_fraction: f32,
    frame_edge: f32,
    items: Vec<Item>,
}

fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| serde_json::from_slice(shop_assets::bytes("catalog.json").unwrap_or(b"{}")).unwrap_or(Catalog { avatar_fraction: 0.625, frame_edge: 0.28, items: Vec::new() }))
}

const GREEN: Color32 = Color32::from_rgb(0x3B, 0xA5, 0x5C);

fn hex(text: &str) -> Color32 {
    let n = u32::from_str_radix(text.trim_start_matches('#'), 16).unwrap_or(0xffffff);
    Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8)
}

/// What someone wears of one kind, from their `equipped` map ("item:variant" strings).
pub fn worn<'a>(equipped: &HashMap<String, String>, kind: &str) -> Option<(&'a Item, &'a Variant)> {
    let text = equipped.get(kind)?;
    let (id, variant) = text.split_once(':')?;
    let item = catalog().items.iter().find(|i| i.id == id)?;
    Some((item, item.variants.iter().find(|v| v.id == variant)?))
}

pub fn equipped_of(value: &Value) -> HashMap<String, String> {
    value.as_object().map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned()))).collect()).unwrap_or_default()
}

fn art(ctx: &egui::Context, path: &str) -> Option<String> {
    let bytes = shop_assets::bytes(path)?;
    let uri = format!("bytes://shop/{path}");
    ctx.include_bytes(uri.clone(), bytes);
    Some(uri)
}

fn variant_path(item: &Item, variant: &Variant) -> String {
    format!("{}/{}_{}.webp", item.folder, item.stem, variant.id)
}

fn seconds(ui: &egui::Ui) -> f32 {
    ui.input(|i| i.time) as f32
}

/// A decoration around an avatar: it spins, pulses or floats while it is on screen.
pub fn paint_decoration(ui: &egui::Ui, avatar: egui::Rect, equipped: &HashMap<String, String>) {
    let Some((item, variant)) = worn(equipped, "decoration") else { return };
    let Some(uri) = art(ui.ctx(), &variant_path(item, variant)) else { return };
    let big = avatar.width() / item.fraction.unwrap_or(catalog().avatar_fraction);
    let mut rect = egui::Rect::from_center_size(avatar.center(), egui::Vec2::splat(big));
    let t = seconds(ui);
    let (mut angle, mut alpha) = (0.0, 1.0);
    match item.anim.as_str() {
        "spin" => angle = t * std::f32::consts::TAU / 26.,
        "pulse" => alpha = 0.62 + 0.38 * (0.5 + 0.5 * (t * std::f32::consts::TAU / 3.4).sin()),
        "float" => rect = rect.translate(egui::vec2(0., -avatar.width() * 0.05 * (0.5 + 0.5 * (t * std::f32::consts::TAU / 3.8).sin()))),
        _ => {}
    }
    if item.anim != "none" {
        ui.ctx().request_repaint_after(Duration::from_millis(40));
    }
    if let Some(layers) = item.layers.as_array() {
        // parts that move on their own: ears twitching, paws bobbing, wings beating
        ui.ctx().request_repaint_after(Duration::from_millis(16));
        for l in layers {
            let n = |key: &str, i: usize| l[key].get(i).and_then(Value::as_f64).unwrap_or(0.) as f32;
            let (x0, x1, y0, y1) = (n("x", 0), n("x", 1), n("y", 0), n("y", 1));
            let period = l["period"].as_f64().unwrap_or(3.) as f32;
            let phase = l["phase"].as_f64().unwrap_or(0.) as f32;
            let turn = l["sign"].as_f64().unwrap_or(1.) as f32 * (l["rotate"].as_f64().unwrap_or(0.) as f32).to_radians() * (std::f32::consts::TAU * (t / period + phase)).sin();
            let lift = l["bob"].as_f64().unwrap_or(0.) as f32 * rect.width() * (std::f32::consts::TAU * t / l["bob_period"].as_f64().unwrap_or(2.6) as f32).sin();
            let sub = egui::Rect::from_min_max(egui::pos2(rect.left() + x0 * rect.width(), rect.top() + y0 * rect.height()), egui::pos2(rect.left() + x1 * rect.width(), rect.top() + y1 * rect.height()));
            let uv = egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1));
            let pivot = egui::vec2((n("pivot", 0) - x0) / (x1 - x0), (n("pivot", 1) - y0) / (y1 - y0));
            egui::Image::new(uri.clone()).uv(uv).maintain_aspect_ratio(false).fit_to_exact_size(sub.size()).rotate(turn, pivot).paint_at(ui, sub.translate(egui::vec2(0., lift)));
        }
    } else {
        egui::Image::new(uri).fit_to_exact_size(rect.size()).rotate(angle, egui::Vec2::splat(0.5)).tint(Color32::WHITE.gamma_multiply(alpha)).paint_at(ui, rect);
    }
    if item.particles.is_object() {
        paint_ring_sparkles(ui, rect, item, variant);
        ui.ctx().request_repaint_after(Duration::from_millis(33));
    }
}

/// Sparkles, embers or bubbles drifting in the band around an avatar, over its decoration.
fn paint_ring_sparkles(ui: &egui::Ui, rect: egui::Rect, item: &Item, variant: &Variant) {
    let e = &item.particles;
    let sprites: Vec<String> = e["sprites"].as_array().map(|a| a.iter().filter_map(|s| art(ui.ctx(), &format!("sprites/{}.webp", s.as_str()?))).collect()).unwrap_or_default();
    if sprites.is_empty() {
        return;
    }
    let span = |key: &str, t: f64| {
        let a = e[key].as_array();
        let lo = a.and_then(|a| a.first()).and_then(Value::as_f64).unwrap_or(0.);
        let hi = a.and_then(|a| a.get(1)).and_then(Value::as_f64).unwrap_or(lo);
        lo + (hi - lo) * t
    };
    let tint = if e["tint"].as_bool().unwrap_or(false) { hex(&variant.color) } else { Color32::WHITE };
    let time = ui.input(|i| i.time);
    let width = rect.width() as f64;
    let rise = e["rise"].as_f64().unwrap_or(0.);
    let speed = e["speed"].as_f64().unwrap_or(0.2);
    let mut state = item.id.bytes().fold(0x9E37_79B9_7F4A_7C15_u64, |h, b| (h ^ b as u64).wrapping_mul(0x1000_0000_01B3));
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    for i in 0..e["count"].as_u64().unwrap_or(12) as usize {
        let (angle, rho, size_t, phase, tw_t) = (next() * std::f64::consts::TAU, 0.6 + 0.36 * next(), next(), next(), next());
        let size = span("size", size_t) * width;
        let mut alpha = 1.0;
        let mut lift = 0.0;
        if rise != 0. {
            let p = (phase + time * speed) % 1.0;
            lift = -p * rise * width;
            alpha = (std::f64::consts::PI * p).sin();
        }
        if e["twinkle"].is_array() {
            alpha = 0.12 + 0.88 * (0.5 + 0.5 * (time * span("twinkle", tw_t) + phase * std::f64::consts::TAU).sin());
        }
        let centre = rect.center() + egui::vec2((angle.cos() * rho * width / 2.) as f32, (angle.sin() * rho * width / 2. + lift) as f32);
        let dest = egui::Rect::from_center_size(centre, egui::Vec2::splat(size as f32));
        egui::Image::new(sprites[i % sprites.len()].clone()).fit_to_exact_size(dest.size()).tint(tint.gamma_multiply(alpha as f32)).paint_at(ui, dest);
    }
}

/// A nameplate behind a row. Returns whether one was drawn.
pub fn paint_plate(ui: &egui::Ui, rect: egui::Rect, equipped: &HashMap<String, String>) -> bool {
    let Some((item, variant)) = worn(equipped, "nameplate") else { return false };
    let Some(uri) = art(ui.ctx(), &variant_path(item, variant)) else { return false };
    App::paint_cover(ui, rect, &uri, 12u8);
    true
}

/// A profile frame: corners keep their size, edges stretch.
pub fn paint_frame(ui: &egui::Ui, rect: egui::Rect, equipped: &HashMap<String, String>, corner: f32) {
    let Some((item, variant)) = worn(equipped, "frame") else { return };
    paint_frame_item(ui, rect, item, variant, corner);
}

fn paint_frame_item(ui: &egui::Ui, rect: egui::Rect, item: &Item, variant: &Variant, corner: f32) {
    let Some(uri) = art(ui.ctx(), &variant_path(item, variant)) else { return };
    let e = catalog().frame_edge;
    let us = [0., e, 1. - e, 1.];
    let xs = [rect.left(), rect.left() + corner, rect.right() - corner, rect.right()];
    let ys = [rect.top(), rect.top() + corner, rect.bottom() - corner, rect.bottom()];
    for row in 0..3 {
        for col in 0..3 {
            if row == 1 && col == 1 {
                continue;
            }
            let dest = egui::Rect::from_min_max(egui::pos2(xs[col], ys[row]), egui::pos2(xs[col + 1], ys[row + 1]));
            let uv = egui::Rect::from_min_max(egui::pos2(us[col], us[row]), egui::pos2(us[col + 1], us[row + 1]));
            egui::Image::new(uri.clone()).uv(uv).maintain_aspect_ratio(false).fit_to_exact_size(dest.size()).paint_at(ui, dest);
        }
    }
}

/// A profile effect: sprites drifting over the card, moved by the catalog's numbers.
pub fn paint_effect(ui: &egui::Ui, rect: egui::Rect, equipped: &HashMap<String, String>) {
    let Some((item, variant)) = worn(equipped, "effect") else { return };
    paint_effect_item(ui, rect, item, variant);
}

fn paint_effect_item(ui: &egui::Ui, rect: egui::Rect, item: &Item, variant: &Variant) {
    let e = &item.emitter;
    let count = e["count"].as_u64().unwrap_or(12) as usize;
    let range = |key: &str, t: f64| {
        let a = e[key].as_array();
        let lo = a.and_then(|a| a.first()).and_then(Value::as_f64).unwrap_or(0.);
        let hi = a.and_then(|a| a.get(1)).and_then(Value::as_f64).unwrap_or(lo);
        lo + (hi - lo) * t
    };
    let sprites: Vec<String> = item.sprites.iter().filter_map(|s| art(ui.ctx(), &format!("sprites/{s}.webp"))).collect();
    if sprites.is_empty() {
        return;
    }
    let tint = if e["tint"].as_bool().unwrap_or(false) { hex(&variant.color) } else { Color32::WHITE };
    let fade = e["fade"].as_f64().unwrap_or(0.15);
    let time = ui.input(|i| i.time);
    let width = rect.width() as f64;
    let height = rect.height() as f64;
    let mut state = item.id.bytes().fold(0x9E37_79B9_7F4A_7C15_u64, |h, b| (h ^ b as u64).wrapping_mul(0x1000_0000_01B3));
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    let painter = ui.painter().with_clip_rect(rect);
    for i in 0..count {
        let (s0, s1, s2, s3, x0, phase) = (next(), next(), next(), next(), next(), next());
        let size = range("size", s0) * width;
        let vy = range("vy", s1);
        let sway = range("sway", s2) * width;
        let spin = range("spin", s3);
        let mut alpha = 1.0;
        let y;
        if vy == 0.0 {
            y = (0.05 + 0.9 * s1) * height;
            let tw = range("twinkle", s2).max(0.5);
            alpha = 0.15 + 0.85 * (0.5 + 0.5 * (time * tw + phase * std::f64::consts::TAU).sin());
        } else {
            let lap = 1.0 + 2. * 0.08;
            let progress = (phase + time * vy.abs() / lap) % 1.0;
            y = (if vy > 0. { progress * lap - 0.08 } else { (1. - progress) * lap - 0.08 }) * height;
            alpha = (progress / fade).clamp(0., 1.) * ((1. - progress) / fade).clamp(0., 1.);
        }
        let x = x0 * width + (time * (0.6 + s2) + phase * std::f64::consts::TAU).sin() * sway;
        let centre = rect.min + egui::vec2(x as f32, y as f32);
        let dest = egui::Rect::from_center_size(centre, egui::Vec2::splat(size as f32));
        let angle = (spin * time + phase * std::f64::consts::TAU) as f32;
        egui::Image::new(sprites[i % sprites.len()].clone())
            .fit_to_exact_size(dest.size())
            .rotate(angle, egui::Vec2::splat(0.5))
            .tint(tint.gamma_multiply(alpha as f32))
            .paint_at(ui, dest);
    }
    let _ = painter;
    ui.ctx().request_repaint_after(Duration::from_millis(33));
}

fn tier_color(tier: &str) -> Color32 {
    match tier {
        "rare" => Color32::from_rgb(0x6E, 0xB6, 0xFF),
        "epic" => Color32::from_rgb(0xC0, 0x8B, 0xFF),
        _ => MUTED,
    }
}

impl App {
    pub(super) fn open_shop(&mut self) {
        self.shop_open = true;
        self.social.refresh_shop();
    }

    /// The Orbs shop: daily quests, then decorations, nameplates, frames and effects.
    pub(super) fn shop_dialog(&mut self, ctx: &egui::Context) {
        if !self.shop_open {
            return;
        }
        if let Some(equipped) = self.social.equipped_changed.take() {
            self.roy_account.set_equipped(equipped);
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        let top = self.dialog_top();
        let screen = ctx.content_rect();
        let width = (screen.width() - 120.).clamp(520., 760.);
        let height = (screen.height() - top - 80.).clamp(360., 760.);
        let orbs = self.social.shop.as_ref().and_then(|s| s["orbs"].as_u64()).unwrap_or_else(|| self.roy_account.orbs());
        let mut close = Self::dialog(ctx, top, "shop", true, |ui| {
            ui.set_width(width);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Shop").size(26.).bold());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::Frame::new().fill(Color32::from_white_alpha(14)).corner_radius(14).inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
                        ui.label(RichText::new(format!("{orbs} Orbs")).bold());
                    });
                });
            });
            ui.add_space(6.);
            ui.horizontal(|ui| {
                for (key, label) in [("quests", "Quests"), ("decoration", "Avatar"), ("nameplate", "Nameplates"), ("frame", "Frames"), ("effect", "Effects")] {
                    if Self::tab(ui, label, self.shop_tab == key, 15.).clicked() {
                        self.shop_tab = key.to_owned();
                    }
                }
            });
            if let Some((text, true)) = self.social.shop_notice.clone() {
                ui.add_space(4.);
                ui.label(RichText::new(text).color(Color32::from_rgb(255, 170, 178)));
            }
            ui.add_space(8.);
            egui::ScrollArea::vertical().max_height(height).auto_shrink([false, false]).show(ui, |ui| {
                if self.shop_tab == "quests" {
                    self.quests_view(ui);
                } else {
                    self.items_view(ui, width);
                }
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
        if close {
            self.shop_open = false;
        }
    }

    fn quests_view(&mut self, ui: &mut egui::Ui) {
        let Some(quests) = self.social.quests.clone() else {
            ui.label(RichText::new("Loading your quests…").color(MUTED));
            return;
        };
        let left = quests["resets_in"].as_i64().unwrap_or(0);
        ui.label(RichText::new("Daily quests").size(20.).bold());
        ui.label(RichText::new(format!("Listen with Roy Music to make progress. New quests in {}h {}m.", left / 3600, left / 60 % 60)).color(MUTED));
        ui.add_space(8.);
        for quest in quests["quests"].as_array().cloned().unwrap_or_default() {
            let tier = quest["tier"].as_str().unwrap_or("easy");
            let accent = match tier {
                "easy" => GREEN,
                "medium" => Color32::from_rgb(0xFF, 0xB8, 0x4D),
                _ => Color32::from_rgb(0xFF, 0x6B, 0x6B),
            };
            let progress = quest["progress"].as_u64().unwrap_or(0);
            let target = quest["target"].as_u64().unwrap_or(1).max(1);
            let (done, claimed) = (quest["done"].as_bool().unwrap_or(false), quest["claimed"].as_bool().unwrap_or(false));
            egui::Frame::new().fill(Color32::from_white_alpha(10)).corner_radius(14).inner_margin(egui::Margin::same(14)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tier.to_uppercase()).small().bold().color(accent));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(format!("{} Orbs", quest["orbs"].as_u64().unwrap_or(0))).bold());
                    });
                });
                ui.label(RichText::new(quest["title"].as_str().unwrap_or("")).size(16.).bold());
                ui.add_space(6.);
                let (bar, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 8.), egui::Sense::hover());
                ui.painter().rect_filled(bar, 4, Color32::from_white_alpha(20));
                ui.painter().rect_filled(egui::Rect::from_min_size(bar.min, egui::vec2(bar.width() * (progress as f32 / target as f32).min(1.), 8.)), 4, accent);
                ui.add_space(6.);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{progress} / {target}")).color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if claimed {
                            ui.add_enabled(false, egui::Button::new("Claimed").corner_radius(16));
                        } else if done {
                            let button = egui::Button::new(RichText::new(format!("Claim {} Orbs", quest["orbs"].as_u64().unwrap_or(0))).bold().color(Color32::BLACK)).fill(Color32::WHITE).corner_radius(16);
                            if ui.add(button).clicked() {
                                self.social.shop_act("/quests/claim", json!({"id": quest["id"]}));
                            }
                        } else {
                            ui.add_enabled(false, egui::Button::new("Keep listening").corner_radius(16));
                        }
                    });
                });
            });
            ui.add_space(8.);
        }
        ui.add_space(6.);
        ui.add(egui::Label::new(RichText::new("Each quest pays once a day, and days start at midnight in Vietnam. Orbs are earned only by listening, and they cannot be turned into money.").small().color(MUTED)).wrap());
    }

    fn items_view(&mut self, ui: &mut egui::Ui, width: f32) {
        let Some(shop) = self.social.shop.clone() else {
            ui.label(RichText::new("Loading the shop…").color(MUTED));
            return;
        };
        let orbs = shop["orbs"].as_u64().unwrap_or(0);
        let owned: Vec<String> = shop["owned"].as_array().map(|a| a.iter().map(|o| format!("{}:{}", o["item"].as_str().unwrap_or(""), o["variant"].as_str().unwrap_or(""))).collect()).unwrap_or_default();
        let equipped = equipped_of(&shop["equipped"]);
        let prices: HashMap<&str, u64> = shop["items"].as_array().map(|a| a.iter().filter_map(|i| Some((i["id"].as_str()?, i["price"].as_u64()?))).collect()).unwrap_or_default();
        let card_width = ((width - 16. * 2. - 24.) / 3.).floor();
        let kind = self.shop_tab.clone();
        let items: Vec<&Item> = catalog().items.iter().filter(|i| i.kind == kind).collect();
        for row in items.chunks(3) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 12.;
                for item in row {
                    let choice = self.shop_choice.get(&item.id).copied().unwrap_or(0).min(item.variants.len() - 1);
                    let variant = &item.variants[choice];
                    let key = format!("{}:{}", item.id, variant.id);
                    let price = prices.get(item.id.as_str()).copied().unwrap_or(item.price);
                    // A card lays its parts out top to bottom, even though the row of cards runs left to right.
                    ui.allocate_ui_with_layout(egui::vec2(card_width, 0.), egui::Layout::top_down(egui::Align::Min), |ui| egui::Frame::new().fill(Color32::from_white_alpha(10)).corner_radius(14).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                        ui.set_width(card_width - 20.);
                        self.preview(ui, item, variant, card_width - 20.);
                        ui.add_space(6.);
                        ui.label(RichText::new(&item.name).bold());
                        ui.label(RichText::new(format!("{} · {}", item.tier, variant.label)).small().color(tier_color(&item.tier)));
                        ui.add_space(4.);
                        ui.horizontal(|ui| {
                            for (i, v) in item.variants.iter().enumerate() {
                                let (rect, response) = ui.allocate_exact_size(egui::vec2(20., 20.), egui::Sense::click());
                                ui.painter().circle_filled(rect.center(), 8.5, hex(&v.color));
                                if i == choice {
                                    ui.painter().circle_stroke(rect.center(), 10., egui::Stroke::new(2., Color32::WHITE));
                                }
                                if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                    self.shop_choice.insert(item.id.clone(), i);
                                }
                            }
                        });
                        ui.add_space(6.);
                        let wide = egui::vec2(ui.available_width(), 32.);
                        if equipped.get(&item.kind) == Some(&key) {
                            if ui.add_sized(wide, egui::Button::new("Wearing").corner_radius(16)).clicked() {
                                self.social.shop_act("/me/equip", json!({"kind": item.kind, "item": null}));
                            }
                        } else if owned.contains(&key) {
                            let button = egui::Button::new(RichText::new("Wear").bold().color(Color32::BLACK)).fill(Color32::WHITE).corner_radius(16);
                            if ui.add_sized(wide, button).clicked() {
                                self.social.shop_act("/me/equip", json!({"kind": item.kind, "item": item.id, "variant": variant.id}));
                            }
                        } else if orbs < price {
                            ui.add_enabled_ui(false, |ui| ui.add_sized(wide, egui::Button::new(format!("{price} Orbs")).corner_radius(16)));
                        } else {
                            let button = egui::Button::new(RichText::new(format!("Buy for {price} Orbs")).bold().color(Color32::BLACK)).fill(Color32::WHITE).corner_radius(16);
                            if ui.add_sized(wide, button).clicked() {
                                let sure = rfd::MessageDialog::new()
                                    .set_title(format!("Buy {}?", item.name))
                                    .set_description(format!("{} for {price} Orbs. You have {orbs}.", variant.label))
                                    .set_buttons(rfd::MessageButtons::YesNo)
                                    .show()
                                    == rfd::MessageDialogResult::Yes;
                                if sure {
                                    self.social.shop_act("/shop/buy", json!({"item": item.id, "variant": variant.id}));
                                }
                            }
                        }
                    }));
                }
            });
            ui.add_space(12.);
        }
    }

    /// The picture on a shop card: the item on a stand-in avatar, name, frame or card.
    fn preview(&self, ui: &mut egui::Ui, item: &Item, variant: &Variant, width: f32) {
        let height = if item.kind == "nameplate" { 74. } else { 138. };
        let (stage, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        ui.painter().rect_filled(stage, 10, Color32::from_rgb(0x17, 0x17, 0x1C));
        let me: HashMap<String, String> = HashMap::from([(item.kind.clone(), format!("{}:{}", item.id, variant.id))]);
        match item.kind.as_str() {
            "decoration" => {
                let avatar = egui::Rect::from_center_size(stage.center(), egui::Vec2::splat(56.));
                ui.painter().circle_filled(avatar.center(), 28., Color32::from_rgb(0x5C, 0x6E, 0xAA));
                ui.painter().text(avatar.center(), egui::Align2::CENTER_CENTER, "R", egui::FontId::proportional(22.), Color32::WHITE);
                paint_decoration(ui, avatar, &me);
            }
            "nameplate" => {
                let plate = stage.shrink2(egui::vec2(8., 8.));
                if paint_plate(ui, plate, &me) {
                    ui.painter().text(plate.left_center() + egui::vec2(14., 0.), egui::Align2::LEFT_CENTER, "Your name", egui::FontId::proportional(15.), Color32::WHITE);
                }
            }
            "frame" => {
                let card = stage.shrink(14.);
                ui.painter().rect_filled(card, 6, Color32::from_rgb(0x22, 0x22, 0x2A));
                paint_frame_item(ui, stage.shrink(6.), item, variant, 28.);
            }
            _ => {
                ui.painter().rect_filled(stage.shrink(6.), 8, Color32::from_rgb(0x22, 0x22, 0x2A));
                paint_effect_item(ui, stage, item, variant);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalog_picture_is_embedded_and_worn_items_resolve() {
        let c = catalog();
        assert!(c.items.len() >= 20);
        for item in &c.items {
            assert!(!item.variants.is_empty(), "{}", item.id);
            if item.kind == "effect" {
                for s in &item.sprites {
                    assert!(shop_assets::bytes(&format!("sprites/{s}.webp")).is_some(), "{s}");
                }
            } else {
                for v in &item.variants {
                    assert!(shop_assets::bytes(&variant_path(item, v)).is_some(), "{} {}", item.id, v.id);
                }
            }
        }
        let on = HashMap::from([("decoration".to_string(), "deco_kitty:gray".to_string())]);
        assert!(worn(&on, "decoration").is_some());
        assert!(worn(&on, "frame").is_none());
        assert!(worn(&HashMap::from([("decoration".to_string(), "deco_kitty:nope".to_string())]), "decoration").is_none());
    }

    #[test]
    fn drawing_what_people_wear_does_not_panic() {
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        let worn = HashMap::from([
            ("decoration".to_string(), "deco_kitty:gray".to_string()),
            ("nameplate".to_string(), "plate_starfield:violet".to_string()),
            ("frame".to_string(), "frame_gold:gold".to_string()),
            ("effect".to_string(), "fx_sakura:pink".to_string()),
        ]);
        for _ in 0..3 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let all = ui.max_rect();
                    assert!(paint_plate(ui, all, &worn));
                    paint_decoration(ui, egui::Rect::from_min_size(egui::pos2(100., 100.), egui::vec2(60., 60.)), &worn);
                    paint_frame(ui, all, &worn, 40.);
                    paint_effect(ui, all, &worn);
                });
            });
        }
    }
}
