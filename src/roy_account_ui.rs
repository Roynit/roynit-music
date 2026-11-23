use reqwest::{StatusCode, blocking::Client};
use riff_core::{account_music::Snapshot, model::Library, vault};
use serde_json::{Value, json};
use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

const DEFAULT_ENDPOINT: &str = "https://roynit.dangz.org/api/account";
type AccountResult = Result<Value, (Option<StatusCode>, String)>;
type Pending = mpsc::Receiver<(Action, AccountResult)>;

const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// A provider account that can be linked to a Roynit ID.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    Google,
    Spotify,
}

impl Provider {
    const ALL: [Self; 2] = [Self::Google, Self::Spotify];
    /// Spotify linking is paused on their side; the server still supports it.
    const SHOWN: [Self; 1] = [Self::Google];

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key() == key)
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Spotify => "spotify",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Google => "Google / YouTube",
            Self::Spotify => "Spotify",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Google => "YouTube likes and playlists, and the history that shapes your Mixes",
            Self::Spotify => "Import your liked songs and playlists",
        }
    }
}

/// What the app should do with the main player core after a link changes.
pub enum Event {
    /// A fresh access token minted by Roynit ID; `fresh` is false for the hourly renewals.
    Adopt {
        provider: Provider,
        token: String,
        expires_in: u64,
        fresh: bool,
    },
    /// The link is gone, so the token derived from it must go too.
    Drop(Provider),
}

#[derive(Clone, Copy, PartialEq)]
enum Action {
    Check,
    SignIn,
    Register,
    Google,
    Pickup,
    Save,
    Avatar,
    AvatarRemove,
    Banner,
    BannerRemove,
    Username,
    CheckName,
    Password,
    SignOut,
    ReadMusic,
    WriteMusic,
    Links,
    LinkStart(Provider),
    LinkToken(Provider),
    LinkRemove(Provider),
}

pub struct State {
    client: Client,
    endpoint: String,
    path: PathBuf,
    token: Option<String>,
    profile: Value,
    register: bool,
    email: String,
    username: String,
    password: String,
    display_name: String,
    bio: String,
    editing: bool,
    pending: Option<Pending>,
    pickup: Option<String>,
    next_poll: Instant,
    deadline: Instant,
    status: String,
    error: bool,
    cloud: Value,
    revision: Option<u64>,
    automatic: bool,
    next_sync: Instant,
    uploaded: Option<String>,
    uploading: Option<String>,
    todo: VecDeque<Action>,
    links: Value,
    available: Value,
    linking: Option<(Provider, Instant, Instant)>,
    adopted: HashSet<Provider>,
    events: Vec<Event>,
    link_notice: Option<(String, bool)>,
    renew_at: Instant,
    security_open: bool,
    new_username: String,
    name_state: NameState,
    name_due: Option<Instant>,
    name_checking: String,
    current_password: String,
    new_password: String,
    confirm_password: String,
}

/// Where the username being typed stands: unchanged, waiting to be checked, or known free or taken.
#[derive(Clone, Copy, PartialEq, Default)]
enum NameState {
    #[default]
    Own,
    Waiting,
    Checking,
    Free,
    Taken,
    Invalid,
}

const RENEW_EVERY: Duration = Duration::from_secs(40 * 60);

impl State {
    pub fn new(root: &std::path::Path) -> Self {
        let endpoint = std::env::var("ROYNIT_ACCOUNT_ENDPOINT")
            .ok()
            .and_then(|raw| {
                let url = url::Url::parse(&raw).ok()?;
                let local = url.scheme() == "http"
                    && matches!(url.host_str(), Some("localhost" | "127.0.0.1"));
                (url.username().is_empty()
                    && url.password().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none()
                    && (url.scheme() == "https" || local))
                    .then(|| raw.trim_end_matches('/').to_owned())
            })
            .unwrap_or_else(|| DEFAULT_ENDPOINT.into());
        let path = root.join("roynit-account.dpapi");
        let token = vault::load(&path).ok().flatten().and_then(|saved| {
            (saved["endpoint"].as_str() == Some(&endpoint))
                .then(|| saved["token"].as_str().map(str::to_owned))
                .flatten()
        });
        let mut state = Self {
            client: Client::builder()
                .timeout(Duration::from_secs(18))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("account HTTP client"),
            endpoint,
            path,
            token,
            profile: Value::Null,
            register: false,
            email: String::new(),
            username: String::new(),
            password: String::new(),
            display_name: String::new(),
            bio: String::new(),
            editing: false,
            pending: None,
            pickup: None,
            next_poll: Instant::now(),
            deadline: Instant::now(),
            status: String::new(),
            error: false,
            cloud: Value::Null,
            revision: None,
            automatic: false,
            next_sync: Instant::now(),
            uploaded: None,
            uploading: None,
            todo: VecDeque::new(),
            links: Value::Null,
            available: Value::Null,
            linking: None,
            adopted: HashSet::new(),
            events: Vec::new(),
            link_notice: None,
            renew_at: Instant::now() + RENEW_EVERY,
            security_open: false,
            new_username: String::new(),
            name_state: NameState::Own,
            name_due: None,
            name_checking: String::new(),
            current_password: String::new(),
            new_password: String::new(),
            confirm_password: String::new(),
        };
        if state.token.is_some() {
            state.request(Action::Check, Value::Null);
        }
        state
    }

    /// Nobody is signed in and no saved session exists to try: the app asks for an account first.
    pub fn needs_sign_in(&self) -> bool {
        self.profile.is_null() && self.token.is_none()
    }

    pub fn is_signed_out(&self) -> bool {
        self.profile.is_null()
    }

    pub fn sign_out(&mut self) {
        self.automatic = false;
        self.request(Action::SignOut, Value::Null);
    }

    /// The provider account linked to this Roynit ID, as it should be shown ("name@gmail.com").
    pub fn linked(&self, provider: Provider) -> Option<String> {
        self.links
            .as_array()?
            .iter()
            .find(|link| link["provider"] == provider.key())
            .map(|link| link["label"].as_str().unwrap_or_default().to_owned())
    }

    pub fn is_linking(&self, provider: Provider) -> bool {
        self.linking.is_some_and(|(p, ..)| p == provider)
    }

    /// Link a provider through the browser, or renew its token when it is already linked.
    pub fn connect(&mut self, provider: Provider) {
        if self.linked(provider).is_some() {
            self.todo.push_back(Action::LinkToken(provider));
        } else {
            self.todo.push_back(Action::LinkStart(provider));
        }
    }

