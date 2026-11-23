//! Friends, what each friend is listening to, and direct messages, all kept on the Roynit ID server.
//! Everything runs on short-lived worker threads; the UI only reads the latest snapshot.
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub struct Person {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub bio: String,
    pub avatar_version: u64,
    pub banner_version: u64,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub struct Listening {
    pub title: String,
    pub artist: String,
    pub cover: Option<String>,
    pub duration: f64,
    pub position: f64,
    pub playing: bool,
    pub at: i64,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub struct Friend {
    #[serde(flatten)]
    pub person: Person,
    pub online: bool,
    pub listening: Option<Listening>,
    pub last_seen: Option<i64>,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub struct Message {
    pub id: i64,
    pub mine: bool,
    pub body: String,
    pub at: i64,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Snapshot {
    now: i64,
    friends: Vec<Friend>,
    incoming: Vec<Person>,
    outgoing: Vec<Person>,
    unread: HashMap<String, u32>,
}

#[derive(Default)]
pub struct Chat {
    pub messages: Vec<Message>,
    pub draft: String,
    /// Scroll to the newest message on the next frame.
    pub stick: bool,
    loaded: bool,
}

/// The song this person is playing right now, as it is shared with friends.
#[derive(Clone, PartialEq, Default)]
pub struct Playing {
    pub title: String,
    pub artist: String,
    pub cover: Option<String>,
    pub duration: f64,
    pub playing: bool,
}

enum Tag {
    Social,
    Presence,
    Chat(String),
    Send(String),
    Act(String),
}

type Reply = Result<Value, String>;

pub struct Social {
    client: Client,
    endpoint: String,
    token: Option<String>,
    calls: Vec<(Tag, mpsc::Receiver<Reply>)>,
    pub friends: Vec<Friend>,
    pub incoming: Vec<Person>,
    pub outgoing: Vec<Person>,
    pub unread: HashMap<String, u32>,
    pub chats: HashMap<String, Chat>,
    /// Seconds the server clock was ahead of the sender's last heartbeat when the snapshot arrived.
    server_now: i64,
    snapshot_at: Instant,
    pub loaded: bool,
    pub notice: Option<(String, bool)>,
    pub invisible: bool,
    next_social: Instant,
    next_presence: Instant,
    next_chat: Instant,
    shared: Option<Playing>,
    shared_position: f64,
    send_presence: bool,
}

impl Social {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(12))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("friends HTTP client");
        let now = Instant::now();
        Self {
            client,
            endpoint: String::new(),
            token: None,
            calls: Vec::new(),
            friends: Vec::new(),
            incoming: Vec::new(),
            outgoing: Vec::new(),
            unread: HashMap::new(),
            chats: HashMap::new(),
            server_now: 0,
            snapshot_at: now,
            loaded: false,
            notice: None,
            invisible: false,
            next_social: now,
            next_presence: now,
            next_chat: now,
            shared: None,
            shared_position: 0.,
            send_presence: true,
        }
    }

    /// Follows the signed-in account; a different (or no) session clears everything from the old one.
    pub fn follow(&mut self, endpoint: &str, token: Option<&str>) {
        if self.token.as_deref() == token && self.endpoint == endpoint {
            return;
        }
        let client = self.client.clone();
        *self = Self::new();
        self.client = client;
        self.endpoint = endpoint.to_owned();
        self.token = token.map(str::to_owned);
    }

    pub fn signed_in(&self) -> bool {
        self.token.is_some()
    }

    pub fn avatar_uri(&self, person: &Person) -> Option<String> {
        (person.avatar_version > 0)
            .then(|| format!("{}/avatar/{}?v={}", self.endpoint, person.id, person.avatar_version))
    }

    pub fn banner_uri(&self, person: &Person) -> Option<String> {
        (person.banner_version > 0)
            .then(|| format!("{}/banner/{}?v={}", self.endpoint, person.id, person.banner_version))
    }

    pub fn requests(&self) -> usize {
        self.incoming.len()
    }

    pub fn unread_total(&self) -> u32 {
        self.unread.values().sum()
    }

    pub fn friend(&self, id: &str) -> Option<&Friend> {
        self.friends.iter().find(|f| f.person.id == id)
    }

    /// Seconds into the song a friend is on now, moving forward while it plays.
    pub fn position_of(&self, listening: &Listening) -> f64 {
        if !listening.playing {
            return listening.position;
        }
        let behind = (self.server_now - listening.at).max(0) as f64 + self.snapshot_at.elapsed().as_secs_f64();
        (listening.position + behind).min(listening.duration.max(listening.position))
    }

    fn call(&mut self, tag: Tag, method: reqwest::Method, path: String, body: Option<Value>) {
        let Some(token) = self.token.clone() else {
            return;
        };
        let (client, address) = (self.client.clone(), format!("{}{path}", self.endpoint));
        let (tx, rx) = mpsc::channel();
        self.calls.push((tag, rx));
        std::thread::spawn(move || {
            let result = (|| {
                let mut request = client.request(method, address).bearer_auth(token);
                if let Some(body) = body {
                    request = request.json(&body);
                }
                let response = request.send().map_err(|_| "Could not reach Roynit ID.".to_owned())?;
                let status = response.status();
                let data: Value = response
                    .json()
                    .map_err(|_| "Roynit ID is unavailable right now.".to_owned())?;
                if status.is_success() {
                    Ok(data)
                } else {
                    Err(data["error"].as_str().unwrap_or("That did not work.").to_owned())
                }
            })();
            let _ = tx.send(result);
        });
    }

    pub fn add_friend(&mut self, username: &str) {
        let body = json!({"username":username});
        self.call(Tag::Act("request".into()), reqwest::Method::POST, "/friends/request".into(), Some(body));
    }

    pub fn answer(&mut self, id: &str, accept: bool) {
        let path = if accept { "/friends/accept" } else { "/friends/decline" };
        let note = if accept { "accepted" } else { "declined" };
        self.call(Tag::Act(note.into()), reqwest::Method::POST, path.into(), Some(json!({"id":id})));
    }

    pub fn remove(&mut self, id: &str) {
        self.chats.remove(id);
        self.call(Tag::Act("removed".into()), reqwest::Method::DELETE, format!("/friends/{id}"), None);
    }

    pub fn send(&mut self, id: &str) {
        let Some(chat) = self.chats.get_mut(id) else {
            return;
        };
        let body = chat.draft.trim().to_owned();
        if body.is_empty() {
            return;
        }
        chat.draft.clear();
        self.call(Tag::Send(id.to_owned()), reqwest::Method::POST, format!("/messages/{id}"), Some(json!({"body":body})));
    }

    pub fn open_chat(&mut self, id: &str) {
        let chat = self.chats.entry(id.to_owned()).or_default();
        chat.stick = true;
        self.next_chat = Instant::now();
    }

    pub fn set_invisible(&mut self, invisible: bool) {
        self.invisible = invisible;
        self.send_presence = true;
    }

    /// Once a frame: collect replies, then start whichever refreshes are due.
    /// `watching` is true while the Friends page is open; `open` is the chat on screen.
    pub fn tick(&mut self, watching: bool, open: Option<&str>, now_playing: Option<(Playing, f64)>) {
        if self.token.is_none() {
            return;
        }
        self.drain();
        let now = Instant::now();
        let busy = |calls: &[(Tag, mpsc::Receiver<Reply>)], f: fn(&Tag) -> bool| calls.iter().any(|(t, _)| f(t));
        if now >= self.next_social && !busy(&self.calls, |t| matches!(t, Tag::Social)) {
            self.next_social = now + Duration::from_secs(if watching { 4 } else { 20 });
            self.call(Tag::Social, reqwest::Method::GET, "/social".into(), None);
        }
        if let Some(id) = open.map(str::to_owned)
            && now >= self.next_chat
            && !busy(&self.calls, |t| matches!(t, Tag::Chat(_)))
        {
            self.next_chat = now + Duration::from_millis(2500);
            let chat = self.chats.entry(id.clone()).or_default();
            let after = if chat.loaded { chat.messages.last().map_or(0, |m| m.id) } else { 0 };
            self.call(
                Tag::Chat(id.clone()),
                reqwest::Method::GET,
                format!("/messages/{id}?after={after}&read=1"),
                None,
            );
        }
        let (playing, position) = now_playing.map_or((None, 0.), |(p, pos)| (Some(p), pos));
        // Heartbeat: right away when the song or its play/pause state changes, otherwise every 25 s.
        if playing != self.shared {
            self.send_presence = true;
        }
        if (self.send_presence || now >= self.next_presence)
            && !busy(&self.calls, |t| matches!(t, Tag::Presence))
        {
            self.send_presence = false;
            self.next_presence = now + Duration::from_secs(25);
            self.shared = playing.clone();
            self.shared_position = position;
            let listening = playing.map(|p| {
                json!({"title":p.title,"artist":p.artist,"cover":p.cover,"duration":p.duration,
                    "position":position,"playing":p.playing})
            });
            let body = json!({"invisible":self.invisible,"listening":listening});
            self.call(Tag::Presence, reqwest::Method::PUT, "/presence".into(), Some(body));
        }
    }

    fn drain(&mut self) {
        let mut finished = Vec::new();
        let mut index = 0;
        while index < self.calls.len() {
            match self.calls[index].1.try_recv() {
                Ok(result) => {
                    let (tag, _) = self.calls.remove(index);
                    finished.push((tag, result));
                }
                Err(mpsc::TryRecvError::Empty) => index += 1,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.calls.remove(index);
                }
            }
        }
        for (tag, result) in finished {
            self.apply(tag, result);
        }
    }

    fn apply(&mut self, tag: Tag, result: Reply) {
        match (tag, result) {
            (Tag::Social, Ok(data)) => {
                if let Ok(snapshot) = serde_json::from_value::<Snapshot>(data) {
                    (self.friends, self.incoming, self.outgoing, self.unread) =
                        (snapshot.friends, snapshot.incoming, snapshot.outgoing, snapshot.unread);
                    self.server_now = snapshot.now;
                    self.snapshot_at = Instant::now();
                    self.loaded = true;
                }
            }
            (Tag::Chat(id), Ok(data)) => {
                let fresh: Vec<Message> =
                    serde_json::from_value(data["messages"].clone()).unwrap_or_default();
                let chat = self.chats.entry(id.clone()).or_default();
                let last = chat.messages.last().map_or(0, |m| m.id);
                let any = fresh.iter().any(|m| m.id > last);
                chat.messages.extend(fresh.into_iter().filter(|m| m.id > last));
                chat.loaded = true;
                chat.stick |= any;
                if any {
                    self.unread.remove(&id);
                }
            }
            (Tag::Send(id), Ok(data)) => {
                if let Ok(message) = serde_json::from_value::<Message>(data) {
                    let chat = self.chats.entry(id).or_default();
                    if chat.messages.last().is_none_or(|m| m.id < message.id) {
                        chat.messages.push(message);
                    }
                    chat.stick = true;
                }
            }
            (Tag::Act(what), Ok(data)) => {
                let name = data["name"].as_str().unwrap_or("them");
                let text = match (what.as_str(), data["accepted"].as_bool()) {
                    ("request", Some(true)) => format!("You and {name} are now friends."),
                    ("request", _) => format!("Friend request sent to {name}."),
                    ("accepted", _) => "Friend added.".to_owned(),
                    ("declined", _) => "Request removed.".to_owned(),
                    _ => "Friend removed.".to_owned(),
                };
                self.notice = Some((text, false));
                self.next_social = Instant::now();
            }
            (Tag::Send(_) | Tag::Act(_), Err(error)) => self.notice = Some((error, true)),
            // A refresh that fails just waits for the next one.
            (Tag::Social | Tag::Chat(_) | Tag::Presence, _) => {}
        }
    }
}
