use anyhow::{Context, Result, ensure};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient, activity};
use serde_json::{Value, json};

#[derive(Default)]
pub struct Presence {
    client: Option<DiscordIpcClient>,
}
impl Presence {
    pub fn connect(&mut self, application_id: &str) -> Result<Value> {
        ensure!(
            !application_id.is_empty() && application_id.chars().all(|c| c.is_ascii_digit()),
            "Invalid Discord application ID"
        );
        self.close();
        let mut client = DiscordIpcClient::new(application_id);
        client.connect()?;
        self.client = Some(client);
        Ok(self.status())
    }
    pub fn status(&self) -> Value {
        json!({"enabled":self.client.is_some(),"transport":"ipc","confirmation":"local_transport_only"})
    }
    pub fn update(&mut self, p: &Value) -> Result<Value> {
        let title = p["title"].as_str().context("Missing presence title")?;
        let artist = p["artist"].as_str().unwrap_or("");
        let mut payload = activity::Activity::new()
            .details(title)
            .state(artist)
            .activity_type(activity::ActivityType::Listening);
        if let Some(start) = p["started_at"].as_i64() {
            let mut times = activity::Timestamps::new().start(start);
            if let Some(end) = p["ends_at"].as_i64() {
                times = times.end(end);
            }
            payload = payload.timestamps(times);
        }
        if let Some(cover) = p["cover"].as_str() {
            ensure!(cover.starts_with("https://"), "Cover must use HTTPS");
            payload = payload.assets(activity::Assets::new().large_image(cover));
        }
        let client = self
            .client
            .as_mut()
            .context("Discord presence is disabled; connect first")?;
        client.set_activity(payload)?;
        Ok(json!({"sent":true}))
    }
    pub fn clear(&mut self) -> Result<Value> {
        if let Some(client) = &mut self.client {
            client.clear_activity()?;
        }
        Ok(json!(true))
    }
    pub fn close(&mut self) {
        if let Some(mut client) = self.client.take() {
            let _ = client.clear_activity();
            let _ = client.close();
        }
    }
}
impl Drop for Presence {
    fn drop(&mut self) {
        self.close();
    }
}