    pub fn unlink(&mut self, provider: Provider) {
        self.linking = None;
        self.todo.push_back(Action::LinkRemove(provider));
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub fn take_link_notice(&mut self) -> Option<(String, bool)> {
        self.link_notice.take()
    }

    fn link_message(&mut self, text: impl Into<String>, error: bool) {
        let text = text.into();
        self.link_notice = Some((text.clone(), error));
        self.notice(text, error);
    }

    /// The server address and the session token, while somebody is signed in.
    pub(super) fn session(&self) -> (&str, Option<&str>) {
        let token = if self.profile.is_null() { None } else { self.token.as_deref() };
        (&self.endpoint, token)
    }

    pub(super) fn has_password(&self) -> bool {
        self.profile["has_password"] == true
    }

    pub(super) fn username(&self) -> &str {
        self.profile["username"].as_str().unwrap_or_default()
    }

    /// Opens "Username & password" with the current username filled in.
    pub(super) fn open_security(&mut self) {
        self.security_open = true;
        self.new_username = self.username().to_owned();
        self.name_state = NameState::Own;
        self.name_due = None;
        (self.current_password, self.new_password, self.confirm_password) = Default::default();
    }

    /// Called when the username box changes: normalise it and schedule a check for whether it is free.
    fn edit_username(&mut self) {
        let text = self.new_username.trim().trim_start_matches('@').to_lowercase();
        let valid = (3..=24).contains(&text.len())
            && text.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        self.name_state = if text == self.username() {
            NameState::Own
        } else if valid {
            NameState::Waiting
        } else {
            NameState::Invalid
        };
        self.name_due = (self.name_state == NameState::Waiting)
            .then(|| Instant::now() + Duration::from_millis(450));
        self.new_username = text;
    }

    pub fn name(&self) -> &str {
        self.profile["display_name"].as_str().unwrap_or("Listener")
    }

    fn notice(&mut self, text: impl Into<String>, error: bool) {
        self.status = text.into();
        self.error = error;
    }

    fn request(&mut self, action: Action, body: Value) {
        self.request_with(action, body, None);
    }

    fn request_with(&mut self, action: Action, body: Value, bytes: Option<Vec<u8>>) {
        if self.pending.is_some() {
            return;
        }
        let (method, path, needs_token) = match action {
            Action::Check => (reqwest::Method::GET, "/me".to_owned(), true),
            Action::SignIn => (reqwest::Method::POST, "/login".to_owned(), false),
            Action::Register => (reqwest::Method::POST, "/register".to_owned(), false),
            Action::Google => (reqwest::Method::POST, "/google/start".to_owned(), false),
            Action::Pickup => (reqwest::Method::POST, "/google/pickup".to_owned(), false),
            Action::Save => (reqwest::Method::PATCH, "/me".to_owned(), true),
            Action::Avatar => (reqwest::Method::PUT, "/me/avatar".to_owned(), true),
            Action::AvatarRemove => (reqwest::Method::DELETE, "/me/avatar".to_owned(), true),
            Action::Username => (reqwest::Method::PATCH, "/me".to_owned(), true),
            Action::CheckName => (
                reqwest::Method::GET,
                format!("/username/check?name={}", self.name_checking),
                true,
            ),
            Action::Password => (reqwest::Method::PUT, "/me/password".to_owned(), true),
            Action::Banner => (reqwest::Method::PUT, "/me/banner".to_owned(), true),
            Action::BannerRemove => (reqwest::Method::DELETE, "/me/banner".to_owned(), true),
            Action::SignOut => (reqwest::Method::POST, "/logout".to_owned(), true),
            Action::ReadMusic => (reqwest::Method::GET, "/music".to_owned(), true),
            Action::WriteMusic => (reqwest::Method::PUT, "/music".to_owned(), true),
            Action::Links => (reqwest::Method::GET, "/links".to_owned(), true),
            Action::LinkStart(p) => (
                reqwest::Method::POST,
                format!("/link/{}/start", p.key()),
                true,
            ),
            Action::LinkToken(p) => (
                reqwest::Method::POST,
                format!("/link/{}/token", p.key()),
                true,
            ),
            Action::LinkRemove(p) => (reqwest::Method::DELETE, format!("/link/{}", p.key()), true),
        };
        let address = self.endpoint.clone() + &path;
        let token = if needs_token {
            self.token.clone()
        } else {
            None
        };
        let client = self.client.clone();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        std::thread::spawn(move || {
            let result = (|| {
                let mut request = client.request(method, address);
                if let Some(token) = token {
                    request = request.bearer_auth(token);
                }
                if let Some(bytes) = bytes {
                    request = request
                        .header(reqwest::header::CONTENT_TYPE, "image/jpeg")
                        .body(bytes);
                } else if !body.is_null() {
                    request = request.json(&body);
                }
                let response = request
                    .send()
                    .map_err(|_| (None, "Could not reach Roynit ID.".to_owned()))?;
                let status = response.status();
                let data: Value = response.json().map_err(|_| {
                    (
                        Some(status),
                        "Roynit ID is unavailable at this address.".to_owned(),
                    )
                })?;
                if status.is_success() {
                    Ok(data)
                } else {
                    Err((
                        Some(status),
                        data["error"]
                            .as_str()
                            .unwrap_or("Roynit ID request failed.")
                            .to_owned(),
                    ))
                }
            })();
            let _ = tx.send((action, result));
        });
    }

    fn clear_session(&mut self) {
        self.events.extend(self.adopted.drain().map(Event::Drop));
        self.todo.clear();
        self.linking = None;
        self.links = Value::Null;
        self.token = None;
        self.profile = Value::Null;
        self.cloud = Value::Null;
        self.revision = None;
        self.uploaded = None;
        self.uploading = None;
        self.automatic = false;
        self.editing = false;
        if self.path.is_file() && std::fs::remove_file(&self.path).is_err() {
            self.notice(
                "Signed out. The local session file could not be removed.",
                true,
            );
            return;
        }
        self.notice("Signed out.", false);
    }

    /// The edit boxes always start from what the server last said.
    fn sync_fields(&mut self) {
        let text = |key: &str| self.profile[key].as_str().unwrap_or_default().to_owned();
        (self.display_name, self.bio) = (text("display_name"), text("bio"));
    }

    /// Where this account's picture is served from; the version in the address changes with every upload.
    fn avatar_uri(&self) -> Option<String> {
        let version = self.profile["avatar_url"]
            .as_str()?
            .rsplit_once("?v=")?
            .1
            .to_owned();
        version.bytes().all(|b| b.is_ascii_digit()).then_some(())?;
        Some(format!(
            "{}/avatar/{}?v={version}",
            self.endpoint,
            self.profile["id"].as_str()?
        ))
    }

    /// Where this account's cover picture is served from, like the avatar's address.
    fn banner_uri(&self) -> Option<String> {
        let version = self.profile["banner_url"].as_str()?.rsplit_once("?v=")?.1.to_owned();
        version.bytes().all(|b| b.is_ascii_digit()).then_some(())?;
        Some(format!("{}/banner/{}?v={version}", self.endpoint, self.profile["id"].as_str()?))
    }

    fn choose_banner(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Pictures", &["jpg", "jpeg", "png", "webp"])
            .pick_file()
        else {
            return;
        };
        match prepare_banner(&path) {
            Ok(bytes) => self.request_with(Action::Banner, Value::Null, Some(bytes)),
            Err(error) => self.notice(error, true),
        }
    }

    fn choose_avatar(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Pictures", &["jpg", "jpeg", "png", "webp"])
            .pick_file()
        else {
            return;
        };
        match prepare_avatar(&path) {
            Ok(bytes) => self.request_with(Action::Avatar, Value::Null, Some(bytes)),
            Err(error) => self.notice(error, true),
        }
    }

    fn accept_account(&mut self, data: Value) {
        self.profile = data["account"].clone();
        self.sync_fields();
        if let Some(token) = data["token"].as_str() {
            self.token = Some(token.to_owned());
            if vault::save(&self.path, &json!({"endpoint":self.endpoint,"token":token})).is_err() {
                self.notice(
                    "Signed in for this session. Windows could not save the sign-in.",
                    true,
                );
            }
        }
        self.request(Action::ReadMusic, Value::Null);
        self.todo.push_back(Action::Links);
    }

    pub fn tick(&mut self, library: &Library) {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok((action, result)) => {
                    self.pending = None;
                    match result {
                        Ok(data) => match action {
                            Action::Check | Action::SignIn | Action::Register => {
                                self.accept_account(data)
                            }
                            Action::Pickup if data["pending"] == true => {}
                            Action::Pickup => {
                                self.pickup = None;
                                self.accept_account(data);
                            }
                            Action::Google => {
                                let url = data["url"]
                                    .as_str()
                                    .and_then(|text| url::Url::parse(text).ok())
                                    .filter(|url| {
                                        url.scheme() == "https"
                                            && url.host_str() == Some("accounts.google.com")
                                    });
                                if let (Some(url), Some(pickup)) = (url, data["pickup"].as_str())
                                    && open::that(url.as_str()).is_ok()
                                {
                                    self.pickup = Some(pickup.to_owned());
                                    self.deadline = Instant::now() + Duration::from_secs(600);
                                    self.next_poll = Instant::now() + POLL_INTERVAL;
                                    self.notice(
                                        "Finish signing in with Google in your browser.",
                                        false,
                                    );
                                } else {
                                    self.notice("Could not open Google sign-in.", true);
                                }
                            }
                            Action::Save
                            | Action::Avatar
                            | Action::AvatarRemove
                            | Action::Banner
                            | Action::BannerRemove => {
                                self.profile = data["account"].clone();
                                self.sync_fields();
                                if action == Action::Save {
                                    self.editing = false;
                                }
                                self.notice(
                                    match action {
                                        Action::Save => "Profile updated.",
                                        Action::Avatar => "Profile picture updated.",
                                        Action::Banner => "Cover picture updated.",
                                        Action::BannerRemove => "Cover picture removed.",
                                        _ => "Profile picture removed.",
                                    },
                                    false,
                                );
                            }
                            Action::Username | Action::Password => {
                                self.profile = data["account"].clone();
                                if action == Action::Username {
                                    self.name_state = NameState::Own;
                                    let text = format!("Your username is now @{}.", self.username());
                                    self.notice(text, false);
                                } else {
                                    (self.current_password, self.new_password, self.confirm_password) =
                                        Default::default();
                                    self.notice("Password saved. Your other devices were signed out.", false);
                                }
                            }
                            Action::CheckName => {
                                if data["username"].as_str() == Some(self.new_username.as_str()) {
                                    self.name_state = if data["available"] == true {
                                        NameState::Free
                                    } else {
                                        NameState::Taken
                                    };
                                }
                            }
                            Action::SignOut => self.clear_session(),
                            Action::Links | Action::LinkRemove(_) => {
                                self.links = data["links"].clone();
                                self.available = data["available"].clone();
                                if let Action::LinkRemove(provider) = action {
                                    self.adopted.remove(&provider);
                                    self.events.push(Event::Drop(provider));
                                    self.link_message(
                                        format!("{} unlinked.", provider.title()),
                                        false,
                                    );
                                }
                                if let Some((provider, ..)) = self.linking
                                    && self.linked(provider).is_some()
                                {
                                    self.linking = None;
                                    self.link_message(
                                        format!("{} linked.", provider.title()),
                                        false,
                                    );
                                    self.todo.push_back(Action::LinkToken(provider));
                                    // Linking Google also turns on Google sign-in: reload the profile's flags.
                                    self.todo.push_back(Action::Check);
                                } else if self.linking.is_none() && action == Action::Links {
                                    // First look after sign-in (or a failed renewal): fetch tokens for what is linked.
                                    for provider in Provider::ALL {
                                        if self.linked(provider).is_some()
                                            && !self.adopted.contains(&provider)
                                        {
                                            self.todo.push_back(Action::LinkToken(provider));
                                        }
                                    }
                                }
                            }
                            Action::LinkStart(provider) => {
                                let url = data["url"]
                                    .as_str()
                                    .and_then(|text| url::Url::parse(text).ok())
                                    .filter(|url| {
                                        url.scheme() == "https"
                                            && matches!(
                                                url.host_str(),
                                                Some(
                                                    "accounts.google.com" | "accounts.spotify.com"
                                                )
                                            )
                                            || self.endpoint.starts_with("http://127.0.0.1")
                                    });
                                if let Some(url) = url
                                    && open::that(url.as_str()).is_ok()
                                {
                                    let now = Instant::now();
                                    self.linking = Some((
                                        provider,
                                        now + Duration::from_secs(600),
                                        now + POLL_INTERVAL,
                                    ));
                                    self.link_message(
                                        format!(
                                            "Finish linking {} in your browser.",
                                            provider.title()
                                        ),
                                        false,
                                    );
                                } else {
                                    self.link_message("Could not open the sign-in page.", true);
                                }
                            }
                            Action::LinkToken(provider) => {
                                if let Some(token) = data["access_token"].as_str() {
                                    let fresh = self.adopted.insert(provider);
                                    self.events.push(Event::Adopt {
                                        provider,
                                        token: token.to_owned(),
                                        expires_in: data["expires_in"].as_u64().unwrap_or(3600),
                                        fresh,
                                    });
                                }
                            }
                            Action::ReadMusic => {
                                self.cloud = data;
                                if self.cloud["revision"] == 0 {
                                    self.revision = Some(0);
                                } else if self.revision != self.cloud["revision"].as_u64() {
                                    self.revision = None;
                                    self.automatic = false;
                                }
                            }
                            Action::WriteMusic => {
                                self.revision = data["revision"].as_u64();
                                self.uploaded = self.uploading.take();
                                self.cloud = data;
                                self.notice("Music library synced.", false);
                            }
                        },
                        Err((_, _)) if action == Action::CheckName => {
                            // Try the check again shortly; nothing to tell the user.
                            self.name_state = NameState::Waiting;
                            self.name_due = Some(Instant::now() + Duration::from_secs(3));
                        }
                        Err((status, error)) => {
                            self.uploading = None;
                            if action == Action::Pickup {
                                self.pickup = None;
                            }
                            if status == Some(StatusCode::UNAUTHORIZED)
                                && !matches!(
                                    action,
                                    Action::SignIn
                                        | Action::Register
                                        | Action::Pickup
                                        | Action::Password
                                )
                            {
                                self.clear_session();
                            }
                            if let Action::LinkToken(provider) | Action::LinkStart(provider) =
                                action
                            {
                                // A revoked link disappears on the server; refresh the list so the button says "Link" again.
                                if status == Some(StatusCode::CONFLICT)
                                    || status == Some(StatusCode::NOT_FOUND)
                                {
                                    self.adopted.remove(&provider);
                                    self.events.push(Event::Drop(provider));
                                    self.todo.push_back(Action::Links);
                                }
                                // A renewal that fails in the background stays quiet; only things the user asked for, or a revoked link, say so.
                                if matches!(action, Action::LinkStart(_))
                                    || status == Some(StatusCode::CONFLICT)
                                    || status == Some(StatusCode::NOT_FOUND)
                                {
                                    self.link_notice = Some((error.clone(), true));
                                }
                            }
                            if status == Some(StatusCode::CONFLICT) && action == Action::WriteMusic
                            {
                                self.revision = None;
                                self.automatic = false;
                                self.request(Action::ReadMusic, Value::Null);
                            }
                            let unsupported =
                                matches!(action, Action::Avatar | Action::AvatarRemove)
                                    && matches!(
                                        status,
                                        Some(
                                            StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
                                        )
                                    );
                            self.notice(
                                if unsupported {
                                    "This Roynit ID server does not support profile pictures yet."
                                        .into()
                                } else {
                                    error
                                },
                                true,
                            );
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.notice("Account request stopped.", true);
                }
            }
        }
        if self.name_state == NameState::Waiting
            && self.pending.is_none()
            && self.name_due.is_some_and(|due| Instant::now() >= due)
        {
            self.name_state = NameState::Checking;
            self.name_checking = self.new_username.clone();
            self.request(Action::CheckName, Value::Null);
        }
        if self.pending.is_none() && self.token.is_some() {
            if let Some((provider, deadline, next)) = self.linking {
                if Instant::now() >= deadline {
                    self.linking = None;
                    self.link_message(format!("Linking {} timed out.", provider.title()), true);
                } else if Instant::now() >= next {
                    self.linking = Some((provider, deadline, Instant::now() + POLL_INTERVAL));
                    self.request(Action::Links, Value::Null);
                }
            }
            if self.pending.is_none()
                && let Some(action) = self.todo.pop_front()
            {
                self.request(action, Value::Null);
            }
            if Instant::now() >= self.renew_at {
                self.renew_at = Instant::now() + RENEW_EVERY;
                self.adopted
                    .clone()
                    .into_iter()
                    .for_each(|p| self.todo.push_back(Action::LinkToken(p)));
            }
        }
        if let Some(pickup) = self.pickup.clone() {
            if Instant::now() >= self.deadline {
                self.pickup = None;
                self.notice("Google sign-in expired.", true);
            } else if Instant::now() >= self.next_poll {
                self.next_poll = Instant::now() + POLL_INTERVAL;
                self.request(Action::Pickup, json!({"pickup":pickup}));
            }
            return;
        }
        if self.automatic && self.revision.is_some() && Instant::now() >= self.next_sync {
            self.next_sync = Instant::now() + Duration::from_secs(25);
            self.upload(library, false);
        }
    }

    fn upload(&mut self, library: &Library, force: bool) {
        let Some(revision) = self.revision else {
            return;
        };
        let snapshot = json!(Snapshot::from_library(library));
        let digest = riff_core::store::key(&snapshot.to_string());
        if !force && self.uploaded.as_ref() == Some(&digest) {
            return;
        }
        self.uploading = Some(digest);
        self.request(
            Action::WriteMusic,
            json!({ "snapshot":snapshot, "revision":revision }),
        );
    }
}
use super::{App, MUTED, TEXT, Weight};
use eframe::egui::{self, Color32, RichText};

/// Crop to the middle square, shrink to 256 px and re-encode, so uploads stay small whatever the source.
fn prepare_avatar(path: &std::path::Path) -> Result<Vec<u8>, String> {
    if std::fs::metadata(path)
        .map_err(|_| "Could not read that file.")?
        .len()
        > 25_000_000
    {
        return Err("Choose a picture under 25 MB.".into());
    }
    let picture =
        image::open(path).map_err(|_| "That file is not a supported picture.".to_owned())?;
    let side = picture.width().min(picture.height());
    if side == 0 {
        return Err("That picture is empty.".into());
    }
    let square = picture
        .crop_imm(
            (picture.width() - side) / 2,
            (picture.height() - side) / 2,
            side,
            side,
        )
        .resize_exact(256, 256, image::imageops::FilterType::Lanczos3)
        .to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88)
        .encode_image(&square)
        .map_err(|_| "Could not prepare that picture.".to_owned())?;
    Ok(out)
}

