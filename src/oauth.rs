use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub client_id: String,
    pub redirect_uri: String,
    #[serde(default)]
    pub client_secret: Option<String>,
}
struct Pending {
    config: Config,
    verifier: String,
    state: String,
    start: Instant,
}
#[derive(Default)]
pub struct OAuth {
    pending: BTreeMap<String, Pending>,
    tokens: BTreeMap<String, Value>,
    configs: BTreeMap<String, Config>,
    expires: BTreeMap<String, Instant>,
}
impl OAuth {
    pub(crate) fn private_session(&self) -> Value {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let expiry: BTreeMap<_, _> = self
            .expires
            .iter()
            .map(|(k, v)| {
                (
                    k,
                    now + v.saturating_duration_since(Instant::now()).as_secs(),
                )
            })
            .collect();
        json!({"tokens":self.tokens,"configs":self.configs,"expires_at":expiry})
    }
    pub(crate) fn restore(&mut self, value: Value) -> Result<()> {
        self.tokens = serde_json::from_value(value["tokens"].clone())?;
        self.configs = serde_json::from_value(value["configs"].clone())?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let expires: BTreeMap<String, u64> = serde_json::from_value(value["expires_at"].clone())?;
        self.expires = expires
            .into_iter()
            .map(|(k, t)| {
                (
                    k,
                    Instant::now() + Duration::from_secs(t.saturating_sub(now).min(86400)),
                )
            })
            .collect();
        Ok(())
    }
    pub fn begin(&mut self, provider: &str, config: Config) -> Result<Value> {
        ensure!(
            !config.client_id.trim().is_empty(),
            "OAuth client_id is required; register your own application"
        );
        let redirect = url::Url::parse(&config.redirect_uri)?;
        ensure!(
            redirect.scheme() == "http"
                && redirect
                    .host_str()
                    .is_some_and(|h| ["127.0.0.1", "[::1]", "localhost"].contains(&h)),
            "OAuth callback must be a loopback HTTP URL"
        );
        let (authorize, scope) = match provider {
            "google" => (
                "https://accounts.google.com/o/oauth2/v2/auth",
                "https://www.googleapis.com/auth/youtube.readonly",
            ),
            "spotify" => (
                "https://accounts.spotify.com/authorize",
                "playlist-read-private playlist-read-collaborative user-library-read",
            ),
            _ => anyhow::bail!("Unsupported OAuth provider"),
        };
        let verifier = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
        let state = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut url = url::Url::parse(authorize)?;
        url.query_pairs_mut().extend_pairs([
            ("client_id", config.client_id.as_str()),
            ("redirect_uri", config.redirect_uri.as_str()),
            ("response_type", "code"),
            ("scope", scope),
            ("state", state.as_str()),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
        ]);
        if provider == "google" {
            url.query_pairs_mut().append_pair("access_type", "offline");
        }
        self.pending.insert(
            provider.into(),
            Pending {
                config,
                verifier,
                state,
                start: Instant::now(),
            },
        );
        Ok(
            json!({"authorization_url":url.as_str(),"expires_in":600,"callback_owner":"host_application"}),
        )
    }
    pub fn finish(
        &mut self,
        client: &reqwest::blocking::Client,
        provider: &str,
        state: &str,
        code: &str,
    ) -> Result<Value> {
        let pending = self.pending.get(provider).context("No pending sign-in")?;
        ensure!(
            pending.start.elapsed() < Duration::from_secs(600),
            "Sign-in expired"
        );
        ensure!(state == pending.state, "OAuth state mismatch");
        ensure!(!code.is_empty(), "Missing authorization code");
        let endpoint = match provider {
            "google" => "https://oauth2.googleapis.com/token",
            "spotify" => "https://accounts.spotify.com/api/token",
            _ => anyhow::bail!("Unsupported OAuth provider"),
        };
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", pending.config.client_id.as_str()),
            ("redirect_uri", pending.config.redirect_uri.as_str()),
            ("code_verifier", pending.verifier.as_str()),
        ];
        if let Some(secret) = pending.config.client_secret.as_deref() {
            form.push(("client_secret", secret));
        }
        let response = client.post(endpoint).form(&form).send()?;
        ensure!(
            response.status().is_success(),
            "OAuth token exchange failed (HTTP {})",
            response.status()
        );
        let token: Value = response.json()?;
        ensure!(
            token["access_token"].is_string(),
            "OAuth response did not contain an access token"
        );
        let expiry = token["expires_in"].as_u64().unwrap_or(3600).min(86400);
        self.expires.insert(
            provider.into(),
            Instant::now() + Duration::from_secs(expiry),
        );
        self.configs.insert(provider.into(), pending.config.clone());
        self.tokens.insert(provider.into(), token);
        self.pending.remove(provider);
        Ok(self.status(provider))
    }
    pub fn token(&self, provider: &str) -> Result<&str> {
        ensure!(
            self.expires
                .get(provider)
                .is_none_or(|t| Instant::now() < *t),
            "Session expired; call oauth.refresh"
        );
        self.tokens
            .get(provider)
            .and_then(|v| v["access_token"].as_str())
            .context("Sign in required")
    }
    pub fn status(&self, provider: &str) -> Value {
        json!({"provider":provider,"signed_in":self.tokens.contains_key(provider),"pending":self.pending.contains_key(provider),"persistent":crate::vault::SUPPORTED,"expires_in":self.expires.get(provider).map(|t|t.saturating_duration_since(Instant::now()).as_secs())})
    }
    pub fn refresh(&mut self, client: &reqwest::blocking::Client, provider: &str) -> Result<Value> {
        let config = self.configs.get(provider).context("No OAuth session")?;
        let old = self.tokens.get(provider).context("No OAuth session")?;
        let refresh = old["refresh_token"]
            .as_str()
            .context("Provider did not grant a refresh token; sign in again")?;
        let endpoint = match provider {
            "google" => "https://oauth2.googleapis.com/token",
            "spotify" => "https://accounts.spotify.com/api/token",
            _ => anyhow::bail!("Unknown OAuth provider"),
        };
        let mut form = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
            ("client_id", config.client_id.as_str()),
        ];
        if let Some(secret) = config.client_secret.as_deref() {
            form.push(("client_secret", secret));
        }
        let response = client.post(endpoint).form(&form).send()?;
        ensure!(
            response.status().is_success(),
            "OAuth refresh failed (HTTP {})",
            response.status()
        );
        let mut token: Value = response.json()?;
        ensure!(
            token["access_token"].is_string(),
            "Missing refreshed access token"
        );
        if token["refresh_token"].is_null() {
            token["refresh_token"] = Value::String(refresh.into());
        }
        let expiry = token["expires_in"].as_u64().unwrap_or(3600).min(86400);
        self.expires.insert(
            provider.into(),
            Instant::now() + Duration::from_secs(expiry),
        );
        self.tokens.insert(provider.into(), token);
        Ok(self.status(provider))
    }
    /// Accept an access token minted for this user by Roynit ID, which holds the provider's refresh token.
    /// No client config is kept, so `refresh` is refused here and the app asks Roynit ID for a new token instead.
    pub fn adopt(&mut self, provider: &str, access_token: &str, expires_in: u64) -> Result<Value> {
        ensure!(["google", "spotify"].contains(&provider), "Unsupported OAuth provider");
        ensure!(
            !access_token.is_empty() && access_token.len() < 4096,
            "Missing access token"
        );
        self.configs.remove(provider);
        self.pending.remove(provider);
        self.expires.insert(
            provider.into(),
            Instant::now() + Duration::from_secs(expires_in.clamp(60, 86400)),
        );
        self.tokens.insert(
            provider.into(),
            json!({"access_token":access_token,"token_type":"Bearer"}),
        );
        Ok(self.status(provider))
    }
    pub fn logout(&mut self, provider: &str) {
        self.tokens.remove(provider);
        self.pending.remove(provider);
        self.configs.remove(provider);
        self.expires.remove(provider);
    }
}

