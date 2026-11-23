use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Mutex, OnceLock, mpsc},
    time::{Duration, Instant},
};

#[cfg(test)]
mod tests;

/// googlevideo refuses a whole-file request ("Range: bytes=0-") on the larger tracks from the iOS client
/// with 403, but serves bounded ranges. FFmpeg always asks for the whole file, so playback goes through a
/// small loopback server that turns its requests into bounded ranges.
struct Upstream {
    url: String,
    agent: String,
    length: u64,
    at: Instant,
}

fn relay_table() -> &'static Mutex<HashMap<String, Upstream>> {
    static TABLE: OnceLock<Mutex<HashMap<String, Upstream>>> = OnceLock::new();
    TABLE.get_or_init(Default::default)
}

fn relay_port() -> Option<u16> {
    static PORT: OnceLock<Option<u16>> = OnceLock::new();
    *PORT.get_or_init(|| {
        let listener = TcpListener::bind(("127.0.0.1", 0)).ok()?;
        let port = listener.local_addr().ok()?.port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                std::thread::spawn(move || {
                    let _ = serve(stream);
                });
            }
        });
        Some(port)
    })
}

/// A loopback address FFmpeg can read; the secret path keeps other local programs from using it.
fn relay(url: &str, agent: &str) -> Option<String> {
    let length = url::Url::parse(url)
        .ok()?
        .query_pairs()
        .find(|(key, _)| key == "clen")?
        .1
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)?;
    let port = relay_port()?;
    let token = format!("{:032x}", rand::random::<u128>());
    let mut table = relay_table().lock().ok()?;
    table.retain(|_, entry| entry.at.elapsed() < CACHE_TTL + Duration::from_secs(600));
    table.insert(
        token.clone(),
        Upstream {
            url: url.to_owned(),
            agent: agent.to_owned(),
            length,
            at: Instant::now(),
        },
    );
    Some(format!("http://127.0.0.1:{port}/{token}"))
}

fn serve(mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request = String::new();
    reader.read_line(&mut request)?;
    let (mut method, mut path) = (request.split(' ').next().unwrap_or(""), "");
    if let Some(p) = request.split(' ').nth(1) {
        path = p;
    }
    let mut range: Option<(u64, Option<u64>)> = None;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line.trim().is_empty() {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("range:")
            && let Some(spec) = value.trim().strip_prefix("bytes=")
            && let Some((a, b)) = spec.split_once('-')
        {
            range = a.trim().parse().ok().map(|a| (a, b.trim().parse().ok()));
        }
    }
    if method != "GET" && method != "HEAD" {
        method = "";
    }
    let entry = {
        let table = relay_table().lock().map_err(|_| std::io::ErrorKind::Other)?;
        table
            .get(path.trim_start_matches('/'))
            .map(|e| (e.url.clone(), e.agent.clone(), e.length))
    };
    // HTTP header lines end in CR LF; building them from parts keeps that visible.
    let reply = |lines: &[String]| lines.join("\r\n") + "\r\n\r\n";
    let (Some((url, agent, length)), false) = (entry, method.is_empty()) else {
        return stream.write_all(
            reply(&["HTTP/1.1 404 Not Found".into(), "Content-Length: 0".into(), "Connection: close".into()])
                .as_bytes(),
        );
    };
    let (start, end) = match range {
        Some((a, b)) => (a.min(length), b.unwrap_or(length - 1).min(length - 1)),
        None => (0, length - 1),
    };
    if start > end {
        return stream.write_all(
            reply(&[
                "HTTP/1.1 416 Range Not Satisfiable".into(),
                format!("Content-Range: bytes */{length}"),
                "Content-Length: 0".into(),
                "Connection: close".into(),
            ])
            .as_bytes(),
        );
    }
    let mut lines = vec![if range.is_some() { "HTTP/1.1 206 Partial Content" } else { "HTTP/1.1 200 OK" }.to_owned()];
    if range.is_some() {
        lines.push(format!("Content-Range: bytes {start}-{end}/{length}"));
    }
    lines.extend([
        "Content-Type: audio/mp4".into(),
        format!("Content-Length: {}", end - start + 1),
        "Accept-Ranges: bytes".into(),
        "Connection: close".into(),
    ]);
    stream.write_all(reply(&lines).as_bytes())?;
    if method == "HEAD" {
        return Ok(());
    }
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| std::io::ErrorKind::Other)?;
    // A small first chunk gets sound out quickly; later chunks are larger to keep the request count low.
    let mut at = start;
    let mut size = 256 * 1024;
    while at <= end {
        let to = (at + size - 1).min(end);
        let response = client
            .get(&url)
            .header("User-Agent", &agent)
            .header("Range", format!("bytes={at}-{to}"))
            .send()
            .map_err(|_| std::io::ErrorKind::ConnectionAborted)?;
        if response.status().as_u16() != 206 {
            return Err(std::io::ErrorKind::ConnectionAborted.into());
        }
        let mut body = response.take(to - at + 1);
        let copied = std::io::copy(&mut body, &mut stream)?;
        if copied == 0 {
            break;
        }
        at += copied;
        size = 1024 * 1024;
    }
    Ok(())
}

