use riff_core::{
    Engine,
    media::{self, EncodeRequest},
    model::Effects,
    tools,
};
use serde_json::json;
use std::{path::Path, time::Duration};

fn fixture(path: &Path, seconds: f64) {
    tools::run(
        "ffmpeg",
        [
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=440:duration={seconds}"),
            "-ar",
            "48000",
            "-ac",
            "2",
            "-metadata",
            "title=Rust audio fixture",
            "-metadata",
            "artist=Riff tests",
            path.to_str().unwrap(),
        ],
        Duration::from_secs(15),
    )
    .unwrap();
}
#[test]
#[ignore = "Requires installed FFmpeg and FFprobe"]
fn encode_trim_effects_and_preserve_source() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.wav");
    fixture(&input, 4.);
    let original = riff_core::store::file_hash(&input).unwrap();
    let track = media::inspect(&input).unwrap();
    assert_eq!(track.title, "Rust audio fixture");
    assert_eq!(track.artist, "Riff tests");
    let output = dir.path().join("edited.flac");
    let effects = Effects {
        speed: 2.,
        pitch: 3.,
        echo: 12.,
        reverb: 8.,
        distortion: 4.,
        eq_bands: [2.; 10],
        normalize: true,
        ..Default::default()
    };
    let r = EncodeRequest {
        input: input.clone(),
        output: output.clone(),
        start: 1.,
        end: Some(3.),
        effects,
        lossless_copy: false,
        title: Some("Edited".into()),
        artist: None,
        album: None,
    };
    media::encode(&r).unwrap();
    let edited = media::inspect(&output).unwrap();
    assert_eq!(edited.title, "Edited");
    assert!(
        (0.8..2.).contains(&edited.duration),
        "duration={}",
        edited.duration
    );
    assert_eq!(riff_core::store::file_hash(&input).unwrap(), original);
    assert!(media::encode(&r).is_err());
    let waveform = media::waveform(&input, 256).unwrap();
    assert_eq!(waveform["peaks"].as_array().unwrap().len(), 256);
    assert!(waveform["peak"].as_f64().unwrap() > 0.01);
}
#[test]
#[ignore = "Requires a native audio output device, FFmpeg and FFprobe; playback is muted"]
fn native_playback_pause_seek_analysis_queue_and_crossfade() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("test.wav");
    fixture(&input, 2.);
    let mut engine = Engine::open(dir.path().join("data")).unwrap();
    engine
        .dispatch(
            &json!({"command":"player.effects","params":{"muted":true,"crossfade_seconds":0.15}}),
        )
        .unwrap();
    let result = engine
        .dispatch(&json!({"command":"library.scan","params":{"paths":[input]}}))
        .unwrap();
    let id = result["tracks"][0]["id"].as_str().unwrap().to_string();
    engine
        .dispatch(&json!({"command":"queue.set","params":{"tracks":[id,id],"play":true}}))
        .unwrap();
    std::thread::sleep(Duration::from_millis(350));
    let position = engine.player.position();
    assert!(position > 0.05, "position={position}");
    let analysis = engine.player.analysis();
    assert!(analysis["peak"].as_f64().unwrap() > 0.01);
    engine.dispatch(&json!({"command":"player.pause"})).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let paused = engine.player.position();
    std::thread::sleep(Duration::from_millis(100));
    assert!((engine.player.position() - paused).abs() < 0.05);
    engine
        .dispatch(&json!({"command":"player.seek","params":{"seconds":1.5}}))
        .unwrap();
    assert!(engine.player.paused());
    engine
        .dispatch(&json!({"command":"player.resume"}))
        .unwrap();
    let mut changed = false;
    for _ in 0..150 {
        if engine.tick().unwrap().is_some() {
            changed = true;
        }
        if engine.player.track_id.is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    assert!(changed);
    assert!(engine.player.track_id.is_none());
    assert_eq!(engine.store.data.history.len(), 2);
    engine
        .dispatch(&json!({"command":"player.preview","params":{"id":id,"start":0.3,"end":0.7}}))
        .unwrap();
    for _ in 0..40 {
        engine.tick().unwrap();
        if engine.player.paused() {
            break;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    assert!(engine.player.paused());
    assert!(engine.player.position() >= 0.7);
    engine.persist_session().unwrap();
    engine.player.stop();
    assert_eq!(engine.store.data.settings["resume"]["id"], id);
}
