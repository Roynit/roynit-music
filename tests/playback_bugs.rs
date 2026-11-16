use riff_core::{
    Engine,
    model::Track,
    provider::{map_track, validate_match_duration},
    spotify, youtube,
};
use serde_json::json;
fn track(title: &str, duration: f64) -> Track {
    Track {
        id: "spotify:test".into(),
        title: title.into(),
        artist: "Justin Bieber, Nicki Minaj".into(),
        album: String::new(),
        duration,
        source: "https://open.spotify.com/track/190jyVPHYjAqEaOGmMzdyk".into(),
        platform: "spotify".into(),
        cover: None,
        local_path: None,
        cache_path: None,
    }
}
#[test]
fn missing_search_durations_cannot_choose_a_dub_mix_or_spoken_commentary() {
    let requested = track("Beauty And A Beat", 227.986);
    let studio = track("Beauty And A Beat (feat. Nicki Minaj)", 0.);
    let dub = track(
        "Beauty And A Beat (Steven Redant Beauty and The Vocal Dub Mix) (feat. Nicki Minaj)",
        0.,
    );
    let spoken = track("Beauty And A Beat | The Real Meaning Explained", 0.);
    let candidates = [studio, dub, spoken];
    assert_eq!(
        spotify::best_match(&requested, &candidates).unwrap().title,
        candidates[0].title
    );
    assert!(spotify::best_match(&requested, &candidates[1..]).is_none());
    assert!(validate_match_duration(&requested, &json!({"duration":443})).is_err());
    assert!(validate_match_duration(&requested, &json!({"duration":228})).is_ok());
    assert!(validate_match_duration(&requested, &json!({"duration":null})).is_err());
    assert!(
        validate_match_duration(
            &track("Beauty And A Beat", 144.693),
            &json!({"duration":228})
        )
        .is_err()
    );
}
#[test]
fn matching_preserves_provider_order_on_ties_and_remaster_titles() {
    let requested = track("Beauty And A Beat", 228.);
    let candidates = [
        track("Beauty And A Beat", 0.),
        track("Beauty And A Beat", 0.),
    ];
    assert!(std::ptr::eq(
        spotify::best_match(&requested, &candidates).unwrap(),
        &candidates[0]
    ));
    assert!(
        spotify::best_match(
            &track("Beauty And A Beat - 2022 Remaster", 228.),
            &candidates
        )
        .is_some()
    );
    assert!(
        spotify::best_match(&track("Beauty And A Beat (Dub Mix)", 443.), &candidates).is_none()
    );
}
#[test]
fn soundcloud_flat_entries_keep_their_provider() {
    let t=map_track(&json!({"id":"319734663","title":"Song","url":"https://soundcloud.com/artist/song","ie_key":"Soundcloud","duration":233})).unwrap();
    assert_eq!(t.platform, "soundcloud");
    assert_eq!(t.id, "soundcloud:319734663");
    let t = map_track(
        &json!({"id":"89762318","title":"Song","webpage_url":"https://soundcloud.com/artist/song"}),
    )
    .unwrap();
    assert_eq!(t.platform, "soundcloud");
}
#[test]
fn youtube_music_reads_song_time_from_tertiary_columns_not_view_counts() {
    let row = json!({"musicResponsiveListItemRenderer":{"playlistItemData":{"videoId":"abcdefghijk"},"flexColumns":[
        {"musicResponsiveListItemFlexColumnRenderer":{"text":{"runs":[{"text":"Song"}]}}},
        {"musicResponsiveListItemFlexColumnRenderer":{"text":{"runs":[{"text":"Artist"}]}}},
        {"musicResponsiveListItemFlexColumnRenderer":{"text":{"runs":[{"text":"3:48"}]}}}
    ]}});
    assert_eq!(youtube::parse_tracks(&row)[0].duration, 228.);
    let mut no_time = row;
    no_time["musicResponsiveListItemRenderer"]["flexColumns"][2]["musicResponsiveListItemFlexColumnRenderer"]
        ["text"]["runs"][0]["text"] = json!("443");
    assert_eq!(youtube::parse_tracks(&no_time)[0].duration, 0.);
}
#[test]
fn replacing_queue_cancels_pending_play_and_repairs_old_soundcloud_tags() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = Engine::open(dir.path().into()).unwrap();
    let mut t = track("Local", 30.);
    t.id = "local:test".into();
    t.local_path = Some(dir.path().join("missing.wav"));
    e.dispatch(&json!({"command":"library.add","params":{"track":t}}))
        .unwrap();
    let job = e
        .dispatch(&json!({"command":"player.request","params":{"id":"local:test"}}))
        .unwrap();
    e.dispatch(&json!({"command":"queue.set","params":{"tracks":[]}}))
        .unwrap();
    for _ in 0..30 {
        e.tick().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let status = e
        .dispatch(&json!({"command":"job.get","params":{"id":job["job_id"]}}))
        .unwrap();
    assert_eq!(status["state"], "cancelled");
    assert!(e.player.track_id.is_none());
    let mut old = track("Legacy SoundCloud", 233.);
    old.id = "youtube:319734663".into();
    old.platform = "youtube".into();
    old.source = "https://soundcloud.com/artist/song".into();
    e.dispatch(&json!({"command":"library.add","params":{"track":old}}))
        .unwrap();
    drop(e);
    let e = Engine::open(dir.path().into()).unwrap();
    assert_eq!(
        e.store.data.tracks["youtube:319734663"].platform,
        "soundcloud"
    );
}

#[test]
#[ignore = "Checks real Spotify-to-YouTube recording resolution against the reported 3m/7m bug"]
fn real_beauty_and_a_beat_is_not_the_seven_minute_dub() {
    let providers = riff_core::provider::Providers {
        cookies: None,
        proxy: None,
        browser: None,
    };
    let requested = track("Beauty And A Beat", 227.986);
    let resolved = providers.resolve(&requested).unwrap();
    assert_eq!(resolved["verified_duration"], true);
    assert!(spotify::duration_matches(
        requested.duration,
        resolved["duration"].as_f64().unwrap()
    ));
    assert!(
        !resolved["matched_title"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("dub")
    );
    println!(
        "Requested {:.3}s, verified {}s: {}",
        requested.duration, resolved["duration"], resolved["matched_title"]
    );
    // The shorter Spotify edition must never silently play the 228-second one.
    let shorter = track("Beauty And A Beat", 144.693);
    if let Ok(value) = providers.resolve(&shorter) {
        assert!(spotify::duration_matches(
            shorter.duration,
            value["duration"].as_f64().unwrap()
        ));
    }
}

#[test]
fn preparing_search_result_does_not_add_it_to_library() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = Engine::open(dir.path().into()).unwrap();
    let mut t = track("An unsaved search result", 30.);
    // Local warmup completes without networking or opening an audio device.
    t.local_path = Some(dir.path().join("unused.wav"));
    let job = e
        .dispatch(&json!({"command":"stream.prepare","params":{"track":t}}))
        .unwrap();
    for _ in 0..50 {
        e.tick().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let status = e
        .dispatch(&json!({"command":"job.get","params":{"id":job["job_id"]}}))
        .unwrap();
    assert_eq!(status["state"], "completed");
    assert!(e.store.data.tracks.is_empty());
    assert!(e.player.track_id.is_none());
    assert!(e.store.data.queue.tracks.is_empty());
}