/// Stream links stay valid for hours; twenty minutes is plenty and keeps a "warmed" track instant to start.
const CACHE_TTL: Duration = Duration::from_secs(20 * 60);

fn cache() -> &'static Mutex<HashMap<String, (Instant, Value)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (Instant, Value)>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn cached(id: &str) -> Option<Value> {
    let mut cache = cache().lock().ok()?;
    cache.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
    cache.get(id).map(|(_, value)| value.clone())
}

fn client() -> Option<&'static reqwest::blocking::Client> {
    static CLIENT: OnceLock<Option<reqwest::blocking::Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::blocking::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                // The first request also pays for DNS and TLS, so it gets more room than a warm one.
                .timeout(Duration::from_millis(2500))
                .build()
                .ok()
        })
        .as_ref()
}

/// Open the connections playback will need (DNS, TLS) before anyone presses Play.
pub(crate) fn preconnect() {
    if let Some(client) = client() {
        std::thread::scope(|scope| {
            for url in ["https://www.youtube.com/", "https://music.youtube.com/"] {
                scope.spawn(move || {
                    let _ = client.head(url).send();
                });
            }
        });
    }
}

/// Resolve stream links ahead of time (three at a time) so Play on these tracks starts at once.
pub(crate) fn warm(sources: Vec<String>) {
    std::thread::spawn(move || {
        let mut ids: Vec<String> = sources.iter().filter_map(|s| video_id(s)).collect();
        ids.dedup();
        ids.retain(|id| cached(id).is_none());
        for chunk in ids.chunks(3) {
            std::thread::scope(|scope| {
                for id in chunk {
                    scope.spawn(move || {
                        let _ = resolve_id(id);
                    });
                }
            });
        }
    });
}

struct Profile {
    endpoint: &'static str,
    agent: &'static str,
    context: fn() -> Value,
}

const PROFILES: [Profile; 2] = [
    Profile {
        endpoint: "https://www.youtube.com/youtubei/v1/player",
        agent: "com.google.ios.youtube/20.10.4 (iPhone16,2; U; CPU iOS 18_3_2 like Mac OS X;)",
        context: || json!({"clientName":"IOS","clientVersion":"20.10.4","deviceMake":"Apple","deviceModel":"iPhone16,2","osName":"iOS","osVersion":"18.3.2.22D60","hl":"en","gl":"US"}),
    },
    Profile {
        endpoint: "https://music.youtube.com/youtubei/v1/player",
        agent: "com.google.android.apps.youtube.music/7.27.52 (Linux; U; Android 14; en_US; Quest 3) gzip",
        context: || json!({"clientName":"ANDROID_MUSIC","clientVersion":"7.27.52","androidSdkVersion":34,"hl":"en","gl":"US"}),
    },
];