/// Crop to a wide 3:1 strip, shrink to 960 px and re-encode, so the cover stays small and fits any card.
fn prepare_banner(path: &std::path::Path) -> Result<Vec<u8>, String> {
    if std::fs::metadata(path).map_err(|_| "Could not read that file.")?.len() > 25_000_000 {
        return Err("Choose a picture under 25 MB.".into());
    }
    let picture = image::open(path).map_err(|_| "That file is not a supported picture.".to_owned())?;
    let (w, h) = (picture.width(), picture.height());
    if w == 0 || h == 0 {
        return Err("That picture is empty.".into());
    }
    let (cw, ch) = if w > h * 3 { (h * 3, h) } else { (w, w / 3) };
    let strip = picture
        .crop_imm((w - cw) / 2, (h - ch) / 2, cw.max(1), ch.max(1))
        .resize_exact(960, 320, image::imageops::FilterType::Lanczos3)
        .to_rgb8();
    for quality in [86, 74, 60] {
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
            .encode_image(&strip)
            .map_err(|_| "Could not prepare that picture.".to_owned())?;
        if out.len() <= 380_000 {
            return Ok(out);
        }
    }
    Err("That picture is too detailed. Try a simpler one.".into())
}

/// A steady colour per name, used behind initials and in the profile header.
pub(super) fn tint_for(name: &str) -> Color32 {
    let hash = name
        .bytes()
        .fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    egui::ecolor::Hsva::new((hash % 360) as f32 / 360., 0.5, 0.34, 1.).into()
}

