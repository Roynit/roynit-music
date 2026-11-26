//! Choose which part of a picture to keep for a profile picture or a cover: drag to move, scroll or use the slider to
//! zoom, and what shows inside the window (a circle, or a wide strip) is exactly what is saved.
use super::{MUTED, Weight};
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke, Vec2};
use image::{DynamicImage, RgbImage};
use std::{f32::consts::PI, path::Path};

const REGION: Vec2 = Vec2::new(560., 380.);

pub enum Outcome {
    Open,
    Cancel,
    Done(RgbImage),
}

pub struct Cropper {
    picture: DynamicImage,
    texture: egui::TextureHandle,
    circle: bool,
    /// The size of the saved picture.
    out: (u32, u32),
    title: &'static str,
    /// 1 means the picture just covers the window.
    zoom: f32,
    /// Where the picture's centre is, relative to the window's centre, in screen pixels.
    offset: Vec2,
}

impl Cropper {
    pub fn new(
        ctx: &egui::Context,
        path: &Path,
        circle: bool,
        out: (u32, u32),
        title: &'static str,
    ) -> Result<Self, String> {
        if std::fs::metadata(path).map_err(|_| "Could not read that file.")?.len() > 25_000_000 {
            return Err("Choose a picture under 25 MB.".into());
        }
        let full = image::open(path).map_err(|_| "That file is not a supported picture.".to_owned())?;
        if full.width() == 0 || full.height() == 0 {
            return Err("That picture is empty.".into());
        }
        // Everything works on a screen-sized copy: it is plenty for pictures that end up at most 960 px wide.
        let picture = if full.width().max(full.height()) > 1600 {
            full.resize(1600, 1600, image::imageops::FilterType::Triangle)
        } else {
            full
        };
        let rgba = picture.to_rgba8();
        let texture = ctx.load_texture(
            "crop-picture",
            egui::ColorImage::from_rgba_unmultiplied(
                [rgba.width() as usize, rgba.height() as usize],
                rgba.as_raw(),
            ),
            egui::TextureOptions::LINEAR,
        );
        Ok(Self { picture, texture, circle, out, title, zoom: 1., offset: Vec2::ZERO })
    }