pub(crate) fn video_id(source: &str) -> Option<String> {
    let source = url::Url::parse(source).ok()?;
    let id = match source.host_str()? {
        "youtube.com" | "www.youtube.com" | "music.youtube.com" if source.path() == "/watch" => {
            source
                .query_pairs()
                .find(|(key, _)| key == "v")?
                .1
                .into_owned()
        }
        "youtu.be" => source.path().strip_prefix('/')?.to_owned(),
        _ => return None,
    };
    crate::youtube::validate_video(&id).ok()?;
    Some(id)
}

pub(crate) fn resolve(source: &str) -> Option<Value> {
    resolve_id(&video_id(source)?)
}

fn resolve_id(id: &str) -> Option<Value> {
    if let Some(hit) = cached(id) {
        return Some(hit);
    }
    let client = client()?;
    let (tx, rx) = mpsc::channel();
    for profile in PROFILES {
        let tx = tx.clone();
        let id = id.to_owned();
        std::thread::spawn(move || {
            let _ = tx.send(query(client, profile, &id));
        });
    }
    drop(tx);
    let deadline = Instant::now() + Duration::from_millis(3000);
    while let Ok(result) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        crate::jobs::check_cancelled().ok()?;
        if let Some(stream) = result {
            if let Ok(mut cache) = cache().lock() {
                cache.insert(id.to_owned(), (Instant::now(), stream.clone()));
            }
            return Some(stream);
        }
    }
    None
}

fn query(client: &reqwest::blocking::Client, profile: Profile, id: &str) -> Option<Value> {
    let response = client
        .post(profile.endpoint)
        .query(&[("prettyPrint", "false")])
        .header("User-Agent", profile.agent)
        .header("X-Goog-Api-Format-Version", "2")
        .json(&json!({"videoId":id,"context":{"client":(profile.context)()}}))
        .send()
        .ok()?
        .error_for_status()
        .ok()?
        .json::<Value>()
        .ok()?;
    if response["playabilityStatus"]["status"] != "OK" || response["videoDetails"]["videoId"] != id
    {
        return None;
    }
    let duration = response["videoDetails"]["lengthSeconds"]
        .as_str()?
        .parse::<u32>()
        .ok()
        .filter(|s| *s > 0)?;
    let formats = response["streamingData"]["adaptiveFormats"].as_array()?;
    let stream = formats
        .iter()
        .filter_map(|format| {
            let mime = format["mimeType"].as_str()?;
            let direct = format["url"].as_str()?;
            if !mime.starts_with("audio/") || !is_media_url(direct) {
                return None;
            }
            Some((
                mime.starts_with("audio/mp4"),
                format["bitrate"].as_u64().unwrap_or(0),
                direct,
                mime,
            ))
        })
        .max_by_key(|(mp4, bitrate, _, _)| (*mp4, *bitrate))?;
    let probe = client
        .get(stream.2)
        .header("User-Agent", profile.agent)
        .header("Range", "bytes=0-1")
        .send()
        .ok()?;
    if !probe.status().is_success() {
        return None;
    }
    let url = relay(stream.2, profile.agent).unwrap_or_else(|| stream.2.to_owned());
    Some(
        json!({"url":url,"headers":{"User-Agent":profile.agent},"duration":duration,"format":if stream.0 {"m4a"} else {"webm"},"resolver":"direct_youtube"}),
    )
}

fn is_media_url(value: &str) -> bool {
    url::Url::parse(value).ok().is_some_and(|url| {
        url.scheme() == "https"
            && url.port().is_none()
            && url.username().is_empty()
            && url.password().is_none()
            && url
                .host_str()
                .is_some_and(|host| host == "googlevideo.com" || host.ends_with(".googlevideo.com"))
    })
}
