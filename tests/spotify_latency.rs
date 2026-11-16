use riff_core::{Engine, model::Track, spotify};
use serde_json::json;

fn track(id: &str, title: &str, artist: &str, duration: f64) -> Track {
    Track {
        id: id.into(),
        title: title.into(),
        artist: artist.into(),
        album: String::new(),
        duration,
        source: format!("https://www.youtube.com/watch?v={id}"),
        platform: "youtube".into(),
        cover: None,
        local_path: None,
        cache_path: None,
    }
}
#[test]
fn spotify_catalog_mapping_and_matching_reject_wrong_recordings() {
    let input = json!({"tracks":{"items":[null,{"id":"abc","name":"Song","artists":[{"name":"Artist"}],"album":{"name":"Album","images":[{"url":"https://i.scdn.co/cover"}]},"duration_ms":180000,"external_urls":{"spotify":"https://open.spotify.com/track/abc"}}]}});
    let tracks = spotify::parse_tracks(&input);
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].platform, "spotify");
    assert_eq!(tracks[0].duration, 180.);
    let candidates = vec![
        track("wrong", "Song", "Other", 180.),
        track("live", "Song live", "Artist", 180.),
        track("right", "Song", "Artist", 181.),
    ];
    assert_eq!(
        spotify::best_match(&tracks[0], &candidates).unwrap().id,
        "right"
    );
    assert!(spotify::best_match(&tracks[0], &candidates[..2]).is_none());
}
#[test]
fn cancelled_pending_play_cannot_restart_after_stop() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = Engine::open(dir.path().into()).unwrap();
    let mut t = track("local", "Local", "Test", 30.);
    t.local_path = Some(dir.path().join("missing.wav"));
    e.dispatch(&json!({"command":"library.add","params":{"track":t}}))
        .unwrap();
    let job = e
        .dispatch(&json!({"command":"player.request","params":{"id":"local"}}))
        .unwrap();
    e.dispatch(&json!({"command":"player.stop"})).unwrap();
    for _ in 0..50 {
        let _ = e.tick();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(e.player.track_id.is_none());
    let status = e
        .dispatch(&json!({"command":"job.get","params":{"id":job["job_id"]}}))
        .unwrap();
    assert_eq!(status["state"], "cancelled");
}
#[test]
#[ignore = "Measures real yt-dlp resolution and cached replay"]
fn cached_stream_resolution_avoids_relaunching_extractor() {
    let providers = riff_core::provider::Providers {
        cookies: None,
        proxy: None,
        browser: None,
    };
    let t = track(
        "dQw4w9WgXcQ",
        "Never Gonna Give You Up",
        "Rick Astley",
        213.,
    );
    providers.invalidate(&t);
    let start = std::time::Instant::now();
    let first = providers.resolve(&t).unwrap();
    let cold = start.elapsed();
    let start = std::time::Instant::now();
    let second = providers.resolve(&t).unwrap();
    let warm = start.elapsed();
    assert_eq!(first, second);
    assert!(warm < std::time::Duration::from_millis(100));
    println!("Resolve cold={cold:?}, cached={warm:?}");
    providers.invalidate(&t);
    assert!(providers.cached(&t).is_none());
}
