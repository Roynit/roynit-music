//! Photos and voice messages in a conversation: photo blocks, the full-screen viewer, voice bubbles with a waveform, and
//! the recording bar. Icons are drawn by hand so they look the same on every PC.
use super::friends::Media;
use super::voice;
use super::*;
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct Shot {
    pub url: String,
    pub local: Option<Arc<[u8]>>,
}

/// Photos shown full screen: a grid when there are several, and one photo at a time to swipe through.
pub(super) struct PhotoView {
    pub shots: Vec<Shot>,
    /// The photo shown big, when there is one.
    pub viewing: Option<usize>,
}

impl PhotoView {
    pub fn of(medias: &[&Media], at: Option<usize>) -> Self {
        Self { shots: medias.iter().map(|m| Shot { url: m.url.clone(), local: m.local.clone() }).collect(), viewing: at }
    }
}

fn source(url: &str, local: &Option<Arc<[u8]>>) -> egui::ImageSource<'static> {
    match local {
        Some(bytes) => egui::ImageSource::Bytes {
            uri: std::borrow::Cow::Owned(format!("bytes://sent-{:x}.jpg", Arc::as_ptr(bytes) as *const u8 as usize)),
            bytes: bytes.clone().into(),
        },
        None => egui::ImageSource::Uri(std::borrow::Cow::Owned(url.to_owned())),
    }
}

/// Fills [rect] with the middle of a picture (like a square thumbnail), with rounded corners.
fn cover(ui: &mut egui::Ui, src: egui::ImageSource<'static>, rect: egui::Rect, radius: u8, dim: bool) {
    let image = egui::Image::new(src);
    let natural = image.load_for_size(ui.ctx(), rect.size()).ok().and_then(|poll| poll.size());
    let Some(size) = natural.filter(|s| s.x > 0. && s.y > 0.) else {
        ui.painter().rect_filled(rect, radius, Color32::from_white_alpha(14));
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(80));
        return;
    };
    let (want, have) = (rect.width() / rect.height(), size.x / size.y);
    let uv = if have > want {
        let w = want / have;
        egui::Rect::from_min_max(egui::pos2((1. - w) / 2., 0.), egui::pos2((1. + w) / 2., 1.))
    } else {
        let h = have / want;
        egui::Rect::from_min_max(egui::pos2(0., (1. - h) / 2.), egui::pos2(1., (1. + h) / 2.))
    };
    let image = image.uv(uv).maintain_aspect_ratio(false).fit_to_exact_size(rect.size()).corner_radius(radius).tint(if dim { Color32::from_white_alpha(150) } else { Color32::WHITE });
    ui.put(rect, image);
}

/// One photo, at its own proportions.
pub(super) fn single(ui: &mut egui::Ui, media: &Media) {
    let tint = if media.pending { Color32::from_white_alpha(150) } else { Color32::WHITE };
    ui.add(egui::Image::new(source(&media.url, &media.local)).max_width(280.).max_height(360.).corner_radius(14).tint(tint));
}

/// Photos sent together as one block of up to four pictures, "+N" over the last when there are more.
pub(super) fn collage(ui: &mut egui::Ui, medias: &[&Media]) {
    const W: f32 = 280.;
    const GAP: f32 = 3.;
    let half = (W - GAP) / 2.;
    let n = medias.len();
    let height = match n {
        2 => half * 1.25,
        3 => W * 0.6 + GAP + half,
        _ => half * 2. + GAP,
    };
    let (area, _) = ui.allocate_exact_size(egui::vec2(W, height), egui::Sense::hover());
    let at = |x: f32, y: f32, w: f32, h: f32| egui::Rect::from_min_size(area.min + egui::vec2(x, y), egui::vec2(w, h));
    let rects: Vec<egui::Rect> = match n {
        2 => vec![at(0., 0., half, height), at(half + GAP, 0., half, height)],
        3 => vec![at(0., 0., W, W * 0.6), at(0., W * 0.6 + GAP, half, half), at(half + GAP, W * 0.6 + GAP, half, half)],
        _ => vec![at(0., 0., half, half), at(half + GAP, 0., half, half), at(0., half + GAP, half, half), at(half + GAP, half + GAP, half, half)],
    };
    for (rect, media) in rects.iter().zip(medias) {
        cover(ui, source(&media.url, &media.local), *rect, 8, media.pending);
    }
    if n > 4 {
        let rect = rects[3];
        ui.painter().rect_filled(rect, 8, Color32::from_black_alpha(150));
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, format!("+{}", n - 4), egui::FontId::proportional(26.), Color32::WHITE);
    }
}

