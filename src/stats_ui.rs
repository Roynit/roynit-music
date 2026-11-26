//! "Listening stats": what the history says, the same numbers as the phone app's stats sheet.
use super::{App, MUTED, Weight};
use eframe::egui::{self, RichText};
use std::collections::HashMap;

impl App {
    pub(super) fn stats_dialog(&mut self, ctx: &egui::Context) {
        if !self.stats_open {
            return;
        }
        let top = self.dialog_top();
        let history = &self.library.history;
        let tracks = &self.library.tracks;
        let seconds: f64 = history.iter().filter_map(|event| tracks.get(&event.track_id)).map(|track| track.duration).sum();
        let mut artists = HashMap::<String, usize>::new();
        let mut songs = HashMap::<&str, usize>::new();
        for event in history {
            if let Some(track) = tracks.get(&event.track_id) {
                let artist = track.artist.split(',').next().unwrap_or("").trim();
                if !artist.is_empty() {
                    *artists.entry(artist.to_owned()).or_default() += 1;
                }
                *songs.entry(event.track_id.as_str()).or_default() += 1;
            }
        }
        let top_artist = artists.iter().max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)));
        let top_song = songs.iter().max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0))).and_then(|(id, count)| Some((tracks.get(*id)?, *count)));
        let rows = [
            ("Plays".to_owned(), format!("{}, about {}h {}m listened", history.len(), seconds as u64 / 3600, seconds as u64 / 60 % 60)),
            ("Library".to_owned(), format!("{} liked songs, {} playlists, {} disliked", self.library.favorites.len(), self.library.playlists.len(), self.library.dislikes.len())),
            ("Top artist".to_owned(), top_artist.map_or("Play a few songs first".into(), |(name, count)| format!("{name} ({count} plays)"))),
            ("Top song".to_owned(), top_song.map_or("Play a few songs first".into(), |(track, count)| format!("{} ({count} plays)", track.title))),
        ];
        let close = Self::dialog(ctx, top, "stats", true, |ui| {
            ui.set_width(440.);
            ui.label(RichText::new("Listening stats").size(24.).bold());
            ui.label(RichText::new("From what you have played").color(MUTED));
            ui.add_space(14.);
            for (label, value) in &rows {
                ui.label(RichText::new(label).small().bold().color(MUTED));
                ui.label(RichText::new(value).size(17.));
                ui.add_space(8.);
            }
        });
        if close {
            self.stats_open = false;
        }
    }
}
