use riff_core::{Engine, spotify_public::*};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn item() -> Value {
    json!({"uri":"spotify:track:abcdefghijklmnopqrstuv","name":"A song","artists":{"items":[{"profile":{"name":"An artist"}}]},"trackDuration":{"totalMilliseconds":180500},"albumOfTrack":{"name":"An album","coverArt":{"sources":[{"url":"https://i.scdn.co/cover"}]}}})
}
#[test]
fn public_mapping_and_urls_preserve_identity_and_metadata() {
    let id = "3cEYpjA9oz9GiPac4AsH4n";
    assert_eq!(
        entity_id(&format!("spotify:playlist:{id}"), "playlist").unwrap(),
        id
    );
    assert_eq!(
        entity_id(
            &format!("https://open.spotify.com/intl-vi/playlist/{id}?si=test"),
            "playlist"
        )
        .unwrap(),
        id
    );
    assert!(
        entity_id(
            &format!("https://open.spotify.com.attacker.test/playlist/{id}"),
            "playlist"
        )
        .is_err()
    );
    assert!(entity_id("https://open.spotify.com/playlist/not-valid", "playlist").is_err());
    let value = json!({"data":{"searchV2":{"tracksV2":{"items":[{"item":{"data":item()}},{"item":{"data":{"__typename":"NotFound"}}}]}}}});
    let tracks = parse_search(&value);
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].artist, "An artist");
    assert_eq!(tracks[0].duration, 180.5);
    assert_eq!(
        tracks[0].source,
        "https://open.spotify.com/track/abcdefghijklmnopqrstuv"
    );
    assert_eq!(tracks[0].album, "An album");
}
#[test]
fn embed_fallback_rejects_unknown_or_partial_lists_and_preserves_duplicates() {
    let v = item();
    assert!(complete_embed_tracks(&json!({"trackList":[v]})).is_err());
    assert!(complete_embed_tracks(&json!({"trackList":[v],"totalCount":2})).is_err());
    let tracks = complete_embed_tracks(&json!({"trackList":[v,v],"totalCount":2})).unwrap();
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0].id, tracks[1].id);
    assert!(parse_embed("<html>private</html>").is_err());
    assert!(
        parse_embed(
            r#"<script id="__NEXT_DATA__">{"props":{"pageProps":{"status":404}}}</script>"#
        )
        .is_err()
    );
}
fn wait(e: &mut Engine, job: Value) -> Value {
    let start = Instant::now();
    loop {
        e.tick().unwrap();
        let state = e
            .dispatch(&json!({"command":"job.get","params":{"id":job["job_id"]}}))
            .unwrap();
        if state["state"] != "running" && state["state"] != "queued" {
            assert_eq!(state["state"], "completed", "{state}");
            return state["data"].clone();
        }
        assert!(start.elapsed() < Duration::from_secs(90));
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
#[ignore = "Calls public Spotify search, track embed and complete playlist import without OAuth"]
fn real_guest_search_and_complete_playlist_import_without_api_keys() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = Engine::open(dir.path().into()).unwrap();
    let job=e.dispatch(&json!({"command":"spotify.search","params":{"query":"Rick Astley Never Gonna Give You Up","limit":10}})).unwrap();
    let results = wait(&mut e, job);
    assert_eq!(results["access"], "public_guest");
    let tracks = results["tracks"].as_array().unwrap();
    assert!(!tracks.is_empty());
    assert!(
        tracks
            .iter()
            .all(|t| t["platform"] == "spotify"
                && t["artist"].as_str().is_some_and(|a| !a.is_empty()))
    );
    println!("Guest search: {} real Spotify tracks", tracks.len());
    let selected = tracks[0].clone();
    e.dispatch(&json!({"command":"library.add","params":{"track":selected}}))
        .unwrap();
    let mut effects = e.store.data.effects.clone();
    effects.muted = true;
    e.dispatch(&json!({"command":"player.effects","params":effects}))
        .unwrap();
    let play = e
        .dispatch(&json!({"command":"player.request","params":{"id":selected["id"]}}))
        .unwrap();
    wait(&mut e, play);
    let start = Instant::now();
    while e.player.position() < 0.2 {
        e.tick().unwrap();
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "Matched stream did not produce audio"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    assert_eq!(e.player.snapshot()["track_id"], selected["id"]);
    e.dispatch(&json!({"command":"player.stop"})).unwrap();
    println!("Anonymous Spotify result produced native audio through matched YouTube Music stream");
    let job=e.dispatch(&json!({"command":"job.submit","params":{"operation":"catalog.import","params":{"url":"https://open.spotify.com/playlist/3cEYpjA9oz9GiPac4AsH4n"}}})).unwrap();
    let result = wait(&mut e, job);
    assert_eq!(result["complete"], true);
    assert_eq!(result["total"], 5);
    assert_eq!(result["matching"], "deferred_until_playback");
    let ids = result["playlist"]["tracks"].as_array().unwrap();
    assert_eq!(ids.len(), 5);
    assert!(
        ids.iter()
            .all(|id| id.as_str().unwrap().starts_with("spotify:"))
    );
    let stored = std::fs::read_to_string(dir.path().join("library.json")).unwrap();
    assert!(!stored.contains("accessToken"));
    assert!(!stored.contains("Bearer"));
    println!(
        "Anonymous playlist import: {} / {} tracks",
        ids.len(),
        result["total"]
    );
    let job=e.dispatch(&json!({"command":"spotify.search","params":{"query":"https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC"}})).unwrap();
    let result = wait(&mut e, job);
    assert_eq!(result["tracks"].as_array().unwrap().len(), 1);
}
