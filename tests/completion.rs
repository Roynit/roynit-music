use riff_core::{
    Engine,
    appearance::Appearance,
    cache,
    store::{self, Store},
    updates,
};
use serde_json::json;
use sha2::{Digest, Sha256};
#[test]
fn quota_preserves_active_audio_and_unmanaged_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join("cache")).unwrap();
    let active = root
        .join("cache")
        .join(format!("{}.opus", store::key("active")));
    let old = root
        .join("cache")
        .join(format!("{}.opus", store::key("old")));
    std::fs::write(&active, [0; 64]).unwrap();
    std::fs::write(&old, [0; 64]).unwrap();
    std::fs::write(root.join("cache/user-notes.txt"), b"keep").unwrap();
    let status = cache::prune(root, 0, std::slice::from_ref(&active)).unwrap();
    assert!(active.exists());
    assert!(!old.exists());
    assert!(root.join("cache/user-notes.txt").exists());
    assert_eq!(status["over_limit"], true);
    cache::prune(root, 0, &[]).unwrap();
    assert!(!active.exists());
}
#[test]
fn appearance_applies_validated_native_theme_and_rejects_invalid_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = Engine::open(dir.path().into()).unwrap();
    e.dispatch(&json!({"command":"themes.save","params":{"id":"example","name":"Example","tokens":{"mode":"light","accent":"#1234AB","font_size":18,"visualizer":"bars","reduced_motion":true}}})).unwrap();
    e.dispatch(&json!({"command":"themes.apply","params":{"id":"example"}}))
        .unwrap();
    assert_eq!(
        e.store.data.settings["appearance"]["accent"],
        json!([18, 52, 171])
    );
    assert!(Appearance::from_tokens(json!({"accent":"#nope"})).is_err());
    assert!(Appearance::from_tokens(json!({"font_size":100})).is_err());
}
#[test]
#[cfg(windows)]
fn proxy_secrets_are_encrypted_and_not_exported() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = Engine::open(dir.path().into()).unwrap();
    let value=e.dispatch(&json!({"command":"network.set","params":{"proxy":"http://test-user:test-password@127.0.0.1:7890"}})).unwrap();
    assert_eq!(value["proxy"], "http://127.0.0.1:7890");
    assert!(!value.to_string().contains("test-password"));
    e.store.export(&dir.path().join("backup.json")).unwrap();
    assert!(
        !std::fs::read_to_string(dir.path().join("backup.json"))
            .unwrap()
            .contains("test-password")
    );
    let encrypted = std::fs::read(dir.path().join("network.dpapi")).unwrap();
    assert!(!String::from_utf8_lossy(&encrypted).contains("test-password"));
    drop(e);
    let e = Engine::open(dir.path().into()).unwrap();
    assert!(
        e.providers
            .proxy
            .as_ref()
            .unwrap()
            .contains("test-password")
    );
}
#[test]
fn updater_rejects_other_repositories_and_bad_checksums() {
    let repo = "example/riff-rust";
    let prefix = format!("https://github.com/{repo}/releases/download/v1.2.3/");
    let release = json!({"tag_name":"v1.2.3","assets":[{"name":updates::ASSET,"browser_download_url":format!("{prefix}{}",updates::ASSET)},{"name":format!("{}.sha256",updates::ASSET),"browser_download_url":format!("{prefix}{}.sha256",updates::ASSET)}]});
    assert_eq!(
        updates::release_info(repo, &release, "1.2.2").unwrap()["available"],
        true
    );
    assert!(updates::validate_repo("rootscripts/riff").is_err());
    assert!(updates::release_info("other/repo", &release, "1.0.0").is_err());
    let dir = tempfile::tempdir().unwrap();
    let bytes = b"MZ fixture executable";
    let hash = format!("{:x}", Sha256::digest(bytes));
    let path = updates::verified_file(dir.path(), "1.2.3", bytes, &hash).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(updates::verified_file(dir.path(), "1.2.4", bytes, &"0".repeat(64)).is_err());
    assert!(
        !dir.path()
            .join("updates/riff-rust-1.2.4-windows-x64.exe")
            .exists()
    );
}
#[test]
fn storage_write_failure_restores_last_committed_memory() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().into()).unwrap();
    store
        .data
        .settings
        .insert("value".into(), json!("committed"));
    store.save().unwrap();
    std::fs::rename(
        dir.path().join("library.json"),
        dir.path().join("library-saved.json"),
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("library.json")).unwrap();
    store.data.settings.insert("value".into(), json!("unsaved"));
    assert!(store.save().is_err());
    assert_eq!(store.data.settings["value"], "committed");
}
#[test]
fn installer_copies_owned_files_and_retains_previous_executable() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.exe");
    let target = dir.path().join("install");
    std::fs::write(&source, b"MZ first").unwrap();
    let exe = riff_core::installation::copy_bundle(&source, &target).unwrap();
    assert_eq!(std::fs::read(&exe).unwrap(), b"MZ first");
    assert!(target.join("LICENSE").is_file());
    std::fs::write(&source, b"MZ second").unwrap();
    riff_core::installation::copy_bundle(&source, &target).unwrap();
    assert_eq!(
        std::fs::read(target.join("riff-desktop.previous.exe")).unwrap(),
        b"MZ first"
    );
}
#[test]
fn catalog_import_defers_matching_and_refuses_unapproved_partial_data() {
    let data = json!({"name":"Collection","partial":true,"tracks":[{"title":"Song","artist":"Artist","duration":180}]});
    assert!(
        riff_core::catalog::import_plan(&data, "https://www.deezer.com/playlist/1", false).is_err()
    );
    let plan =
        riff_core::catalog::import_plan(&data, "https://www.deezer.com/playlist/1", true).unwrap();
    assert_eq!(plan["tracks"][0]["platform"], "catalog");
    assert_eq!(plan["tracks"][0]["duration"].as_f64(), Some(180.));
    assert_eq!(plan["matching"], "deferred_until_playback");
}
#[test]
fn asynchronous_queue_preparation_is_cancellable_and_timer_expires() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = Engine::open(dir.path().into()).unwrap();
    e.dispatch(&json!({"command":"library.add","params":{"track":{"id":"local:one","title":"One","duration":20,"source":"local","local_path":dir.path().join("unused.wav")}}})).unwrap();
    let job = e
        .dispatch(&json!({"command":"queue.play","params":{"tracks":["local:one"]}}))
        .unwrap();
    assert!(job["job_id"].is_string());
    assert_eq!(e.snapshot()["pending_track_id"], "local:one");
    e.dispatch(&json!({"command":"jobs.cancel_all"})).unwrap();
    assert!(e.snapshot()["pending_track_id"].is_null());
    e.dispatch(&json!({"command":"player.stop"})).unwrap();
    e.dispatch(&json!({"command":"player.sleep","params":{"seconds":0.01}}))
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));
    let mut fired = false;
    for _ in 0..20 {
        if let Some(event) = e.tick().unwrap() {
            fired |= event["event"] == "sleep_timer";
        }
    }
    assert!(fired);
    assert!(e.snapshot()["sleep_remaining"].is_null());
    assert!(e.player.track_id.is_none());
}