/// A voice message: play button, waveform that fills as it plays, and the length. The whole bubble plays it.
pub(super) fn voice_bubble(ui: &mut egui::Ui, media: &Media, id: i64) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(240., 38.), egui::Sense::hover());
    if media.pending {
        ui.painter().text(rect.left_center(), egui::Align2::LEFT_CENTER, "Sending voice message…", egui::FontId::proportional(14.), MUTED);
        return;
    }
    let playing = voice::state(&media.url);
    let painter = ui.painter();
    let circle = rect.left_center() + egui::vec2(19., 0.);
    painter.circle_filled(circle, 19., Color32::WHITE);
    let ink = Color32::from_gray(28);
    if playing.is_some() {
        pause_glyph(painter, circle, 7., ink);
    } else {
        play_glyph(painter, circle, 7., ink);
    }
    let bars = voice::shape(id, 30);
    let x0 = rect.left() + 48.;
    let step = (rect.width() - 48. - 44.) / bars.len() as f32;
    let progress = playing.unwrap_or(0.);
    for (i, level) in bars.iter().enumerate() {
        let x = x0 + i as f32 * step + step / 2.;
        let h = 4. + level * 24.;
        let color = if (i as f32 / bars.len() as f32) < progress { Color32::WHITE } else { Color32::from_white_alpha(95) };
        painter.line_segment([egui::pos2(x, rect.center().y - h / 2.), egui::pos2(x, rect.center().y + h / 2.)], egui::Stroke::new((step * 0.55).max(2.), color));
    }
    painter.text(rect.right_center(), egui::Align2::RIGHT_CENTER, format!("{}:{:02}", media.seconds / 60, media.seconds % 60), egui::FontId::proportional(13.), Color32::WHITE);
    if playing.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
    }
}

fn play_glyph(painter: &egui::Painter, c: egui::Pos2, r: f32, color: Color32) {
    painter.add(egui::Shape::convex_polygon(vec![c + egui::vec2(-r * 0.6, -r), c + egui::vec2(r, 0.), c + egui::vec2(-r * 0.6, r)], color, egui::Stroke::NONE));
}

fn pause_glyph(painter: &egui::Painter, c: egui::Pos2, r: f32, color: Color32) {
    for dx in [-r * 0.55, r * 0.55] {
        painter.rect_filled(egui::Rect::from_center_size(c + egui::vec2(dx, 0.), egui::vec2(r * 0.6, r * 1.8)), 1, color);
    }
}

/// A round icon button drawn by [paint].
pub(super) fn icon_button(ui: &mut egui::Ui, size: f32, tip: &str, paint: impl FnOnce(&egui::Painter, egui::Pos2, f32, Color32)) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    if response.hovered() {
        ui.painter().circle_filled(rect.center(), size / 2., Color32::from_white_alpha(18));
    }
    let color = if response.hovered() { Color32::WHITE } else { MUTED };
    paint(ui.painter(), rect.center(), size * 0.3, color);
    response.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(super) fn photo_glyph(painter: &egui::Painter, c: egui::Pos2, r: f32, color: Color32) {
    let stroke = egui::Stroke::new(1.6_f32, color);
    let frame = egui::Rect::from_center_size(c, egui::vec2(r * 2.6, r * 2.1));
    painter.rect_stroke(frame, 3, stroke, egui::StrokeKind::Inside);
    painter.circle_filled(frame.min + egui::vec2(r * 0.85, r * 0.75), r * 0.28, color);
    painter.add(egui::Shape::line(vec![frame.left_bottom() + egui::vec2(1., -2.), c + egui::vec2(-r * 0.3, -r * 0.1), c + egui::vec2(r * 0.25, r * 0.5), c + egui::vec2(r * 0.55, r * 0.15), frame.right_bottom() + egui::vec2(-1., -2.)], stroke));
}

pub(super) fn mic_glyph(painter: &egui::Painter, c: egui::Pos2, r: f32, color: Color32) {
    let stroke = egui::Stroke::new(1.6_f32, color);
    painter.rect_filled(egui::Rect::from_center_size(c + egui::vec2(0., -r * 0.35), egui::vec2(r * 0.95, r * 1.7)), r * 0.48, color);
    let points: Vec<egui::Pos2> = (0..=12).map(|i| {
        let a = std::f32::consts::PI * i as f32 / 12.;
        c + egui::vec2(-a.cos() * r * 0.85, r * 0.15 + a.sin() * r * 0.8)
    }).collect();
    painter.add(egui::Shape::line(points, stroke));
    painter.line_segment([c + egui::vec2(0., r * 0.95), c + egui::vec2(0., r * 1.3)], stroke);
}