pub fn youtube_library(
    client: &reqwest::blocking::Client,
    token: &str,
    kind: &str,
    playlist: Option<&str>,
) -> Result<Value> {
    youtube_library_pages(client, token, kind, playlist, usize::MAX)
}

pub(crate) fn youtube_library_pages(
    client: &reqwest::blocking::Client,
    token: &str,
    kind: &str,
    playlist: Option<&str>,
    pages: usize,
) -> Result<Value> {
    let (route, mut query) = match kind {
        "playlists" => (
            "playlists",
            vec![
                ("part", "snippet,contentDetails".into()),
                ("mine", "true".into()),
            ],
        ),
        "likes" => (
            "videos",
            vec![
                ("part", "snippet,contentDetails".into()),
                ("myRating", "like".into()),
            ],
        ),
        "tracks" => (
            "playlistItems",
            vec![
                ("part", "snippet,contentDetails".into()),
                (
                    "playlistId",
                    playlist.context("playlist_id is required")?.into(),
                ),
            ],
        ),
        _ => anyhow::bail!("kind must be playlists, likes or tracks"),
    };
    query.push(("maxResults", "50".into()));
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut next = String::new();
    for _ in 0..pages {
        crate::jobs::check_cancelled()?;
        let mut q = query.clone();
        if !next.is_empty() {
            q.push(("pageToken", next));
        }
        let value: Value = client
            .get(format!("https://www.googleapis.com/youtube/v3/{route}"))
            .bearer_auth(token)
            .timeout(std::time::Duration::from_secs(if pages == usize::MAX {
                30
            } else {
                6
            }))
            .query(&q)
            .send()?
            .error_for_status()?
            .json()?;
        items.extend(value["items"].as_array().cloned().unwrap_or_default());
        next = value["nextPageToken"].as_str().unwrap_or("").into();
        if next.is_empty() {
            break;
        }
        ensure!(seen.insert(next.clone()), "Repeated pagination token");
        ensure!(items.len() < 100000, "Library exceeds pagination limit");
    }
    Ok(json!({"items":items}))
}

pub fn map_youtube_tracks(value: &Value) -> Vec<crate::model::Track> {
    value["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let snippet = &item["snippet"];
            let id = item["contentDetails"]["videoId"]
                .as_str()
                .or(snippet["resourceId"]["videoId"].as_str())
                .or(item["id"].as_str())?;
            if crate::youtube::validate_video(id).is_err() {
                return None;
            }
            let title = snippet["title"].as_str()?;
            if ["Deleted video", "Private video"].contains(&title) {
                return None;
            }
            let cover = ["maxres", "standard", "high", "medium", "default"]
                .iter()
                .find_map(|key| snippet["thumbnails"][key]["url"].as_str())
                .map(str::to_string);
            Some(crate::model::Track {
                id: format!("youtube:{id}"),
                title: title.into(),
                artist: snippet["videoOwnerChannelTitle"]
                    .as_str()
                    .or(snippet["channelTitle"].as_str())
                    .unwrap_or("")
                    .into(),
                album: String::new(),
                duration: crate::imports::iso_duration(
                    item["contentDetails"]["duration"].as_str().unwrap_or(""),
                ),
                source: format!("https://www.youtube.com/watch?v={id}"),
                platform: "youtube".into(),
                cover,
                local_path: None,
                cache_path: None,
            })
        })
        .collect()
}
