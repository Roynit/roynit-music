use riff_core::Engine;
use serde_json::json;

#[test]
#[ignore = "Live network test; requires installed yt-dlp"]
fn youtube_and_soundcloud_search() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = Engine::open(dir.path().into()).unwrap();
    for platform in ["youtube", "soundcloud"] {
        let v=e.dispatch(&json!({"command":"search","params":{"query":"Beethoven Moonlight Sonata","limit":2,"platform":platform}})).unwrap();
        assert!(
            !v.as_array().unwrap().is_empty(),
            "{platform} returned no results"
        );
        println!("{platform}: {} results", v.as_array().unwrap().len());
    }
}

#[test]
#[ignore = "Live lyrics service test"]
fn synchronized_lyrics_lookup_and_offline_cache() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = Engine::open(dir.path().into()).unwrap();
    let request = json!({"command":"lyrics.get","params":{"title":"Never Gonna Give You Up","artist":"Rick Astley","duration":213.}});
    let v = e.dispatch(&request).unwrap();
    assert!(!v["text"].as_str().unwrap().is_empty());
    let cached=e.dispatch(&json!({"command":"lyrics.get","params":{"title":"Never Gonna Give You Up","artist":"Rick Astley","online":false}})).unwrap();
    assert_eq!(v, cached);
}

#[test]
#[ignore = "Live YouTube extraction and muted native streaming playback; may require user cookies"]
fn resolve_and_stream_public_audio() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = Engine::open(dir.path().into()).unwrap();
    e.dispatch(&json!({"command":"library.add","params":{"track":{"id":"youtube:jNQXAC9IVRw","title":"Me at the zoo","source":"https://www.youtube.com/watch?v=jNQXAC9IVRw","platform":"youtube"}}})).unwrap();
    e.dispatch(&json!({"command":"player.effects","params":{"muted":true}}))
        .unwrap();
    e.dispatch(&json!({"command":"player.play","params":{"id":"youtube:jNQXAC9IVRw"}}))
        .unwrap();
    let mut peak = 0_f64;
    for _ in 0..100 {
        e.tick().unwrap();
        let a = e.player.analysis();
        peak = peak.max(a["peak"].as_f64().unwrap_or(0.));
        if peak > 0.001 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let state = e.player.snapshot();
    e.player.stop();
    assert!(peak > 0.001, "No PCM received: {state}");
}

#[test]
#[ignore = "Downloads a short public test video as Opus, then verifies muted offline playback"]
fn download_metadata_and_offline_playback() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = Engine::open(dir.path().into()).unwrap();
    let id = "youtube:jNQXAC9IVRw";
    e.dispatch(&json!({"command":"library.add","params":{"track":{"id":id,"title":"Me at the zoo","source":"https://www.youtube.com/watch?v=jNQXAC9IVRw","platform":"youtube"}}})).unwrap();
    let v = e
        .dispatch(&json!({"command":"download","params":{"id":id,"format":"opus"}}))
        .unwrap();
    let path = std::path::PathBuf::from(v["path"].as_str().unwrap());
    assert!(path.is_file());
    let track = riff_core::media::inspect(&path).unwrap();
    assert!(track.duration > 10.);
    assert!(!track.title.is_empty());
    // A broken network source proves the saved file is selected without provider resolution.
    e.store.data.tracks.get_mut(id).unwrap().source = "https://invalid.test/not-a-provider".into();
    e.dispatch(&json!({"command":"player.effects","params":{"muted":true}}))
        .unwrap();
    e.dispatch(&json!({"command":"player.play","params":{"id":id}}))
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));
    let peak = e.player.analysis()["peak"].as_f64().unwrap();
    e.player.stop();
    assert!(peak > 0.001);
}