fn initials(name: &str) -> String {
    let letters: String = name
        .split_whitespace()
        .take(2)
        .filter_map(|word| word.chars().next())
        .flat_map(char::to_uppercase)
        .collect();
    if letters.is_empty() {
        "?".into()
    } else {
        letters
    }
}

/// The account's round picture (or initials) inside `rect`, with no interaction.
pub(super) fn paint_avatar(ui: &mut egui::Ui, account: &State, rect: egui::Rect) {
    if account.is_signed_out() {
        ui.painter()
            .circle_filled(rect.center(), rect.width() / 2., Color32::from_gray(0x3A));
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "👤",
            egui::FontId::proportional(16.),
            Color32::WHITE,
        );
        return;
    }
    let name = account.name();
    let painter = ui.painter();
    painter.circle_filled(rect.center(), rect.width() / 2., tint_for(name));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initials(name),
        egui::FontId::proportional(rect.width() * 0.38),
        Color32::from_white_alpha(230),
    );
    if let Some(uri) = account.avatar_uri() {
        egui::Image::new(uri)
            .fit_to_exact_size(rect.size())
            .corner_radius((rect.width() / 2.) as u8)
            .paint_at(ui, rect);
    }
}

/// A round picture; with `editable` it shows "Choose photo" on hover and reports clicks.
fn avatar(ui: &mut egui::Ui, account: &State, size: f32, editable: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(size, size),
        if editable {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    let name = account.profile["display_name"]
        .as_str()
        .unwrap_or("Listener");
    ui.painter()
        .circle_filled(rect.center(), size / 2., tint_for(name));
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initials(name),
        egui::FontId::proportional(size * 0.36),
        Color32::from_white_alpha(230),
    );
    if let Some(uri) = account.avatar_uri() {
        egui::Image::new(uri)
            .fit_to_exact_size(rect.size())
            .corner_radius((size / 2.) as u8)
            .paint_at(ui, rect);
    }
    if editable {
        let t =
            ui.ctx()
                .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.15);
        if t > 0. {
            ui.painter().circle_filled(
                rect.center(),
                size / 2.,
                Color32::from_black_alpha((150. * t) as u8),
            );
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Choose photo",
                egui::FontId::proportional((size * 0.11).max(12.)),
                Color32::from_white_alpha((255. * t) as u8),
            );
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    response
}

