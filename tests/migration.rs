use base64::Engine as _;
use serde_json::json;
#[test]
fn reads_account_created_by_original_node_crypto_format() {
    let fixture = "UkZBQwcHBwcHBwcHBwcHB6EOTjXRWGg60B4uOeb7FijcBAnjLdUGKz6hFc+bc7K2hq2g09wKowtsNXBwxcPiz+zZGP3X2njxA2L4Ovzia2IHSJQUbBJZePzrsZMENTc8tNWNIywfCwXbLjKXi823iw7s7X8KTuZF+BSq0NId+u+HZW0T48k72Gwn07h5ouOVBc1/pf4gPngyvBxUQSMT5jBrB4k7+P2Rl/CU4q/sqC+BjJx0KdZY7qOb10bcuO5gxj89zP1ci7/dCjSp6xT4";
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("account.rfacc");
    let mut bytes = base64::engine::general_purpose::STANDARD
        .decode(fixture)
        .unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let value = riff_core::migration::read(&path).unwrap();
    let (data, report) = riff_core::migration::convert(&value, &Default::default()).unwrap();
    assert_eq!(report["favorites"], 1);
    assert_eq!(data.favorites[0], "youtube:dQw4w9WgXcQ");
    let export = dir.path().join("export.rfacc");
    riff_core::migration::export(&export, &data).unwrap();
    let again = riff_core::migration::read(&export).unwrap();
    let (data2, _) = riff_core::migration::convert(&again, &Default::default()).unwrap();
    assert_eq!(data.favorites, data2.favorites);
    bytes[20] ^= 1;
    std::fs::write(&path, bytes).unwrap();
    assert!(riff_core::migration::read(&path).is_err());
}
#[test]
fn converts_plain_storage_without_erasing_existing_tracks() {
    let value = json!({"devsize_playlists":json!([{"id":"old","name":"Mix","tracks":[{"id":"dQw4w9WgXcQ","title":"Song","platform":"youtube"}]}]).to_string(),"riff_theme_mode":"dark"});
    let (data, report) = riff_core::migration::convert(&value, &Default::default()).unwrap();
    assert_eq!(report["playlists"], 1);
    assert_eq!(data.settings["riff_theme_mode"], "dark");
    assert_eq!(data.playlists["old"].tracks.len(), 1);
}
