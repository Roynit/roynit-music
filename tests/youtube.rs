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
