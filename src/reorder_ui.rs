//! Drag-to-reorder lists. A grabbed item follows the pointer and lifts off the page; the others slide
//! out of its way, and when it is dropped the new order is reported. Removing an item collapses its slot
//! instead of making the list jump.
use eframe::egui::{self, Color32, Rect, Sense, Vec2};
use riff_core::model::Track;
use std::time::{Duration, Instant};

/// What an item looked like when it was removed, so its slot can fade and collapse after it is gone.
pub(super) enum Snapshot {
    Playlist { name: String, count: usize, cover: Option<String> },
    Track(Track),
}

pub(super) struct Ghost {
    pub list: String,
    pub key: String,
    pub index: usize,
    pub at: Instant,
    pub snapshot: Snapshot,
}

/// Index of the slot the grabbed item would land in: how many of the other items sit above its centre.
pub(super) fn target_slot(slots: &[f32], dragged: usize, dragged_top: f32, dragged_height: f32) -> usize {
    let centre = dragged_top + dragged_height / 2.;
    let mut top = 0.;
    let mut above = 0;
    for (i, slot) in slots.iter().enumerate() {
        if i != dragged {
            if top + slot / 2. < centre {
                above += 1;
            }
            top += slot;
        }
    }
    above
}

/// Item indices in the order they are shown while `from` is held over slot `to`.
pub(super) fn moved(len: usize, from: usize, to: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..len).collect();
    let item = order.remove(from);
    order.insert(to.min(len - 1), item);
    order
}

/// Top edge of every item (indexed by item) when they are laid out in `order`.
pub(super) fn slot_tops(slots: &[f32], order: &[usize]) -> Vec<f32> {
    let mut tops = vec![0.; slots.len()];
    let mut y = 0.;
    for &i in order {
        tops[i] = y;
        y += slots[i];
    }
    tops
}

#[derive(Clone)]
struct Held {
    key: String,
    /// Where inside the item the pointer grabbed it, so it does not jump to the pointer.
    grab: f32,
}

/// One frame of a reorderable list. Call [`List::begin`], then for each index in [`List::order`] draw
/// the item into [`List::rect`] and pass its response to [`List::feed`]; [`List::finish`] reports a drop.
pub(super) struct List {
    id: egui::Id,
    keys: Vec<String>,
    origin: egui::Pos2,
    width: f32,
    draw_heights: Vec<f32>,
    tops: Vec<f32>,
    held: Option<usize>,
    held_top: f32,
    target: usize,
    dropped: Option<Vec<usize>>,
    can_drag: Vec<bool>,
}

