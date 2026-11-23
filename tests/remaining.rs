use riff_core::{Engine, installation, store};
use serde_json::json;
#[test]
fn broken_extension_files_do_not_hide_valid_themes_or_plugins() {
    let root = tempfile::tempdir().unwrap();
    let mut e = Engine::open(root.path().into()).unwrap();
    e.dispatch(&json!({"command":"themes.save","params":{"id":"good","name":"Good","tokens":{"accent":"#123456"}}})).unwrap();
    std::fs::write(root.path().join("themes/broken.json"), "invalid").unwrap();
    std::fs::write(root.path().join("plugins/broken.json"), "invalid").unwrap();
    assert_eq!(
        e.dispatch(&json!({"command":"themes.list"}))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        e.dispatch(&json!({"command":"plugins.list"})).unwrap(),
        json!([])
    );
    let report = e.dispatch(&json!({"command":"extensions.status"})).unwrap();
    assert_eq!(report["themes"]["warnings"].as_array().unwrap().len(), 1);
    assert_eq!(report["plugins"]["warnings"].as_array().unwrap().len(), 1);
    assert!(e.dispatch(&json!({"command":"themes.save","params":{"id":"bad","name":"Bad","tokens":{"mode":"fake"}}})).is_err());
}
#[test]
fn cookie_detach_preserves_original_and_logout_survives_restart() {
    let root = tempfile::tempdir().unwrap();
    let cookie = root.path().join("private-cookie.txt");
    std::fs::write(&cookie, "# Netscape HTTP Cookie File").unwrap();
    {
        let mut e = Engine::open(root.path().into()).unwrap();
        e.dispatch(&json!({"command":"cookies.set","params":{"path":cookie}}))
            .unwrap();
        e.dispatch(&json!({"command":"cookies.clear"})).unwrap();
        e.dispatch(&json!({"command":"oauth.logout","params":{"provider":"spotify"}}))
            .unwrap();
        assert!(e.providers.cookies.is_none());
    }
    assert!(cookie.is_file());
    let mut e = Engine::open(root.path().into()).unwrap();
    assert!(e.providers.cookies.is_none());
    assert_eq!(
        e.dispatch(&json!({"command":"oauth.status","params":{"provider":"spotify"}}))
            .unwrap()["signed_in"],
        false
    );
}
#[test]
fn rollback_staging_checks_receipt_and_never_overwrites_its_input() {
    let root = tempfile::tempdir().unwrap();
    let exe = root.path().join("source.exe");
    let installed = root.path().join("installed");
    let staging = root.path().join("staging");
    std::fs::write(&exe, b"MZ first").unwrap();
    installation::copy_bundle(&exe, &installed).unwrap();
    std::fs::write(&exe, b"MZ second").unwrap();
    installation::copy_bundle(&exe, &installed).unwrap();
    let result = installation::stage_previous(&installed, &staging).unwrap();
    assert_eq!(
        std::fs::read(result["staged_path"].as_str().unwrap()).unwrap(),
        b"MZ first"
    );
    assert_eq!(
        std::fs::read(installed.join("Roy Music.previous.exe")).unwrap(),
        b"MZ first"
    );
    assert_eq!(
        std::fs::read(installed.join("Roy Music.exe")).unwrap(),
        b"MZ second"
    );
    std::fs::write(installed.join("Roy Music.previous.exe"), b"MZ tampered").unwrap();
    assert!(installation::stage_previous(&installed, &staging).is_err());
}
#[test]
fn invalid_backup_identity_and_queue_history_leave_live_library_untouched() {
    let root = tempfile::tempdir().unwrap();
    let mut e = Engine::open(root.path().into()).unwrap();
    let bad = root.path().join("bad.json");
    let mut data = e.store.data.clone();
    data.queue.back_stack = vec![10];
    store::write_json(&bad, &data).unwrap();
    assert!(
        e.dispatch(&json!({"command":"backup.import","params":{"path":bad}}))
            .is_err()
    );
    assert!(e.store.data.queue.back_stack.is_empty());
}

#[test]
#[ignore = "Runs real media-tool version checks for a support-report privacy audit"]
fn support_export_excludes_track_titles_paths_and_credentials() {
    let root = tempfile::tempdir().unwrap();
    let mut e = Engine::open(root.path().into()).unwrap();
    let track:riff_core::model::Track=serde_json::from_value(json!({"id":"local:private","title":"PRIVATE_TITLE_NEVER_EXPORT","source":"C:/private_user/music.wav","platform":"local"})).unwrap();
    e.store.data.tracks.insert(track.id.clone(), track);
    e.store
        .data
        .settings
        .insert("client_secret".into(), json!("PRIVATE_SECRET_NEVER_EXPORT"));
    let path = root.path().join("support.json");
    let job = e
        .dispatch(&json!({"command":"support.export","params":{"path":path}}))
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(40);
    loop {
        e.tick().unwrap();
        let status = e
            .dispatch(&json!({"command":"job.get","params":{"id":job["job_id"]}}))
            .unwrap();
        if status["state"] == "completed" {
            break;
        }
        assert!(status["state"] != "failed", "{status}");
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let text = std::fs::read_to_string(path).unwrap();
    assert!(!text.contains("PRIVATE_"));
    assert!(!text.contains("private_user"));
    assert!(!text.contains(root.path().to_str().unwrap()));
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["track_count"], 1);
    assert!(report["tools"]["ffmpeg"]["available"].is_boolean());
}
