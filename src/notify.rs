//! Desktop notifications: a Windows toast under the Roy Music name, one short "ting", good-morning and birthday wishes.
use super::{App, MUTED};
use chrono::{Datelike, Local, Timelike};
use eframe::egui::{self, RichText};
use serde_json::json;

const TING: &[u8] = include_bytes!("../assets/roy_ting.wav");

/// Plays the bright, short "ting" (16-bit mono WAV, decoded by hand: the player has no WAV decoder, only FFmpeg).
pub fn ting() {
    std::thread::spawn(|| {
        let Some(data) = TING.get(44..) else { return };
        let samples: Vec<f32> = data.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.).collect();
        let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else { return };
        let sink = rodio::Sink::connect_new(stream.mixer());
        sink.append(rodio::buffer::SamplesBuffer::new(1, 44_100, samples));
        sink.sleep_until_end();
    });
}

/// A Windows toast, shown as "Roy Music". The text goes in through the environment, never into the script itself.
pub fn toast(title: &str, body: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const SCRIPT: &str = r#"
$id='Roynit.RoyMusic'
$k="HKCU:\Software\Classes\AppUserModelId\$id"
if(!(Test-Path $k)){New-Item $k -Force|Out-Null;New-ItemProperty $k -Name DisplayName -Value 'Roy Music' -Force|Out-Null}
[void][Windows.UI.Notifications.ToastNotificationManager,Windows.UI.Notifications,ContentType=WindowsRuntime]
[void][Windows.Data.Xml.Dom.XmlDocument,Windows.Data.Xml.Dom.XmlDocument,ContentType=WindowsRuntime]
$t=[Security.SecurityElement]::Escape($env:ROY_TITLE);$b=[Security.SecurityElement]::Escape($env:ROY_BODY)
$x=New-Object Windows.Data.Xml.Dom.XmlDocument
$x.LoadXml("<toast><visual><binding template='ToastGeneric'><text>$t</text><text>$b</text></binding></visual><audio silent='true'/></toast>")
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier($id).Show([Windows.UI.Notifications.ToastNotification]::new($x))
"#;
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", SCRIPT])
            .env("ROY_TITLE", title)
            .env("ROY_BODY", body)
            .creation_flags(0x0800_0000)
            .spawn();
    }
    #[cfg(not(windows))]
    let _ = (title, body);
}

fn announce(title: &str, body: &str) {
    ting();
    toast(title, body);
}

/// Month and day of a birthday written as year-month-day. 29 February is celebrated on the 28th in other years.
fn birthday_today(text: &str) -> Option<i32> {
    let mut parts = text.split('-').map(|p| p.parse::<u32>().ok());
    let (year, month, mut day) = (parts.next()??, parts.next()??, parts.next()??);
    let now = Local::now();
    let leap = now.year() % 4 == 0 && (now.year() % 100 != 0 || now.year() % 400 == 0);
    if month == 2 && day == 29 && !leap {
        day = 28;
    }
    (now.month() == month && now.day() == day).then_some(now.year() - year as i32)
}

impl App {
    /// Called once a frame after the friends service: rings for new messages, wishes good morning and happy birthday.
    pub(super) fn notify_tick(&mut self) {
        let reading = if self.page == super::Page::Friends { self.open_chat.clone() } else { None };
        for (name, text) in self.social.fresh_messages(reading.as_deref()) {
            announce(&name, &text);
        }
        if self.roy_account.needs_sign_in() || self.roy_account.session().1.is_none() {
            return;
        }
        let now = Local::now();
        let name = self.roy_account.display_name().to_owned();
        let comma = if name.is_empty() { String::new() } else { format!(", {name}") };
        let setting = |key: &str| self.library.settings.get(key).cloned();
        if let Some(age) = birthday_today(self.roy_account.birthday())
            && now.hour() >= 7
            && self.greeted_birthday != now.year()
            && setting("notify_birthday").and_then(|v| v.as_i64()) != Some(now.year() as i64)
        {
            self.greeted_birthday = now.year();
            self.send("preferences", "settings.set", json!({"notify_birthday": now.year()}));
            announce(&format!("Happy birthday{comma}! 🎂"), &format!("Wishing you a day full of great songs as you turn {age}!"));
            self.birthday_dialog = Some(age);
        } else if (7..=10).contains(&now.hour()) {
            let day = now.format("%Y-%m-%d").to_string();
            if self.greeted_day != day && setting("notify_morning").and_then(|v| v.as_str().map(str::to_owned)).as_deref() != Some(day.as_str()) {
                self.greeted_day = day.clone();
                self.send("preferences", "settings.set", json!({"notify_morning": day}));
                let lines = [
                    "Start the day with a great song",
                    "Rise and shine, new music is waiting for you",
                    "Have a lovely day, your music is ready",
                ];
                announce(&format!("Good morning{comma} ☀️"), lines[now.ordinal() as usize % lines.len()]);
            }
        }
    }

    pub(super) fn birthday_dialog(&mut self, ctx: &egui::Context) {
        let Some(age) = self.birthday_dialog else { return };
        let top = self.dialog_top();
        let name = self.roy_account.display_name().to_owned();
        let mut thanks = false;
        let asked = Self::dialog(ctx, top, "birthday", true, |ui| {
            ui.set_width(420.);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("🎂").size(54.));
                ui.label(RichText::new(format!("Happy birthday{}!", if name.is_empty() { String::new() } else { format!(", {name}") })).size(26.).strong());
                ui.label(RichText::new(format!("Wishing you a day full of great songs as you turn {age}.")).color(MUTED));
                ui.add_space(16.);
                thanks = ui.add(egui::Button::new(RichText::new("Thank you!").strong().color(egui::Color32::BLACK)).fill(egui::Color32::WHITE).corner_radius(18).min_size(egui::vec2(140., 36.))).clicked();
            });
        });
        if asked || thanks {
            self.birthday_dialog = None;
        }
    }
}
