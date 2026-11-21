use riff_core::youtube::{Music, parse_tracks};
use serde_json::json;

#[test]
fn radio_parser_preserves_provider_order_and_deduplicates() {
    let v = json!({"contents":[
        {"playlistPanelVideoRenderer":{"videoId":"abcdefghijk","title":{"runs":[{"text":"First"}]},"lengthText":{"simpleText":"3:12"},"longBylineText":{"runs":[{"text":"Artist","navigationEndpoint":{"browseEndpoint":{"browseId":"UC123"}}}]}}},
        {"playlistPanelVideoRenderer":{"videoId":"123456789ab","title":{"simpleText":"Second"}}},
        {"playlistPanelVideoRenderer":{"videoId":"abcdefghijk","title":{"simpleText":"Duplicate"}}}
    ]});
    let tracks = parse_tracks(&v);
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0].title, "First");
    assert_eq!(tracks[1].title, "Second");
    assert_eq!(tracks[0].duration, 192.);
    assert_eq!(tracks[0].artist, "Artist");
}

#[test]
#[ignore = "Calls live YouTube Music radio, search and browse; anonymous session"]
fn real_youtube_music_radio_search_and_home() {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("Mozilla/5.0")
        .build()
        .unwrap();
    let api = Music {
        client: &client,
        cookies: None,
    };
    let radio = api.radio("dQw4w9WgXcQ", 20, None).unwrap();
    let tracks = radio["tracks"].as_array().unwrap();
    assert!(!tracks.is_empty());
    assert_eq!(radio["source"], "youtube_music_radio");
    assert_eq!(radio["provider_order"], true);
    println!(
        "Radio: {} tracks; first: {}",
        tracks.len(),
        tracks[0]["title"]
    );
    let search = api.search("Rick Astley Never Gonna Give You Up").unwrap();
    assert!(!search["tracks"].as_array().unwrap().is_empty());
    let home = api.home().unwrap();
    println!(
        "Home: {} tracks, {} collections",
        home["tracks"].as_array().unwrap().len(),
        home["items"].as_array().unwrap().len()
    );
    assert!(home["tracks"].as_array().unwrap().len() + home["items"].as_array().unwrap().len() > 0);
    let albums = api
        .search_filtered("Daft Punk Random Access Memories", "albums")
        .unwrap();
    let album = albums["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["kind"] == "album")
        .expect("No album search result");
    let page = api
        .browse(album["id"].as_str().unwrap(), album["params"].as_str())
        .unwrap();
    assert!(!page["tracks"].as_array().unwrap().is_empty());
    let artists = api.search_filtered("Daft Punk", "artists").unwrap();
    assert!(
        artists["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["kind"] == "artist")
    );
}

#[test]
#[ignore = "Builds real YouTube Music mixes from two seeded listening preferences"]
fn personal_mixes_surface_real_tracks_and_save_a_playlist() {
    use riff_core::{Engine, model::PlayEvent};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
    let root = tempfile::tempdir().unwrap();
    let mut engine = Engine::open(root.path().into()).unwrap();
    for (id, title, artist) in [
        ("dQw4w9WgXcQ", "Never Gonna Give You Up", "Rick Astley"),
        ("FGBhQbmPwH8", "One More Time", "Daft Punk"),
    ] {
        engine
            .dispatch(&json!({"command":"library.add","params":{"track":{
                "id":format!("youtube:{id}"),"title":title,"artist":artist,
                "platform":"ytmusic","source":format!("https://music.youtube.com/watch?v={id}")
            }}}))
            .unwrap();
    }
    engine.store.data.history.push(PlayEvent {
        track_id: "youtube:dQw4w9WgXcQ".into(),
        started_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    });
    engine
        .store
        .data
        .favorites
        .push("youtube:FGBhQbmPwH8".into());
    let job = engine.dispatch(&json!({"command":"music.mixes"})).unwrap();
    let started = Instant::now();
    let result = loop {
        engine.tick().unwrap();
        let response = engine
            .dispatch(&json!({"command":"job.get","params":{"id":job["job_id"]}}))
            .unwrap();
        if response["state"] == "completed" {
            break response["data"].clone();
        }
        assert!(
            response["state"] != "failed" && started.elapsed() < Duration::from_secs(40),
            "{response}"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let mixes = result["mixes"].as_array().unwrap();
    assert_eq!(mixes.len(), 2);
    let picks = result["picks"].as_array().unwrap();
    assert!(!picks.is_empty());
    assert_eq!(
        picks
            .iter()
            .map(|track| track["id"].as_str().unwrap())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        picks.len()
    );
    let mix = &mixes[0];
    let playlist = engine
        .dispatch(&json!({"command":"mix.apply","params":mix}))
        .unwrap();
    assert_eq!(
        playlist["tracks"].as_array().unwrap().len(),
        mix["tracks"].as_array().unwrap().len()
    );
    assert!(
        engine
            .store
            .data
            .playlists
            .contains_key(playlist["id"].as_str().unwrap())
    );
    println!(
        "Generated {} mixes and {} picks, saved {} tracks",
        mixes.len(),
        picks.len(),
        playlist["tracks"].as_array().unwrap().len()
    );
}

#[test]
#[ignore = "Plays muted live YouTube audio and fills Up Next from YouTube Music radio"]
fn normal_playback_autofills_up_next() {
    use riff_core::Engine;
    use std::time::{Duration, Instant};
    let root = tempfile::tempdir().unwrap();
    let mut engine = Engine::open(root.path().into()).unwrap();
    let id = "youtube:dQw4w9WgXcQ";
    for request in [
        json!({"command":"library.add","params":{"track":{
            "id":id,"title":"Never Gonna Give You Up","artist":"Rick Astley",
            "platform":"ytmusic","source":"https://music.youtube.com/watch?v=dQw4w9WgXcQ"
        }}}),
        json!({"command":"player.effects","params":{"muted":true}}),
        json!({"command":"queue.play","params":{"tracks":[id],"index":0}}),
    ] {
        engine.dispatch(&request).unwrap();
    }
    let started = Instant::now();
    while engine.store.data.queue.tracks.len() <= 1 {
        engine.tick().unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(45),
            "Autoplay did not fill queue"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(engine.player.track_id.as_deref(), Some(id));
    assert_eq!(engine.store.data.queue.current(), Some(id));
    let added = engine.store.data.queue.tracks.len() - 1;
    assert!((1..=24).contains(&added));
    let next = engine.store.data.queue.tracks[1].clone();
    engine
        .dispatch(&json!({"command":"player.next.request"}))
        .unwrap();
    let started = Instant::now();
    while engine.player.track_id.as_deref() != Some(next.as_str()) {
        engine.tick().unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(45),
            "Suggested track did not play"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    engine.dispatch(&json!({"command":"player.stop"})).unwrap();
    println!("Appended {added} radio tracks and played the first suggestion");
}