    fn window(&self) -> Rect {
        let size = if self.circle {
            Vec2::splat(300.)
        } else {
            let width = 480.;
            Vec2::new(width, width * self.out.1 as f32 / self.out.0 as f32)
        };
        Rect::from_center_size(Pos2::ZERO + REGION / 2., size)
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Outcome {
        let mut outcome = Outcome::Open;
        let frame = egui::Frame::popup(&ctx.style()).inner_margin(22).corner_radius(14);
        let modal = egui::Modal::new(egui::Id::new("crop-picture-dialog"))
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_width(REGION.x);
                ui.label(RichText::new(self.title).size(22.).bold());
                ui.label(RichText::new("Drag to move. Scroll or use the slider to zoom.").color(MUTED));
                ui.add_space(12.);
                let (region, response) = ui.allocate_exact_size(REGION, egui::Sense::drag());
                let window = self.window().translate(region.min.to_vec2());
                let size = self.texture.size_vec2();
                let base = (window.width() / size.x).max(window.height() / size.y);
                if response.hovered() {
                    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                    if scroll != 0. {
                        self.zoom = (self.zoom * (1. + scroll * 0.002)).clamp(1., 5.);
                    }
                }
                if response.dragged() {
                    self.offset += response.drag_delta();
                }
                let scale = base * self.zoom;
                let shown = size * scale;
                // The picture always covers the window: no empty edges in what is saved.
                let slack = ((shown - window.size()) / 2.).max(Vec2::ZERO);
                self.offset = self.offset.clamp(-slack, slack);
                let picture = Rect::from_center_size(window.center() + self.offset, shown);
                if response.hovered() || response.dragged() {
                    ui.ctx().set_cursor_icon(if response.dragged() {
                        egui::CursorIcon::Grabbing
                    } else {
                        egui::CursorIcon::Grab
                    });
                }
                let painter = ui.painter_at(region);
                painter.rect_filled(region, 8, Color32::from_gray(12));
                painter.image(
                    self.texture.id(),
                    picture,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
                    Color32::WHITE,
                );
                self.dim(&painter, region, window);
                let ring = Stroke::new(2., Color32::WHITE);
                if self.circle {
                    painter.circle_stroke(window.center(), window.width() / 2., ring);
                } else {
                    painter.rect_stroke(window, 4, ring, egui::StrokeKind::Outside);
                    // Thirds, to help line a cover up.
                    let faint = Stroke::new(1., Color32::from_white_alpha(70));
                    for i in 1..3 {
                        let t = i as f32 / 3.;
                        painter.vline(window.left() + window.width() * t, window.y_range(), faint);
                        painter.hline(window.x_range(), window.top() + window.height() * t, faint);
                    }
                }
                ui.add_space(10.);
                ui.horizontal(|ui| {
                    // A visible rail: the theme's own is the same colour as the dialog.
                    ui.visuals_mut().widgets.inactive.bg_fill = Color32::from_gray(95);
                    ui.spacing_mut().slider_width = 300.;
                    ui.label(RichText::new("-").color(MUTED));
                    ui.add(
                        egui::Slider::new(&mut self.zoom, 1.0..=5.0)
                            .show_value(false)
                            .clamping(egui::SliderClamping::Always),
                    );
                    ui.label(RichText::new("+").color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Apply").bold().color(Color32::BLACK))
                                    .fill(Color32::WHITE)
                                    .corner_radius(18)
                                    .min_size(egui::vec2(96., 36.)),
                            )
                            .clicked()
                        {
                            outcome = Outcome::Done(self.result(window, picture, scale));
                        }
                        if ui.add(egui::Button::new("Cancel").frame(false)).clicked() {
                            outcome = Outcome::Cancel;
                        }
                    });
                });
            });
        if modal.should_close() && matches!(outcome, Outcome::Open) {
            outcome = Outcome::Cancel;
        }
        outcome
    }

    /// Everything outside the window is dimmed; a circle needs its four corners filled in by hand.
    fn dim(&self, painter: &egui::Painter, region: Rect, window: Rect) {
        let shade = Color32::from_black_alpha(175);
        for band in [
            Rect::from_min_max(region.min, Pos2::new(region.right(), window.top())),
            Rect::from_min_max(Pos2::new(region.left(), window.bottom()), region.max),
            Rect::from_min_max(Pos2::new(region.left(), window.top()), Pos2::new(window.left(), window.bottom())),
            Rect::from_min_max(Pos2::new(window.right(), window.top()), Pos2::new(region.right(), window.bottom())),
        ] {
            painter.rect_filled(band, 0, shade);
        }
        if !self.circle {
            return;
        }
        let radius = window.width() / 2.;
        for (corner, from) in [
            (window.left_top(), PI),
            (window.right_top(), 1.5 * PI),
            (window.right_bottom(), 0.),
            (window.left_bottom(), 0.5 * PI),
        ] {
            // A fan from the corner to the arc: the gap between the square and the circle.
            let mut mesh = egui::epaint::Mesh::default();
            mesh.colored_vertex(corner, shade);
            for step in 0..=24 {
                let angle = from + step as f32 / 24. * 0.5 * PI;
                mesh.colored_vertex(
                    window.center() + Vec2::new(angle.cos(), angle.sin()) * radius,
                    shade,
                );
            }
            for i in 1..=24u32 {
                mesh.add_triangle(0, i, i + 1);
            }
            painter.add(egui::Shape::mesh(mesh));
        }
    }

    /// What is inside the window, at the size to be saved.
    fn result(&self, window: Rect, picture: Rect, scale: f32) -> RgbImage {
        let (w, h) = (self.picture.width() as f32, self.picture.height() as f32);
        let x = ((window.left() - picture.left()) / scale).clamp(0., w - 1.);
        let y = ((window.top() - picture.top()) / scale).clamp(0., h - 1.);
        let cw = (window.width() / scale).clamp(1., w - x);
        let ch = (window.height() / scale).clamp(1., h - y);
        self.picture
            .crop_imm(x as u32, y as u32, cw as u32, ch as u32)
            .resize_exact(self.out.0, self.out.1, image::imageops::FilterType::Lanczos3)
            .to_rgb8()
    }
}
