use riff_core::{model::Track, provider::Providers, youtube::Music};
use serde_json::json;
fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("track-list JSON path");
    let text = std::fs::read_to_string(path)?;
    let tracks: Vec<Track> = serde_json::from_str(text.trim_start_matches('\u{feff}'))?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let api = Music {
        client: &client,
        cookies: None,
    };
    let providers = Providers {
        cookies: None,
        proxy: None,
        browser: None,
    };
    for track in tracks {
        let found = api.search(&format!("{} {}", track.artist, track.title))?;
        let candidates: Vec<Track> = serde_json::from_value(found["tracks"].clone())?;
        println!(
            "{}",
            json!({"requested":track.title,"artist":track.artist,"duration":track.duration,"candidates":candidates.iter().map(|t|json!({"title":t.title,"artist":t.artist,"duration":t.duration,"source":t.source})).collect::<Vec<_>>()})
        );
        match providers.resolve(&track) {
            Ok(actual) => println!(
                "{}",
                json!({"selected":actual["matched_title"],"source":actual["playback_source"],"actual_duration":actual["duration"],"requested_duration":track.duration,"verified":actual["verified_duration"]})
            ),
            Err(e) => println!("{}", json!({"refused":track.title,"reason":e.to_string()})),
        }
    }
    Ok(())
}