fn trash_glyph(painter: &egui::Painter, c: egui::Pos2, r: f32, color: Color32) {
    let stroke = egui::Stroke::new(1.6_f32, color);
    painter.line_segment([c + egui::vec2(-r, -r * 0.7), c + egui::vec2(r, -r * 0.7)], stroke);
    painter.line_segment([c + egui::vec2(-r * 0.35, -r * 0.7), c + egui::vec2(-r * 0.35, -r)], stroke);
    painter.line_segment([c + egui::vec2(r * 0.35, -r * 0.7), c + egui::vec2(r * 0.35, -r)], stroke);
    painter.add(egui::Shape::closed_line(vec![c + egui::vec2(-r * 0.75, -r * 0.5), c + egui::vec2(r * 0.75, -r * 0.5), c + egui::vec2(r * 0.6, r), c + egui::vec2(-r * 0.6, r)], stroke));
}

fn send_glyph(painter: &egui::Painter, c: egui::Pos2, r: f32, color: Color32) {
    painter.add(egui::Shape::convex_polygon(vec![c + egui::vec2(-r, -r), c + egui::vec2(r * 1.1, 0.), c + egui::vec2(-r, r), c + egui::vec2(-r * 0.5, 0.)], color, egui::Stroke::NONE));
}

pub(super) enum Recorded {
    Going,
    Cancel,
    Send,
}