/// The small round picture in the title bar; a plain person disc when nobody is signed in.
pub(super) fn badge(ui: &mut egui::Ui, account: &State, rect: egui::Rect) -> egui::Response {
    let response = ui.interact(rect, ui.id().with("account-badge"), egui::Sense::click());
    let t = ui
        .ctx()
        .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);
    let painter = ui.painter();
    painter.circle_filled(
        rect.center(),
        rect.width() / 2. + 2. * t,
        Color32::from_gray(0x2A).lerp_to_gamma(Color32::from_gray(0x3A), t),
    );
    if account.is_signed_out() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "👤",
            egui::FontId::proportional(16.),
            Color32::WHITE,
        );
    } else {
        let name = account.name();
        let inner = rect.shrink(2.);
        painter.circle_filled(inner.center(), inner.width() / 2., tint_for(name));
        painter.text(
            inner.center(),
            egui::Align2::CENTER_CENTER,
            initials(name),
            egui::FontId::proportional(13.),
            Color32::from_white_alpha(230),
        );
        if let Some(uri) = account.avatar_uri() {
            egui::Image::new(uri)
                .fit_to_exact_size(inner.size())
                .corner_radius((inner.width() / 2.) as u8)
                .paint_at(ui, inner);
        }
    }
    response
}

/// "Jan 2026" from a Unix time in seconds (civil-from-days, no date crate needed).
fn month_year(secs: i64) -> String {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    const NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!("{} {year}", NAMES[(month - 1) as usize])
}

/// Link Google and Spotify once; Settings → Accounts then works without anyone creating a client ID.
fn services_card(ui: &mut egui::Ui, account: &mut State) {
    card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Connected services").size(19.).bold());
        muted(
            ui,
            "Link an account once and Roy Music uses it for likes, playlists, Mixes and imports. Roynit ID keeps the access; this device only gets short-lived tokens.",
        );
        ui.add_space(8.);
        for provider in Provider::SHOWN {
            let linked = account.linked(provider);
            let working = account.is_linking(provider);
            let ready = account.available[provider.key()] != false;
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_max_width((ui.available_width() - 190.).max(160.));
                    ui.label(RichText::new(provider.title()).bold());
                    let detail = match &linked {
                        Some(label) if !label.is_empty() => format!("Linked as {label}"),
                        Some(_) => "Linked".to_owned(),
                        None => provider.hint().to_owned(),
                    };
                    ui.add(egui::Label::new(RichText::new(detail).small().color(MUTED)).truncate());
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_enabled_ui(account.pending.is_none() || working, |ui| {
                        if working {
                            if ui.button("Cancel").clicked() {
                                account.linking = None;
                                account.notice("Linking cancelled.", false);
                            }
                            ui.spinner();
                        } else if linked.is_some() {
                            if outline_button(ui, "Unlink").clicked() {
                                account.unlink(provider);
                            }
                        } else if ui
                            .add_enabled(ready, egui::Button::new(RichText::new("Link").bold().color(Color32::BLACK))
                                .fill(Color32::WHITE).corner_radius(18).min_size(egui::vec2(96., 34.)))
                            .on_disabled_hover_text("This server has not been set up for linking yet.")
                            .clicked()
                        {
                            account.connect(provider);
                        }
                    });
                });
            });
            ui.add_space(6.);
        }
    });
}

fn outline_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).bold())
            .fill(Color32::TRANSPARENT)
            .stroke(egui::Stroke::new(1.0_f32, Color32::from_gray(0x7C)))
            .corner_radius(18)
            .min_size(egui::vec2(110., 36.)),
    )
}

fn card(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::new()
        .fill(ui.visuals().widgets.inactive.weak_bg_fill)
        .corner_radius(18)
        .inner_margin(20)
}

fn muted(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.add(
        egui::Label::new(RichText::new(text.into()).color(ui.visuals().weak_text_color())).wrap(),
    );
}

fn field(ui: &mut egui::Ui, title: &str, value: &mut String, private: bool, limit: usize) {
    ui.label(RichText::new(title).bold());
    ui.add(
        egui::TextEdit::singleline(value)
            .password(private)
            .char_limit(limit)
            .desired_width(ui.available_width())
            .margin(egui::vec2(12., 9.)),
    );
    ui.add_space(4.);
}

fn primary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let selection = ui.visuals().selection;
    ui.add(
        egui::Button::new(RichText::new(text).bold().color(selection.stroke.color))
            .fill(selection.bg_fill)
            .corner_radius(19)
            .min_size(egui::vec2(136., 36.)),
    )
}

