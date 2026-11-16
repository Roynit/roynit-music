use serde_json::json;
#[test]
fn parses_apple_and_vk_public_metadata_without_duplicates() {
    let html = r#"<html><head><meta property="og:title" content="Example playlist"></head><body><script type="application/json">{"tracks":[{"type":"songs","attributes":{"name":"First","artistName":"Artist","durationInMillis":213000}},{"@type":"MusicRecording","name":"Second","byArtist":{"name":"Other"},"duration":"PT4M5S"}],"trackCount":3}</script><div data-audio='[1,2,"url","First","Artist",213]'></div></body></html>"#;
    let v = riff_core::imports::parse_public_html(html).unwrap();
    assert_eq!(v["name"], "Example playlist");
    assert_eq!(v["tracks"].as_array().unwrap().len(), 2);
    assert_eq!(v["tracks"][1]["duration"], 245.);
    assert_eq!(v["partial"], true);
}
#[test]
fn google_library_mapping_handles_deleted_and_private_items() {
    let v = json!({"items":[{"id":"dQw4w9WgXcQ","snippet":{"title":"Song","channelTitle":"Artist"},"contentDetails":{"duration":"PT3M33S"}},{"id":"abcdefghijk","snippet":{"title":"Private video"}}]});
    let tracks = riff_core::oauth::map_youtube_tracks(&v);
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].duration, 213.);
    assert_eq!(tracks[0].id, "youtube:dQw4w9WgXcQ");
}
#[test]
fn queue_reordering_preserves_current_occurrence_and_rejects_invalid_permutation() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = riff_core::Engine::open(dir.path().into()).unwrap();
    e.store.data.queue.tracks = vec!["a".into(), "a".into(), "b".into()];
    e.store.data.queue.index = Some(1);
    e.dispatch(&json!({"command":"queue.reorder","params":{"order":[2,0,1]}}))
        .unwrap();
    assert_eq!(e.store.data.queue.index, Some(2));
    assert_eq!(e.store.data.queue.tracks[2], "a");
    assert!(
        e.dispatch(&json!({"command":"queue.reorder","params":{"order":[0,0,2]}}))
            .is_err()
    );
}
#[test]
#[ignore = "Requires cargo build --release --example shuffle_queue"]
fn native_rust_plugin_executes_and_requires_explicit_enable() {
    let binary = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"))
        .join(format!(
            "release/examples/shuffle_queue{}",
            std::env::consts::EXE_SUFFIX
        ));
    assert!(binary.is_file());
    let dir = tempfile::tempdir().unwrap();
    let mut e = riff_core::Engine::open(dir.path().join("data")).unwrap();
    let manifest = dir.path().join("plugin.json");
    std::fs::write(
        &manifest,
        json!({"id":"shuffle","name":"Shuffle upcoming","version":"1.0.0","executable":binary})
            .to_string(),
    )
    .unwrap();
    let plugin = riff_core::plugins::import(&e.store.root, &manifest).unwrap();
    assert!(!plugin.enabled);
    assert!(
        riff_core::plugins::execute(
            &e.store.root,
            "shuffle",
            json!({"queue":{"tracks":["a","b"],"index":0}})
        )
        .is_err()
    );
    riff_core::plugins::set_enabled(&e.store.root, "shuffle", true).unwrap();
    e.store.data.queue.tracks = vec!["a".into(), "b".into(), "c".into(), "d".into()];
    e.store.data.queue.index = Some(1);
    let result = e
        .dispatch(&json!({"command":"plugins.run","params":{"id":"shuffle"}}))
        .unwrap();
    let id = result["job_id"].as_str().unwrap();
    let start = std::time::Instant::now();
    loop {
        e.tick().unwrap();
        let job = e
            .dispatch(&json!({"command":"job.get","params":{"id":id}}))
            .unwrap();
        if job["state"] != "running" && job["state"] != "queued" {
            assert_eq!(job["state"], "completed", "{job}");
            break;
        }
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(e.store.data.queue.tracks[0], "a");
    assert_eq!(e.store.data.queue.tracks[1], "b");
    assert_eq!(e.store.data.queue.index, Some(1));
}
