use riff_core::{
    Engine, lyrics, media,
    model::{Effects, Queue, Repeat},
    store::Store,
};
use serde_json::json;

#[test]
fn queue_repeat_shuffle_and_history() {
    let mut q = Queue {
        tracks: vec!["a".into(), "b".into(), "c".into()],
        index: Some(0),
        ..Default::default()
    };
    assert_eq!(q.advance(false), Some("b"));
    assert_eq!(q.previous(), Some("a"));
    q.repeat = Repeat::One;
    assert_eq!(q.advance(true), Some("a"));
    assert_eq!(q.advance(false), Some("b"));
    q.index = Some(2);
    q.repeat = Repeat::All;
    assert_eq!(q.advance(true), Some("a"));
    q.shuffle = true;
    for _ in 0..50 {
        let old = q.index;
        q.advance(true);
        assert_ne!(old, q.index);
    }
    q.shuffle = false;
    q.repeat = Repeat::Off;
    q.index = Some(2);
    assert_eq!(q.advance(true), None);
}
#[test]
fn lrc_handles_repeated_stamps_offset_words_and_unicode() {
    let lines = lyrics::parse(
        "[offset:-250]\n[00:01.50][00:03.250]Xin chào\n[00:05.00]<00:05.00>你好 <00:05.50>world\n[ar:Name]",
    );
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].time, 1.25);
    assert_eq!(lines[1].time, 3.0);
    assert_eq!(lines[2].words.len(), 2);
    assert_eq!(lines[2].words[1].time, 5.25);
    assert_eq!(lyrics::active(&lines, 0., 0.), None);
    assert_eq!(lyrics::active(&lines, 3.1, 0.), Some(1));
}
#[test]
fn store_lock_atomic_replace_and_corruption_protection() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().into()).unwrap();
    assert!(Store::open(dir.path().into()).is_err());
    store.data.settings.insert("first".into(), json!(1));
    store.save().unwrap();
    store.data.settings.insert("second".into(), json!(2));
    store.save().unwrap();
    drop(store);
    assert_eq!(
        Store::open(dir.path().into()).unwrap().data.settings["second"],
        2
    );
    std::fs::write(dir.path().join("library.json"), "{broken").unwrap();
    assert!(Store::open(dir.path().into()).is_err());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("library.json")).unwrap(),
        "{broken"
    );
}
#[test]
fn engine_roundtrip_and_failed_import_preserves_library() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = Engine::open(dir.path().into()).unwrap();
    let track = json!({"id":"youtube:test","title":"Test","source":"https://www.youtube.com/watch?v=test","platform":"youtube"});
    e.dispatch(&json!({"command":"library.add","params":{"track":track}}))
        .unwrap();
    let playlist = e
        .dispatch(
            &json!({"command":"playlist.save","params":{"name":"Mix","tracks":["youtube:test"]}}),
        )
        .unwrap();
    e.dispatch(&json!({"command":"favorites.set","params":{"id":"youtube:test"}}))
        .unwrap();
    let backup = dir.path().join("backup.json");
    e.dispatch(&json!({"command":"backup.export","params":{"path":backup}}))
        .unwrap();
    e.dispatch(&json!({"command":"library.remove","params":{"id":"youtube:test"}}))
        .unwrap();
    assert!(
        e.store.data.playlists[playlist["id"].as_str().unwrap()]
            .tracks
            .is_empty()
    );
    e.dispatch(&json!({"command":"backup.import","params":{"path":backup}}))
        .unwrap();
    assert_eq!(e.store.data.favorites.len(), 1);
    std::fs::write(&backup, "{\"schema_version\":999}").unwrap();
    assert!(
        e.dispatch(&json!({"command":"backup.import","params":{"path":backup}}))
            .is_err()
    );
    assert_eq!(e.store.data.tracks.len(), 1);
    drop(e);
    assert_eq!(
        Engine::open(dir.path().into())
            .unwrap()
            .store
            .data
            .tracks
            .len(),
        1
    );
}
#[test]
fn effects_reject_nan_and_out_of_range() {
    let mut effects = Effects {
        speed: 0.25,
        pitch: 24.,
        ..Default::default()
    };
    assert!(media::filter(&effects).unwrap().contains("atempo=0.5"));
    effects.volume = f32::NAN;
    assert!(effects.validate().is_err());
    effects.volume = 0.5;
    effects.reverb = 101.;
    assert!(effects.validate().is_err());
}
#[test]
fn url_validation_blocks_local_and_unexpected_sources() {
    for u in [
        "file:///etc/passwd",
        "https://127.0.0.1/",
        "https://youtube.com.evil.test/",
        "https://user:pass@youtube.com/watch?v=x",
    ] {
        assert!(riff_core::provider::validate_url(u).is_err());
    }
    assert!(riff_core::provider::validate_url("https://music.youtube.com/watch?v=x").is_ok());
}