#[test]
fn translation_chunks_preserve_unicode_and_cache_same_language() {
    let text = "Tôi thích âm nhạc. ".repeat(200);
    let chunks = riff_core::translation::chunks(&text);
    assert!(chunks.iter().all(|c| c.len() <= 450));
    assert_eq!(chunks.concat(), text);
    assert_eq!(riff_core::translation::detect_source(&text), Some("vi"));
    let root = tempfile::tempdir().unwrap();
    let client = riff_core::network::Config::default().client().unwrap();
    let request =
        json!({"text":"Tôi thích âm nhạc.","source":"vi","target":"vi","provider":"mymemory"});
    let result = riff_core::translation::fetch(&client, root.path(), &request).unwrap();
    assert_eq!(result["text"], request["text"]);
    assert_eq!(
        riff_core::translation::fetch(&client, root.path(), &request).unwrap()["cached"],
        true
    );
}

fn wait(e: &mut Engine, mut job: serde_json::Value) -> serde_json::Value {
    let started = std::time::Instant::now();
    loop {
        e.tick().unwrap();
        let status = e
            .dispatch(&json!({"command":"job.get","params":{"id":job["job_id"]}}))
            .unwrap();
        if status["state"] != "running" && status["state"] != "queued" {
            assert_eq!(status["state"], "completed", "{status}");
            let data = status["data"].clone();
            if data["job_id"].is_string() {
                job = data;
                continue;
            }
            return data;
        }
        assert!(started.elapsed() < std::time::Duration::from_secs(60));
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
#[test]
#[ignore = "Native muted output, FFmpeg, physical default-device selection and native plugin hook"]
fn native_async_playback_output_device_sleep_and_hook() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("fixture.wav");
    riff_core::tools::run(
        "ffmpeg",
        [
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=4",
            file.to_str().unwrap(),
        ],
        std::time::Duration::from_secs(15),
    )
    .unwrap();
    let mut e = Engine::open(root.path().join("data")).unwrap();
    e.dispatch(&json!({"command":"player.effects","params":{"muted":true}}))
        .unwrap();
    let devices = e.dispatch(&json!({"command":"player.devices"})).unwrap();
    e.dispatch(&json!({"command":"player.device","params":{"name":devices["default"]}}))
        .unwrap();
    assert_eq!(e.store.data.settings["audio_device"], devices["default"]);
    let scan = e
        .dispatch(&json!({"command":"library.scan","params":{"paths":[file]}}))
        .unwrap();
    let id = scan["tracks"][0]["id"].clone();
    let plugin = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"))
        .join("release/examples/shuffle_queue.exe");
    assert!(plugin.is_file());
    let manifest = root.path().join("hook.json");
    store::write_json(&manifest,&json!({"id":"queue-hook","name":"Queue hook","version":"1","executable":plugin,"hooks":["track_changed"]})).unwrap();
    e.dispatch(&json!({"command":"plugins.import","params":{"path":manifest}}))
        .unwrap();
    e.dispatch(&json!({"command":"plugins.enable","params":{"id":"queue-hook","enabled":true}}))
        .unwrap();
    let job = e
        .dispatch(&json!({"command":"queue.play","params":{"tracks":[id,id,id]}}))
        .unwrap();
    wait(&mut e, job);
    let start = std::time::Instant::now();
    let mut hook = false;
    while start.elapsed() < std::time::Duration::from_secs(3) {
        e.tick().unwrap();
        let jobs = e.dispatch(&json!({"command":"job.list"})).unwrap();
        hook = jobs
            .as_array()
            .unwrap()
            .iter()
            .any(|j| j["operation"] == "plugins.run" && j["state"] == "completed");
        if hook && e.player.position() > 0.05 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(hook);
    assert!(e.player.position() > 0.05);
    let next = e
        .dispatch(&json!({"command":"player.next.request"}))
        .unwrap();
    e.dispatch(&json!({"command":"queue.append","params":{"tracks":[id]}}))
        .unwrap();
    e.dispatch(&json!({"command":"player.pause"})).unwrap();
    assert_eq!(e.store.data.queue.index, Some(0));
    assert_eq!(e.store.data.queue.tracks.len(), 4);
    for _ in 0..10 {
        e.tick().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        e.dispatch(&json!({"command":"job.get","params":{"id":next["job_id"]}}))
            .unwrap()["state"],
        "cancelled"
    );
    #[cfg(windows)]
    {
        // Windows environment changes are thread-safe; this ignored native test is run serially.
        let old = std::env::var_os("RIFF_FFMPEG");
        unsafe {
            std::env::set_var("RIFF_FFMPEG", root.path().join("missing-decoder.exe"));
        }
        let result = e.player.play(
            "wrong-track".into(),
            "missing.wav".into(),
            vec![],
            20.,
            riff_core::model::Effects {
                muted: true,
                ..Default::default()
            },
        );
        unsafe {
            if let Some(value) = old {
                std::env::set_var("RIFF_FFMPEG", value);
            } else {
                std::env::remove_var("RIFF_FFMPEG");
            }
        }
        assert!(result.is_err());
        assert_eq!(e.player.track_id.as_deref(), id.as_str());
    }
    e.dispatch(&json!({"command":"player.resume"})).unwrap();
    e.dispatch(&json!({"command":"player.sleep","params":{"seconds":0.05}}))
        .unwrap();
    let start = std::time::Instant::now();
    while !e.player.paused() {
        e.tick().unwrap();
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    e.dispatch(&json!({"command":"player.stop"})).unwrap();
}

#[test]
fn failed_next_restores_queue_cursor_and_offline_delete_protects_originals() {
    let root = tempfile::tempdir().unwrap();
    let mut e = Engine::open(root.path().join("data")).unwrap();
    for id in ["one", "two"] {
        e.dispatch(&json!({"command":"library.add","params":{"track":{"id":id,"title":id,"duration":20,"source":"local","local_path":root.path().join(format!("{id}.wav"))}}})).unwrap();
    }
    e.dispatch(&json!({"command":"queue.set","params":{"tracks":["one","two"],"index":0}}))
        .unwrap();
    let job = e
        .dispatch(&json!({"command":"player.next.request"}))
        .unwrap();
    for _ in 0..30 {
        e.tick().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        e.dispatch(&json!({"command":"job.get","params":{"id":job["job_id"]}}))
            .unwrap()["state"],
        "failed"
    );
    assert_eq!(e.store.data.queue.index, Some(0));
    let original = root.path().join("original.wav");
    std::fs::write(&original, b"original audio").unwrap();
    let id = "youtube:abcdefghijk";
    let mut track = json!({"id":id,"title":"Download","artist":"Artist","platform":"youtube","duration":20,"source":"https://www.youtube.com/watch?v=abcdefghijk","local_path":original});
    e.dispatch(&json!({"command":"library.add","params":{"track":track}}))
        .unwrap();
    assert!(
        e.dispatch(&json!({"command":"offline.delete","params":{"id":id}}))
            .is_err()
    );
    assert!(original.exists());
    let download = e
        .store
        .root
        .join("downloads")
        .join(format!("{}.wav", store::key(id)));
    std::fs::copy(&original, &download).unwrap();
    track["local_path"] = json!(download);
    e.dispatch(&json!({"command":"library.add","params":{"track":track}}))
        .unwrap();
    e.dispatch(&json!({"command":"offline.delete","params":{"id":id}}))
        .unwrap();
    assert!(!download.exists());
    assert!(original.exists());
}
#[test]
#[ignore = "Live Spotify-seeded YouTube Music radio and muted native streaming"]
fn spotify_seed_radio_preserves_original_track_and_provider_queue() {
    let root = tempfile::tempdir().unwrap();
    let mut e = Engine::open(root.path().into()).unwrap();
    let id = "spotify:4PTG3Z6ehGkBFwjybzWkR8";
    e.dispatch(&json!({"command":"library.add","params":{"track":{"id":id,"title":"Never Gonna Give You Up","artist":"Rick Astley","duration":213.573,"platform":"spotify","source":"https://open.spotify.com/track/4PTG3Z6ehGkBFwjybzWkR8"}}})).unwrap();
    e.dispatch(&json!({"command":"player.effects","params":{"muted":true}}))
        .unwrap();
    let job = e
        .dispatch(&json!({"command":"radio.start","params":{"id":id}}))
        .unwrap();
    wait(&mut e, job);
    assert_eq!(e.store.data.queue.tracks[0], id);
    assert!(e.store.data.queue.tracks.len() > 10);
    let start = std::time::Instant::now();
    while e.player.track_id.as_deref() != Some(id) || e.player.position() < 0.05 {
        e.tick().unwrap();
        assert!(start.elapsed() < std::time::Duration::from_secs(30));
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    assert_eq!(e.snapshot()["playback_source"]["verified_duration"], true);
    e.dispatch(&json!({"command":"player.stop"})).unwrap();
}

#[test]
fn bulk_download_validates_before_starting_and_skips_local_files() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = Engine::open(dir.path().into()).unwrap();
    e.store.data.tracks.insert("local:fixture".into(), serde_json::from_value(json!({"id":"local:fixture","title":"Fixture","source":"fixture.wav","platform":"local"})).unwrap());
    let result = e.dispatch(&json!({"command":"download.bulk","params":{"ids":["local:fixture","local:fixture"],"format":"opus"}})).unwrap();
    assert_eq!(result["skipped"], 1);
    assert_eq!(result["jobs"], json!([]));
    assert!(
        e.dispatch(
            &json!({"command":"download.bulk","params":{"ids":["missing"],"format":"opus"}})
        )
        .is_err()
    );
    assert!(e.snapshot()["tasks"].as_array().unwrap().is_empty());
}
#[test]
fn cloudflare_music_manifest_is_scoped_and_validated() {
    let value = serde_json::json!({"version":"0.1.3","filename":"Roynit-Music-Setup.exe","sha256":"a".repeat(64),"notes":"Update"});
    let info = riff_core::updates::music_release_info(&value, "0.1.2").unwrap();
    assert_eq!(info["available"], true);
    assert_eq!(
        info["download"],
        "https://roynit.dangz.org/get/roynit-music/0.1.3"
    );
    assert_eq!(
        riff_core::updates::music_release_info(&value, "0.1.3").unwrap()["available"],
        false
    );
    for invalid in [
        serde_json::json!({"version":"0.1.3/evil", "filename":"x.exe","sha256":"a".repeat(64)}),
        serde_json::json!({"version":"0.1.3","filename":"x.zip","sha256":"a".repeat(64)}),
        serde_json::json!({"version":"0.1.3","filename":"x.exe","sha256":"invalid"}),
    ] {
        assert!(riff_core::updates::music_release_info(&invalid, "0.1.2").is_err());
    }
}

#[test]
fn idle_play_and_seek_do_not_require_a_loaded_decoder() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = riff_core::Engine::open(dir.path().into()).unwrap();
    for command in ["player.resume", "player.toggle", "player.seek"] {
        let response = engine
            .dispatch(&serde_json::json!({"command":command,"params":{"seconds":0.5}}))
            .unwrap();
        assert!(response["track_id"].is_null());
    }
    let id = "youtube:dQw4w9WgXcQ";
    for request in [
        serde_json::json!({"command":"library.add","params":{"track":{"id":id,"title":"Test track","source":"https://www.youtube.com/watch?v=dQw4w9WgXcQ","platform":"youtube"}}}),
        serde_json::json!({"command":"queue.set","params":{"tracks":[id],"index":0}}),
    ] {
        engine.dispatch(&request).unwrap();
    }
    let job = engine
        .dispatch(&serde_json::json!({"command":"player.resume"}))
        .unwrap();
    assert!(job["job_id"].is_string());
    for command in ["player.resume", "player.toggle", "player.seek"] {
        let response = engine
            .dispatch(&serde_json::json!({"command":command,"params":{"seconds":0.5}}))
            .unwrap();
        assert_eq!(response["pending_track_id"], id);
        assert!(response["job_id"].is_null());
    }
    engine
        .dispatch(&serde_json::json!({"command":"player.stop"}))
        .unwrap();
}
