use serde_json::{Value, json};
use std::{
    sync::{OnceLock, mpsc},
    time::Duration,
};

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
    let id = video_id(source)?;
    static CLIENT: OnceLock<Option<reqwest::blocking::Client>> = OnceLock::new();
    let client = CLIENT
        .get_or_init(|| {
            reqwest::blocking::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_millis(1100))
                .build()
                .ok()
        })
        .as_ref()?;
    let (tx, rx) = mpsc::channel();
    for profile in PROFILES {
        let tx = tx.clone();
        let id = id.clone();
        std::thread::spawn(move || {
            let _ = tx.send(query(client, profile, &id));
        });
    }
    drop(tx);
    let deadline = std::time::Instant::now() + Duration::from_millis(1500);
    while let Ok(result) =
        rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
    {
        crate::jobs::check_cancelled().ok()?;
        if result.is_some() {
            return result;
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
    Some(
        json!({"url":stream.2,"headers":{"User-Agent":profile.agent},"duration":duration,"format":if stream.0 {"m4a"} else {"webm"},"resolver":"direct_youtube"}),
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