impl List {
    /// `slots` is the room each item takes (its height plus the gap, shrinking to 0 while it is being
    /// removed); `draw_heights` is how tall each item itself is drawn.
    pub(super) fn begin(
        ui: &mut egui::Ui,
        id: &str,
        keys: Vec<String>,
        slots: Vec<f32>,
        draw_heights: Vec<f32>,
        can_drag: Vec<bool>,
        animate: f32,
    ) -> Self {
        let id = ui.id().with(id);
        let total: f32 = slots.iter().sum();
        let (area, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), total), Sense::hover());
        let held_state: Option<Held> = ui.data(|d| d.get_temp(id));
        let held = held_state
            .as_ref()
            .and_then(|h| keys.iter().position(|k| *k == h.key))
            // The release frame still counts as held, so the drop is seen instead of snapping back.
            .filter(|_| ui.input(|i| i.pointer.primary_down() || i.pointer.any_released()));
        let pointer_y = ui.input(|i| i.pointer.hover_pos()).map(|p| p.y);
        let (mut held_top, mut target) = (0., 0);
        let mut order: Vec<usize> = (0..keys.len()).collect();
        if let (Some(at), Some(py), Some(state)) = (held, pointer_y, held_state.as_ref()) {
            held_top = (py - area.top() - state.grab).clamp(0., (total - draw_heights[at]).max(0.));
            target = target_slot(&slots, at, held_top, draw_heights[at]);
            order = moved(keys.len(), at, target);
            // Near the top or bottom edge of the scroll area the list scrolls on its own.
            let clip = ui.clip_rect();
            if py < clip.top() + 28. {
                ui.scroll_with_delta(egui::vec2(0., 14.));
            } else if py > clip.bottom() - 28. {
                ui.scroll_with_delta(egui::vec2(0., -14.));
            }
            ui.ctx().request_repaint();
        } else if held_state.is_some() && held.is_none() {
            ui.data_mut(|d| d.remove::<Held>(id));
        }
        let goals = slot_tops(&slots, &order);
        let tops = keys
            .iter()
            .enumerate()
            .map(|(i, key)| ui.ctx().animate_value_with_time(id.with(("top", key)), goals[i], animate))
            .collect();
        Self {
            id,
            keys,
            origin: area.min,
            width: area.width(),
            draw_heights,
            tops,
            held,
            held_top,
            target,
            dropped: None,
            can_drag,
        }
    }

    /// Items in paint order: the grabbed one last, so it floats above the rest.
    pub(super) fn order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.keys.len()).filter(|i| Some(*i) != self.held).collect();
        order.extend(self.held);
        order
    }

    pub(super) fn rect(&self, i: usize) -> Rect {
        let top = if Some(i) == self.held { self.held_top } else { self.tops[i] };
        Rect::from_min_size(
            egui::pos2(self.origin.x, self.origin.y + top),
            egui::vec2(self.width, self.draw_heights[i]),
        )
    }

    pub(super) fn is_held(&self, i: usize) -> bool {
        Some(i) == self.held
    }

    /// 0..1, easing up while an item is picked up and back down when it is let go.
    pub(super) fn lift(&self, ui: &egui::Ui, i: usize) -> f32 {
        ui.ctx()
            .animate_bool_with_time(self.id.with(("lift", &self.keys[i])), Some(i) == self.held, 0.12)
    }

    /// The grip on an item's right edge: only here does a press start a drag, so the rest of the row stays
    /// a plain click target. It shows while the row is hovered or held.
    pub(super) fn grip(&mut self, ui: &egui::Ui, i: usize, row: Rect) {
        if !self.can_drag[i] {
            return;
        }
        let zone = Rect::from_min_max(egui::pos2(row.right() - GRIP, row.top()), row.right_bottom());
        let response = ui.interact(zone, self.id.with(("grip", &self.keys[i])), Sense::click_and_drag());
        let active = response.hovered() || self.held == Some(i);
        if active || ui.rect_contains_pointer(row) {
            let color = if active { Color32::WHITE } else { Color32::from_gray(0x80) };
            for dx in [-3.5, 3.5] {
                for dy in [-6., 0., 6.] {
                    ui.painter().circle_filled(zone.center() + Vec2::new(dx, dy), 1.6, color);
                }
            }
        }
        self.feed(ui, i, &response);
    }

    fn feed(&mut self, ui: &egui::Ui, i: usize, response: &egui::Response) {
        if response.drag_started()
            && let Some(pointer) = ui.input(|i| i.pointer.press_origin().or(i.pointer.hover_pos()))
        {
            let grab = pointer.y - self.rect(i).top();
            ui.data_mut(|d| d.insert_temp(self.id, Held { key: self.keys[i].clone(), grab }));
            ui.ctx().request_repaint();
        }
        if response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if response.hovered() && self.held.is_none() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        if response.drag_stopped() {
            if self.held == Some(i) && self.target != i {
                self.dropped = Some(moved(self.keys.len(), i, self.target));
            }
            ui.data_mut(|d| d.remove::<Held>(self.id));
        }
    }

    /// The new order (item indices) when something was dropped in a new place this frame.
    pub(super) fn finish(self) -> Option<Vec<usize>> {
        self.dropped
    }
}

/// Width of the grip zone at an item's right edge.
pub(super) const GRIP: f32 = 26.;

/// A soft shadow that grows as an item lifts off the page.
pub(super) fn paint_lift(ui: &egui::Ui, rect: Rect, radius: u8, lift: f32) {
    if lift <= 0.01 {
        return;
    }
    ui.painter().add(
        egui::Shadow {
            offset: [0, (6. * lift) as i8],
            blur: (18. * lift) as u8,
            spread: 0,
            color: Color32::from_black_alpha((150. * lift) as u8),
        }
        .as_shape(rect, radius),
    );
}

/// An item being removed: its slot and opacity shrink to nothing over a fraction of a second.
pub(super) const COLLAPSE: Duration = Duration::from_millis(240);

pub(super) fn collapse(at: Instant) -> f32 {
    let t = (at.elapsed().as_secs_f32() / COLLAPSE.as_secs_f32()).min(1.);
    1. - (1. - (1. - t).powi(3))
}

#[allow(dead_code)]
pub(super) fn vec(x: f32, y: f32) -> Vec2 {
    egui::vec2(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slot_follows_the_dragged_centre() {
        let slots = [54., 54., 54., 54.];
        // Held item 0 dragged until its centre is past the middle of item 2 (above = items 1 and 2).
        assert_eq!(target_slot(&slots, 0, 2. * 54. - 20., 54.), 2);
        assert_eq!(target_slot(&slots, 0, 0., 54.), 0, "still on top: stays put");
        assert_eq!(target_slot(&slots, 3, 0., 54.), 0, "dragged to the very top");
        assert_eq!(target_slot(&slots, 1, 400., 54.), 3, "dragged below everything");
    }

    #[test]
    fn items_slide_aside_for_the_dragged_one() {
        assert_eq!(moved(4, 0, 2), vec![1, 2, 0, 3]);
        assert_eq!(moved(4, 3, 1), vec![0, 3, 1, 2]);
        assert_eq!(moved(3, 1, 1), vec![0, 1, 2]);
        let tops = slot_tops(&[10., 20., 30.], &moved(3, 0, 2));
        assert_eq!(tops, vec![50., 0., 20.], "item 1 now first, item 2 next, item 0 last");
    }

    #[test]
    fn a_collapsing_slot_reaches_zero() {
        let long_ago = Instant::now() - Duration::from_secs(2);
        assert_eq!(collapse(long_ago), 0.);
        assert!(collapse(Instant::now()) > 0.95);
    }
}