impl App {
    /// The bar that replaces the message box while recording: bin, pause, live waveform, time, send.
    pub(super) fn recording_bar(&mut self, ui: &mut egui::Ui, accent: Color32) -> Recorded {
        let Some((_, recorder)) = &self.recording else {
            return Recorded::Cancel;
        };
        let mut outcome = Recorded::Going;
        if icon_button(ui, 38., "Throw this recording away", trash_glyph).clicked() {
            outcome = Recorded::Cancel;
        }
        let seconds = recorder.seconds();
        let paused = recorder.paused();
        let levels = recorder.levels();
        egui::Frame::new().fill(accent.gamma_multiply(0.55)).corner_radius(22).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
            ui.set_width(ui.available_width() - 50.);
            ui.horizontal(|ui| {
                let (rect, response) = ui.allocate_exact_size(egui::vec2(32., 32.), egui::Sense::click());
                ui.painter().circle_filled(rect.center(), 16., Color32::WHITE);
                if paused {
                    play_glyph(ui.painter(), rect.center(), 6., Color32::from_gray(28));
                } else {
                    pause_glyph(ui.painter(), rect.center(), 6., Color32::from_gray(28));
                }
                if response.on_hover_text(if paused { "Resume" } else { "Pause" }).clicked()
                    && let Some((_, recorder)) = &self.recording
                {
                    recorder.toggle_pause();
                }
                let width = (ui.available_width() - 52.).max(40.);
                let (wave, _) = ui.allocate_exact_size(egui::vec2(width, 32.), egui::Sense::hover());
                let step = wave.width() / levels.len().max(1) as f32;
                for (i, level) in levels.iter().enumerate() {
                    let x = wave.left() + i as f32 * step + step / 2.;
                    let h = if *level <= 0. { 2. } else { 3. + level * 26. };
                    ui.painter().line_segment([egui::pos2(x, wave.center().y - h / 2.), egui::pos2(x, wave.center().y + h / 2.)], egui::Stroke::new((step * 0.5).max(1.5), Color32::from_white_alpha(if *level <= 0. { 60 } else { 230 })));
                }
                ui.label(RichText::new(format!("{}:{:02}", seconds as i64 / 60, seconds as i64 % 60)).bold().color(Color32::WHITE));
            });
        });
        let (rect, response) = ui.allocate_exact_size(egui::vec2(40., 40.), egui::Sense::click());
        ui.painter().circle_filled(rect.center(), 20., accent);
        send_glyph(ui.painter(), rect.center(), 7., ui.visuals().selection.stroke.color);
        if response.on_hover_text("Send").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() || seconds as i64 >= voice::MAX_SECONDS {
            outcome = Recorded::Send;
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        outcome
    }

    /// The full-screen photo viewer: a grid for several photos, then one photo at a time (arrow keys, the buttons or a drag).
    pub(super) fn photo_viewer(&mut self, ctx: &egui::Context) {
        let Some(view) = &self.photo_view else {
            return;
        };
        let screen = ctx.content_rect();
        let shots = view.shots.clone();
        let mut viewing = view.viewing;
        let mut close = false;
        let count = shots.len();
        let (left, right, escape) = ctx.input(|i| (i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight), i.key_pressed(egui::Key::Escape)));
        egui::Area::new(egui::Id::new("photo-view")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            let backdrop = ui.allocate_rect(screen, egui::Sense::click_and_drag());
            ui.painter().rect_filled(screen, 0, Color32::from_black_alpha(238));
            let drag_id = egui::Id::new("photo-view-drag");
            if backdrop.dragged() {
                let sum = ui.memory(|m| m.data.get_temp::<f32>(drag_id)).unwrap_or(0.) + backdrop.drag_delta().x;
                ui.memory_mut(|m| m.data.insert_temp(drag_id, sum));
            }
            let swiped = if backdrop.drag_stopped() { ui.memory_mut(|m| m.data.remove_temp::<f32>(drag_id)).unwrap_or(0.) } else { 0. };
            let shut = egui::Rect::from_min_size(screen.right_top() + egui::vec2(-60., 14.), egui::vec2(44., 44.));
            match viewing {
                Some(at) => {
                    let (mut next, size) = (at, screen.size() * 0.86);
                    if (left || swiped > 70.) && at > 0 {
                        next = at - 1;
                    }
                    if (right || swiped < -70.) && at + 1 < count {
                        next = at + 1;
                    }
                    let shot = &shots[next.min(count - 1)];
                    let place = egui::Rect::from_center_size(screen.center(), size);
                    ui.put(place, egui::Image::new(source(&shot.url, &shot.local)).fit_to_exact_size(size).corner_radius(8));
                    if count > 1 {
                        ui.painter().text(screen.center_top() + egui::vec2(0., 34.), egui::Align2::CENTER_CENTER, format!("{} / {}", next + 1, count), egui::FontId::proportional(15.), Color32::WHITE);
                        for (dir, label, enabled) in [(-1i32, "‹", next > 0), (1, "›", next + 1 < count)] {
                            let x = if dir < 0 { screen.left() + 16. } else { screen.right() - 60. };
                            let button = egui::Rect::from_min_size(egui::pos2(x, screen.center().y - 24.), egui::vec2(44., 48.));
                            if enabled && ui.put(button, egui::Button::new(RichText::new(label).size(30.).color(Color32::WHITE)).fill(Color32::from_white_alpha(26)).corner_radius(24)).clicked() {
                                next = (next as i32 + dir) as usize;
                            }
                        }
                    }
                    viewing = Some(next);
                }
                None => {
                    ui.painter().text(screen.left_top() + egui::vec2(72., 36.), egui::Align2::LEFT_CENTER, format!("{count} photos"), egui::FontId::proportional(19.), Color32::WHITE);
                    let columns = ((screen.width() - 48.) / 220.).floor().clamp(2., 5.) as usize;
                    let tile = (screen.width() - 48. - (columns as f32 - 1.) * 8.) / columns as f32;
                    let top = screen.top() + 76.;
                    for (i, shot) in shots.iter().enumerate() {
                        let (row, col) = (i / columns, i % columns);
                        let rect = egui::Rect::from_min_size(egui::pos2(screen.left() + 24. + col as f32 * (tile + 8.), top + row as f32 * (tile + 8.)), egui::vec2(tile, tile));
                        if rect.top() > screen.bottom() {
                            break;
                        }
                        cover(ui, source(&shot.url, &shot.local), rect, 10, false);
                        let hit = ui.interact(rect, egui::Id::new(("photo-tile", i)), egui::Sense::click());
                        if hit.hovered() {
                            ui.painter().rect_stroke(rect, 10, egui::Stroke::new(2.0_f32, Color32::WHITE), egui::StrokeKind::Inside);
                        }
                        if hit.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            viewing = Some(i);
                        }
                    }
                }
            }
            let button = ui.interact(shut, egui::Id::new("photo-view-close"), egui::Sense::click());
            ui.painter().circle_filled(shut.center(), 22., Color32::from_white_alpha(if button.hovered() { 50 } else { 26 }));
            for (a, b) in [(egui::vec2(-7., -7.), egui::vec2(7., 7.)), (egui::vec2(-7., 7.), egui::vec2(7., -7.))] {
                ui.painter().line_segment([shut.center() + a, shut.center() + b], egui::Stroke::new(2.0_f32, Color32::WHITE));
            }
            if button.on_hover_text("Close").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                close = true;
            }
        });
        if escape {
            // Back out one step: from one photo to the grid, from the grid (or a lone photo) to the chat.
            if viewing.is_some() && count > 1 {
                viewing = None;
            } else {
                close = true;
            }
        }
        if close {
            self.photo_view = None;
        } else if let Some(view) = &mut self.photo_view {
            view.viewing = viewing;
        }
    }
}