#[test]
fn background_jobs_report_completion_without_blocking_state_queries() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = Engine::open(dir.path().into()).unwrap();
    let result=e.dispatch(&json!({"command":"job.submit","params":{"operation":"library.scan","params":{"paths":[]}}})).unwrap();
    let id = result["job_id"].as_str().unwrap();
    e.dispatch(&json!({"command":"player.state"})).unwrap();
    let mut complete = false;
    for _ in 0..100 {
        let v = e
            .dispatch(&json!({"command":"job.get","params":{"id":id}}))
            .unwrap();
        if v["state"] == "completed" {
            complete = true;
            assert!(v["data"]["tracks"].as_array().unwrap().is_empty());
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(complete);
    assert!(
        e.dispatch(
            &json!({"command":"job.submit","params":{"operation":"backup.import","params":{}}})
        )
        .is_err()
    );
}

#[test]
fn yrc_parses_word_end_times_and_theme_paths_stay_in_directory() {
    let lines = lyrics::parse_yrc("[1000,1500](1000,500,0)Xin (1500,1000,0)chào");
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].words[0].end, Some(1.5));
    assert_eq!(lines[0].text, "Xin chào");
    let dir = tempfile::tempdir().unwrap();
    let mut e = Engine::open(dir.path().into()).unwrap();
    assert!(
        e.dispatch(
            &json!({"command":"themes.save","params":{"id":"../outside","name":"bad","tokens":{}}})
        )
        .is_err()
    );
    e.dispatch(&json!({"command":"themes.save","params":{"id":"dark","name":"Dark","tokens":{"background":"#121214"}}})).unwrap();
    let themes = e.dispatch(&json!({"command":"themes.list"})).unwrap();
    assert_eq!(themes[0]["tokens"]["background"], "#121214");
    e.dispatch(&json!({"command":"themes.delete","params":{"id":"dark"}}))
        .unwrap();
}
#[test]
fn oauth_verifies_state_and_loopback_callback_without_network() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = Engine::open(dir.path().into()).unwrap();
    assert!(e.dispatch(&json!({"command":"oauth.begin","params":{"provider":"google","config":{"client_id":"test","redirect_uri":"https://evil.test/callback"}}})).is_err());
    let result=e.dispatch(&json!({"command":"oauth.begin","params":{"provider":"google","config":{"client_id":"test","redirect_uri":"http://127.0.0.1:9000/"}}})).unwrap();
    let url = url::Url::parse(result["authorization_url"].as_str().unwrap()).unwrap();
    assert!(
        url.query_pairs()
            .any(|(k, v)| k == "code_challenge_method" && v == "S256")
    );
    assert!(e.dispatch(&json!({"command":"oauth.finish","params":{"provider":"google","state":"wrong","code":"x"}})).unwrap_err().to_string().contains("state mismatch"));
}

#[test]
fn karaoke_estimates_words_between_lines() {
    let lines = lyrics::parse("[00:05.00]Go now, please\n[00:08.00]Next line");
    let words = lyrics::estimate_word_timings(&lines[0], lines.get(1), 20.);
    assert_eq!(
        words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>(),
        ["Go", "now,", "please"]
    );
    assert_eq!(words[0].time, 5.);
    assert!(
        words
            .windows(2)
            .all(|pair| pair[0].end == Some(pair[1].time))
    );
    assert!(words[2].end.unwrap() < 8.);
    let cjk = lyrics::parse("[00:01.00]日本語");
    assert_eq!(lyrics::estimate_word_timings(&cjk[0], None, 5.).len(), 3);
}

#[test]
fn karaoke_merges_matching_words_but_rejects_other_recordings() {
    let mut lines = lyrics::parse("[00:05.00]Go now\n[00:20.00]Go now");
    let timed = lyrics::parse_yrc("[5200,1200](5200,600,0)Go (5800,600,0)now");
    lyrics::merge_word_timings(&mut lines, &timed);
    assert_eq!(lines[0].words.len(), 2);
    assert!((lines[0].words[0].time - 5.).abs() < 0.001);
    assert!(lines[1].words.is_empty());
}

#[test]
fn account_merge_keeps_deletions_from_coming_back() {
    use riff_core::{account_music::Snapshot, model::{Library, Playlist, Track}};
    let track = |n: &str| Track {
        id: format!("ytmusic:{n}"), title: n.into(), artist: String::new(), album: String::new(), duration: 1.0,
        source: format!("https://music.youtube.com/watch?v={n}"), platform: "ytmusic".into(), cover: None,
        local_path: None, cache_path: None,
    };
    let cloud = |songs: &[&str]| {
        let mut s = Snapshot::default();
        for n in songs {
            let t = track(n);
            s.favorites.push(t.id.clone());
            s.tracks.insert(t.id.clone(), t);
        }
        s.playlists.insert("p".into(), Playlist { id: "p".into(), name: "P".into(), tracks: s.favorites.clone(), cover: 0 });
        s
    };
    let mut library = Library::default();
    cloud(&["a", "b"]).merge_into(&mut library);
    assert_eq!(library.favorites.len(), 2);
    // Deleted here, cloud still has it: stays deleted.
    library.favorites.retain(|id| id != "ytmusic:a");
    library.playlists.get_mut("p").unwrap().tracks.retain(|id| id != "ytmusic:a");
    cloud(&["a", "b"]).merge_into(&mut library);
    assert_eq!(library.favorites, vec!["ytmusic:b"]);
    assert_eq!(library.playlists["p"].tracks, vec!["ytmusic:b"]);
    // Deleted on another device: goes here too.
    cloud(&[]).merge_into(&mut library);
    assert!(library.favorites.is_empty());
    assert!(library.playlists["p"].tracks.is_empty());
}