impl App {
    pub(super) fn roynit_profile(&mut self, ui: &mut egui::Ui) {
        ui.add_space(18.);
        if self.roy_account.profile.is_null() && self.roy_account.token.is_none() {
            muted(ui, "Sign in to see your profile.");
        } else if self.roy_account.profile.is_null() {
            self.roynit_sign_in(ui);
        } else {
            self.roynit_dashboard(ui);
        }
        if !self.roy_account.status.is_empty() {
            ui.add_space(8.);
            let color = if self.roy_account.error {
                Color32::from_rgb(244, 155, 158)
            } else {
                ui.visuals().weak_text_color()
            };
            ui.add(egui::Label::new(RichText::new(&self.roy_account.status).color(color)).wrap());
        }
        if self.roy_account.editing && !self.roy_account.profile.is_null() {
            self.profile_dialog(ui.ctx());
        }
    }

    /// Username (unique, checked as you type), password (set or change) and Google sign-in.
    pub(super) fn security_dialog(&mut self, ctx: &egui::Context) {
        if !self.roy_account.security_open || self.roy_account.profile.is_null() {
            return;
        }
        let account = &mut self.roy_account;
        let mut close = false;
        let frame = egui::Frame::popup(&ctx.style())
            .inner_margin(24)
            .corner_radius(12);
        let modal = egui::Modal::new(egui::Id::new("account-security"))
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_width(440.);
                ui.label(RichText::new("Username & password").size(24.).bold());
                ui.add_space(14.);
                let room = (ctx.content_rect().height() - 230.).max(240.);
                egui::ScrollArea::vertical().max_height(room).auto_shrink([false, true]).show(ui, |ui| {
                let busy = account.pending.is_some();
                let green = Color32::from_rgb(0x3B, 0xA5, 0x5C);
                let red = Color32::from_rgb(244, 155, 158);
                let button = |text: &str| {
                    egui::Button::new(RichText::new(text).bold().color(Color32::BLACK))
                        .fill(Color32::WHITE)
                        .corner_radius(18)
                        .min_size(egui::vec2(96., 34.))
                };

                ui.label(RichText::new("Username").bold());
                let before = account.new_username.clone();
                ui.add(
                    egui::TextEdit::singleline(&mut account.new_username)
                        .hint_text("username")
                        .char_limit(26)
                        .desired_width(f32::INFINITY)
                        .margin(egui::vec2(12., 9.)),
                );
                if account.new_username != before {
                    account.edit_username();
                }
                let typed = account.new_username.clone();
                let (note, color) = match account.name_state {
                    NameState::Own => ("This is your username now.".to_owned(), MUTED),
                    NameState::Waiting | NameState::Checking => ("Checking…".to_owned(), MUTED),
                    NameState::Free => (format!("@{typed} is available."), green),
                    NameState::Taken => (format!("@{typed} is already taken. Try another."), red),
                    NameState::Invalid => ("Use 3–24 letters, numbers or underscores.".to_owned(), red),
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(note).small().color(color));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let free = account.name_state == NameState::Free && !busy;
                        if ui.add_enabled(free, button("Change")).clicked() {
                            account.request(Action::Username, json!({"username":typed}));
                        }
                    });
                });
                muted(ui, "Each username belongs to one person. Friends find you by it.");

                ui.add_space(16.);
                ui.separator();
                ui.add_space(10.);
                let has_password = account.profile["has_password"] == true;
                ui.label(RichText::new(if has_password { "Change password" } else { "Set a password" }).bold());
                if has_password {
                    field(ui, "Current password", &mut account.current_password, true, 128);
                } else {
                    muted(ui, "You signed in with Google. A password lets you also sign in with your email or username.");
                    ui.add_space(4.);
                }
                field(ui, "New password", &mut account.new_password, true, 128);
                field(ui, "Confirm new password", &mut account.confirm_password, true, 128);
                let long = account.new_password.chars().count() >= 6;
                let same = account.new_password == account.confirm_password;
                let hint = if !account.new_password.is_empty() && !long {
                    Some(("Use at least 6 characters.", red))
                } else if !account.confirm_password.is_empty() && !same {
                    Some(("The two passwords do not match.", red))
                } else {
                    None
                };
                ui.horizontal(|ui| {
                    if let Some((text, color)) = hint {
                        ui.label(RichText::new(text).small().color(color));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let ready = long
                            && same
                            && !busy
                            && (!has_password || !account.current_password.is_empty());
                        if ui.add_enabled(ready, button(if has_password { "Change" } else { "Set" })).clicked() {
                            account.request(
                                Action::Password,
                                json!({"current":account.current_password,"password":account.new_password}),
                            );
                        }
                    });
                });

                ui.add_space(12.);
                ui.separator();
                ui.add_space(10.);
                ui.label(RichText::new("Google").bold());
                if account.profile["has_google"] == true {
                    ui.label(RichText::new("You can sign in with Google.").color(green));
                } else {
                    muted(ui, "Link Google to sign in with it, and to use your YouTube likes and playlists.");
                    if ui.button("Link Google").clicked() {
                        account.connect(Provider::Google);
                    }
                }
                if !account.status.is_empty() {
                    ui.add_space(8.);
                    let color = if account.error { red } else { MUTED };
                    ui.add(egui::Label::new(RichText::new(&account.status).color(color)).wrap());
                }
                });
                ui.add_space(12.);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("Done").frame(false)).clicked() {
                        close = true;
                    }
                });
            });
        if close || modal.should_close() {
            account.security_open = false;
            (account.current_password, account.new_password, account.confirm_password) = Default::default();
        }
    }

    fn profile_dialog(&mut self, ctx: &egui::Context) {
        let account = &mut self.roy_account;
        let mut close = false;
        let mut open_security = false;
        let frame = egui::Frame::popup(&ctx.style())
            .inner_margin(24)
            .corner_radius(12);
        let modal = egui::Modal::new(egui::Id::new("edit-profile"))
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_width(440.);
                ui.label(RichText::new("Profile details").size(24.).bold());
                ui.add_space(18.);
                ui.add_enabled_ui(account.pending.is_none(), |ui| {
                    let (cover, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 110.), egui::Sense::click());
                    ui.painter().rect_filled(cover, 10, tint_for(account.name()));
                    if let Some(uri) = account.banner_uri() {
                        App::paint_cover(ui, cover, &uri, 10);
                    }
                    let hover = ui.ctx().animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.15);
                    ui.painter().rect_filled(cover, 10, Color32::from_black_alpha((120. * hover) as u8));
                    ui.painter().text(
                        cover.center(),
                        egui::Align2::CENTER_CENTER,
                        "Change cover",
                        egui::FontId::proportional(15.),
                        Color32::from_white_alpha((255. * hover) as u8),
                    );
                    if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        account.choose_banner();
                    }
                    if account.banner_uri().is_some() && ui.small_button("Remove cover").clicked() {
                        account.request(Action::BannerRemove, Value::Null);
                    }
                    ui.add_space(8.);
                    ui.horizontal(|ui| {
                        if avatar(ui, account, 128., true).clicked() {
                            account.choose_avatar();
                        }
                        ui.add_space(8.);
                        ui.vertical(|ui| {
                            ui.add_space(22.);
                            if ui.button("Choose photo").clicked() {
                                account.choose_avatar();
                            }
                            if account.avatar_uri().is_some() && ui.button("Remove photo").clicked()
                            {
                                account.request(Action::AvatarRemove, Value::Null);
                            }
                            muted(ui, "JPEG, PNG or WebP");
                        });
                    });
                    ui.add_space(12.);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("@{}", account.username())).color(MUTED));
                        if ui.small_button("Username & password").clicked() {
                            open_security = true;
                        }
                    });
                    ui.add_space(4.);
                    field(ui, "Name", &mut account.display_name, false, 48);
                    ui.label(RichText::new("Bio").bold());
                    ui.add(
                        egui::TextEdit::multiline(&mut account.bio)
                            .char_limit(160)
                            .desired_rows(3)
                            .desired_width(f32::INFINITY)
                            .margin(egui::vec2(12., 9.)),
                    );
                    ui.label(
                        RichText::new(format!("{}/160", account.bio.chars().count()))
                            .small()
                            .color(MUTED),
                    );
                });
                if !account.status.is_empty() {
                    let color = if account.error {
                        Color32::from_rgb(244, 155, 158)
                    } else {
                        MUTED
                    };
                    ui.add(egui::Label::new(RichText::new(&account.status).color(color)).wrap());
                }
                ui.add_space(10.);
                ui.horizontal(|ui| {
                    if account.pending.is_some() {
                        ui.spinner();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let ready =
                            account.pending.is_none() && !account.display_name.trim().is_empty();
                        if ui
                            .add_enabled(
                                ready,
                                egui::Button::new(
                                    RichText::new("Save").bold().color(Color32::BLACK),
                                )
                                .fill(Color32::WHITE)
                                .corner_radius(18)
                                .min_size(egui::vec2(96., 36.)),
                            )
                            .clicked()
                        {
                            account.request(
                            Action::Save,
                            json!({"display_name":account.display_name.trim(),"bio":account.bio}),
                        );
                        }
                        if ui.add(egui::Button::new("Cancel").frame(false)).clicked() {
                            close = true;
                        }
                    });
                });
            });
        if open_security {
            account.editing = false;
            account.open_security();
        }
        if close || modal.should_close() {
            account.editing = false;
            account.sync_fields();
        }
    }

    fn roynit_sign_in(&mut self, ui: &mut egui::Ui) {
        card(ui).show(ui, |ui| {
            ui.set_max_width(475.);
            self.sign_in_body(ui, false);
        });
    }

    /// Shown over everything on a fresh install, until someone signs in; it cannot be dismissed.
    pub(super) fn roynit_gate(&mut self, ctx: &egui::Context) {
        if !self.roy_account.needs_sign_in() {
            return;
        }
        let top = self.dialog_top();
        Self::dialog(ctx, top, "gate", false, |ui| {
            ui.set_width(400.);
            self.sign_in_body(ui, true);
            let account = &self.roy_account;
            if !account.status.is_empty() {
                ui.add_space(8.);
                let color = if account.error {
                    Color32::from_rgb(244, 155, 158)
                } else {
                    MUTED
                };
                ui.add(egui::Label::new(RichText::new(&account.status).color(color)).wrap());
            }
        });
    }

    fn sign_in_body(&mut self, ui: &mut egui::Ui, gate: bool) {
        let account = &mut self.roy_account;
        ui.horizontal(|ui| {
            ui.add(
                egui::Image::new(egui::include_image!("../assets/icon.png"))
                    .fit_to_exact_size(egui::vec2(52., 52.)),
            );
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(if gate {
                        "Welcome to Roy Music"
                    } else {
                        "Roynit ID"
                    })
                    .size(25.)
                    .bold(),
                );
                muted(
                    ui,
                    if gate {
                        "Sign in to continue"
                    } else {
                        "One account for the Roynit ecosystem"
                    },
                );
            });
        });
        ui.add_space(16.);
        if account.token.is_some() && account.profile.is_null() {
            if account.pending.is_some() {
                ui.spinner();
            } else {
                if ui.button("Retry session").clicked() {
                    account.request(Action::Check, Value::Null);
                }
                if ui.button("Sign out on this device").clicked() {
                    account.clear_session();
                }
            }
            return;
        }
        if account.pickup.is_some() {
            ui.spinner();
            if ui.button("Cancel").clicked() {
                account.pickup = None;
                if account.pending.is_some() {
                    account.pending = None;
                }
                account.notice("Google sign-in cancelled.", false);
            }
            return;
        }
        ui.add_enabled_ui(account.pending.is_none(), |ui| {
                ui.horizontal(|ui| {
                    if App::filter_chip(ui, "Sign in", !account.register).clicked() {
                        account.register = false;
                        account.email.clear();
                        account.notice("",false);
                    }
                    if App::filter_chip(ui, "Create account", account.register).clicked() {
                        account.register = true;
                        account.email.clear();
                        account.notice("",false);
                    }
                });
                ui.add_space(10.);
                if account.register {
                    field(ui,"Email",&mut account.email,false,254);
                    field(ui,"Username",&mut account.username,false,24);
                } else {
                    field(ui,"Email or username",&mut account.email,false,254);
                }
                field(ui,"Password",&mut account.password,true,128);
                if account.register {
                    muted(ui, "Use at least 6 characters.");
                }
                ui.add_space(8.);
                ui.horizontal_wrapped(|ui| {
                    let label = if account.register {"Create account"} else {"Sign in"};
                    if primary(ui,label).clicked() {
                        let password = std::mem::take(&mut account.password);
                        if account.register {
                            account.request(Action::Register,json!({"email":account.email,"username":account.username,"password":password}));
                        } else {
                            account.request(Action::SignIn,json!({"login":account.email,"password":password}));
                        }
                        account.notice("",false);
                    }
                    if ui.button("Continue with Google").clicked() {
                        account.password.clear();
                        account.request(Action::Google, Value::Null);
                        account.notice("",false);
                    }
                });
            });
        if account.pending.is_some() {
            ui.spinner();
        }
    }

    fn roynit_dashboard(&mut self, ui: &mut egui::Ui) {
        let account = &mut self.roy_account;
        let text = |key: &str, fallback: &str| {
            account.profile[key].as_str().unwrap_or(fallback).to_owned()
        };
        let name = text("display_name", "Listener");
        let bio = text("bio", "");
        let username = text("username", "listener");
        let email = text("email", "");
        let provider = if account.profile["provider"] == "google" {
            "Google"
        } else {
            "Password"
        };
        let joined = account.profile["created_at"]
            .as_i64()
            .map(month_year)
            .unwrap_or_default();
        let mut activity = std::collections::HashMap::<&str, usize>::new();
        for event in self.library.history.iter().rev().take(250) {
            if let Some(track) = self.library.tracks.get(&event.track_id)
                && !track.artist.trim().is_empty()
            {
                *activity.entry(&track.artist).or_default() += 1;
            }
        }
        let mut artists: Vec<_> = activity.into_iter().collect();
        artists.sort_by(|(a, left), (b, right)| right.cmp(left).then(a.cmp(b)));

        ui.horizontal(|ui| {
            if avatar(ui, account, 176., true).clicked() {
                account.editing = true;
            }
            ui.add_space(12.);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                ui.add_space(24.);
                ui.label(RichText::new("Profile").size(13.).color(TEXT));
                ui.add(egui::Label::new(RichText::new(&name).size(54.).bold()).truncate());
                if !bio.is_empty() {
                    ui.add(egui::Label::new(RichText::new(&bio).color(MUTED)).wrap());
                }
                ui.label(
                    RichText::new(format!(
                        "{} plays  ·  {} liked  ·  {} playlists",
                        self.library.history.len(),
                        self.library.favorites.len(),
                        self.library.playlists.len()
                    ))
                    .color(MUTED),
                );
            });
        });
        let mut connected = false;
        ui.add_space(18.);
        ui.horizontal(|ui| {
            if outline_button(ui, "Edit profile").clicked() {
                account.editing = true;
            }
            ui.add_enabled_ui(account.pending.is_none(), |ui| {
                if outline_button(ui, "Sign out").clicked() {
                    account.automatic = false;
                    account.request(Action::SignOut, Value::Null);
                }
            });
        });
        ui.add_space(26.);
        ui.scope(|ui| {
            ui.set_max_width(640.);
            services_card(ui, account);
        });
        if !artists.is_empty() {
            ui.add_space(26.);
            ui.label(RichText::new("Top artists lately").size(22.).bold());
            ui.add_space(8.);
            ui.horizontal_wrapped(|ui| {
                for (artist, count) in artists.iter().take(6) {
                    ui.vertical(|ui| {
                        ui.set_width(128.);
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(120., 120.), egui::Sense::hover());
                        ui.painter()
                            .circle_filled(rect.center(), 60., tint_for(artist));
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            initials(artist),
                            egui::FontId::proportional(40.),
                            Color32::from_white_alpha(220),
                        );
                        ui.add(egui::Label::new(RichText::new(*artist).bold()).truncate());
                        ui.label(
                            RichText::new(format!(
                                "{count} {}",
                                if *count == 1 { "play" } else { "plays" }
                            ))
                            .small()
                            .color(MUTED),
                        );
                    });
                    ui.add_space(8.);
                }
            });
        }
        ui.add_space(26.);
        let mut merge = None;
        ui.scope(|ui| {
            ui.set_max_width(640.);
            card(ui).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Music sync").size(19.).bold());
                muted(ui,"Sync online tracks from listening history, liked songs and playlists. History helps shape your mixes.");
                ui.add_enabled_ui(account.pending.is_none(), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if account.cloud["revision"].as_u64().is_some_and(|revision| revision > 0) {
                            if let Some(snapshot) = account.cloud.get("snapshot") {
                                if ui.button("Merge cloud library").clicked() {
                                    merge = Some(snapshot.clone());
                                }
                            } else if ui.button("Refresh cloud library").clicked() {
                                account.request(Action::ReadMusic,Value::Null);
                            }
                        } else if account.revision.is_none() && ui.button("Check cloud library").clicked() {
                            account.request(Action::ReadMusic,Value::Null);
                        }
                        if account.revision.is_some() && ui.button("Sync this device").clicked() {
                            account.upload(&self.library,true);
                        }
                    });
                    if account.revision.is_some() {
                        ui.checkbox(&mut account.automatic, "Auto-sync on this device for this session");
                    }
                });
                if account.pending.is_some() { ui.spinner(); }
            });
            ui.add_space(12.);
            card(ui).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Account").size(19.).bold());
                ui.add_space(4.);
                egui::Grid::new("account-details").num_columns(2).spacing([24., 6.]).show(ui, |ui| {
                    for (label, value) in [("Email", &email), ("Username", &format!("@{username}")), ("Sign-in", &provider.to_owned()), ("Member since", &joined)] {
                        ui.label(RichText::new(label).color(MUTED));
                        ui.add(egui::Label::new(value.as_str()).truncate());
                        ui.end_row();
                    }
                });
                ui.add_space(8.);
                if ui.button("Music connections").clicked() {
                    connected = true;
                }
            });
        });
        if let Some(snapshot) = merge {
            self.send(
                "account-merged",
                "account.music.merge",
                json!({"snapshot":snapshot}),
            );
        }
        if connected {
            self.settings_group = 3;
            self.settings_query.clear();
            self.open_settings();
        }
    }

    pub(super) fn roynit_merge_complete(&mut self) {
        self.roy_account.revision = self.roy_account.cloud["revision"].as_u64();
        self.roy_account
            .notice("Cloud music merged into this device.", false);
        self.reset_mixes();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_year_matches_known_dates() {
        assert_eq!(month_year(0), "Jan 1970");
        assert_eq!(month_year(1_795_364_123), "Nov 2026");
        assert_eq!(month_year(1_709_251_199), "Feb 2024");
    }

    #[test]
    fn avatar_is_cropped_square_and_small() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wide.png");
        image::RgbImage::from_fn(1200, 500, |x, _| image::Rgb([(x / 5) as u8, 90, 200]))
            .save(&path)
            .unwrap();
        let bytes = prepare_avatar(&path).unwrap();
        assert!(bytes.starts_with(&[0xff, 0xd8, 0xff]) && bytes.len() < 150_000);
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (256, 256));
        std::fs::write(dir.path().join("x.txt"), "no").unwrap();
        assert!(prepare_avatar(&dir.path().join("x.txt")).is_err());
    }
}

#[cfg(test)]
mod banner_tests {
    use super::prepare_banner;

    #[test]
    fn any_picture_becomes_a_small_wide_cover() {
        let path = std::env::temp_dir().join(format!("roy-banner-{}.png", std::process::id()));
        for (w, h) in [(1000, 1000), (3000, 500), (200, 900)] {
            image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x % 251) as u8, (y % 241) as u8, 90]))
                .save(&path)
                .unwrap();
            let bytes = prepare_banner(&path).unwrap();
            let cover = image::load_from_memory(&bytes).unwrap();
            assert_eq!((cover.width(), cover.height()), (960, 320), "{w}x{h}");
            assert!(bytes.len() < 380_000);
        }
        let _ = std::fs::remove_file(&path);
    }
}
