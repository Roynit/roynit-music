use crate::model::{Library, PlayEvent, Playlist, Track};
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use anyhow::{Context, Result, ensure};
use flate2::{Compression, read::DeflateDecoder, write::DeflateEncoder};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub fn read(path: &Path) -> Result<Value> {
    ensure!(
        std::fs::metadata(path)?.len() <= 64 * 1024 * 1024,
        "Account file exceeds 64 MB"
    );
    let bytes = std::fs::read(path)?;
    if !bytes.starts_with(b"RFAC") {
        return Ok(serde_json::from_slice(&bytes)?);
    }
    ensure!(bytes.len() >= 32, "Truncated Riff account file");
    let key = Sha256::digest(b"riff-account-key-v1");
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|_| anyhow::anyhow!("Invalid account key"))?;
    let mut encrypted = bytes[32..].to_vec();
    encrypted.extend_from_slice(&bytes[16..32]);
    let compressed = cipher
        .decrypt(Nonce::from_slice(&bytes[4..16]), encrypted.as_ref())
        .map_err(|_| anyhow::anyhow!("Account authentication failed; file is damaged"))?;
    let mut decoded = Vec::new();
    DeflateDecoder::new(compressed.as_slice())
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut decoded)?;
    ensure!(
        decoded.len() <= 64 * 1024 * 1024,
        "Decompressed account exceeds 64 MB"
    );
    Ok(serde_json::from_slice(&decoded)?)
}
pub fn export(path: &Path, library: &Library) -> Result<()> {
    ensure!(!path.exists(), "Export file already exists");
    let map_track = |id: &str| {
        library.tracks.get(id).map(|t|json!({"id":t.id.trim_start_matches("youtube:").trim_start_matches("soundcloud:"),"title":t.title,"artist":t.artist,"album":t.album,"duration":t.duration,"platform":t.platform,"url":t.source,"thumbnail":t.cover,"path":t.local_path}))
    };
    let playlists:Vec<_>=library.playlists.values().map(|p|json!({"id":p.id,"name":p.name,"tracks":p.tracks.iter().filter_map(|id|map_track(id)).collect::<Vec<_>>()})).collect();
    let favorites: Vec<_> = library
        .favorites
        .iter()
        .filter_map(|id| map_track(id))
        .collect();
    let history: Vec<_> = library
        .history
        .iter()
        .filter_map(|h| map_track(&h.track_id))
        .collect();
    let local: Vec<_> = library
        .tracks
        .values()
        .filter(|t| t.local_path.is_some())
        .filter_map(|t| map_track(&t.id))
        .collect();
    let mut settings = serde_json::Map::new();
    for (k, v) in &library.settings {
        settings.insert(
            k.clone(),
            Value::String(if let Some(s) = v.as_str() {
                s.into()
            } else {
                v.to_string()
            }),
        );
    }
    for (k, v) in [
        ("devsize_playlists", json!(playlists)),
        ("devsize_favorites", json!(favorites)),
        ("devsize_history", json!(history)),
        ("devsize_local_tracks", json!(local)),
    ] {
        settings.insert(k.into(), json!(v.to_string()));
    }
    let payload = json!({"app":"Riff","version":"1.0","localStorage":settings});
    let mut compressor = DeflateEncoder::new(Vec::new(), Compression::default());
    compressor.write_all(&serde_json::to_vec(&payload)?)?;
    let bytes = compressor.finish()?;
    let key = Sha256::digest(b"riff-account-key-v1");
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|_| anyhow::anyhow!("Invalid account key"))?;
    let nonce = rand::random::<[u8; 12]>();
    let encrypted = cipher
        .encrypt(Nonce::from_slice(&nonce), bytes.as_ref())
        .map_err(|_| anyhow::anyhow!("Cannot encode account"))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(b"RFAC")?;
    file.write_all(&nonce)?;
    file.write_all(&encrypted[encrypted.len() - 16..])?;
    file.write_all(&encrypted[..encrypted.len() - 16])?;
    file.as_file().sync_all()?;
    file.persist_noclobber(path).map_err(|e| e.error)?;
    Ok(())
}
pub fn convert(value: &Value, base: &Library) -> Result<(Library, Value)> {
    let raw = value
        .get("localStorage")
        .unwrap_or(value)
        .as_object()
        .context("Expected Riff storage.json or .rfacc localStorage object")?;
    let get = |key: &str| -> Value {
        raw.get(key)
            .map(|v| {
                if let Some(s) = v.as_str() {
                    serde_json::from_str(s).unwrap_or_else(|_| v.clone())
                } else {
                    v.clone()
                }
            })
            .unwrap_or(Value::Null)
    };
    let mut data = base.clone();
    let mut warnings = Vec::new();
    if raw.keys().any(|key| {
        key.starts_with("riff_theme_")
            || key == "riff_adv_reduce_motion"
            || key == "riff_font_scale"
    }) {
        match crate::extensions::legacy_appearance(value) {
            Ok(appearance) => {
                data.settings
                    .insert("appearance".into(), serde_json::to_value(appearance)?);
            }
            Err(error) => warnings.push(json!({"area":"appearance","error":error.to_string()})),
        }
    }

    for (k, v) in raw {
        data.settings.insert(k.clone(), v.clone());
    }
    let mut add = |v: &Value| -> Option<String> {
        match track(v) {
            Ok(t) => {
                let id = t.id.clone();
                data.tracks.insert(id.clone(), t);
                Some(id)
            }
            Err(e) => {
                warnings.push(json!({"title":v["title"],"error":e.to_string()}));
                None
            }
        }
    };
    let playlist_values = get("devsize_playlists");
    let mut playlists = Vec::new();
    for p in playlist_values.as_array().into_iter().flatten() {
        let ids = p["tracks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(&mut add)
            .collect();
        playlists.push(Playlist {
            id: p["id"]
                .as_str()
                .map(str::to_string)
                .or_else(|| p["id"].as_u64().map(|n| n.to_string()))
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            name: p["name"].as_str().unwrap_or("Imported playlist").into(),
            tracks: ids,
        });
    }
    let favorites = get("devsize_favorites");
    let favorites: Vec<_> = favorites
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(&mut add)
        .collect();
    let history = get("devsize_history");
    let history: Vec<_> = history
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| {
            let id = add(v)?;
            Some(PlayEvent {
                track_id: id,
                started_at: v["playedAt"].as_u64().unwrap_or(0) / 1000,
            })
        })
        .collect();
    for key in [
        "devsize_local_tracks",
        "riff_local_tracks",
        "devsize_local_files",
    ] {
        let local = get(key);
        for value in local.as_array().into_iter().flatten() {
            add(value);
        }
    }
    for playlist in playlists {
        data.playlists.insert(playlist.id.clone(), playlist);
    }
    for id in favorites {
        if !data.favorites.contains(&id) {
            data.favorites.push(id);
        }
    }
    data.history.extend(history);
    let summary = json!({"tracks":data.tracks.len(),"playlists":data.playlists.len(),"favorites":data.favorites.len(),"warnings":warnings,"mode":"merge"});
    Ok((data, summary))
}
fn track(v: &Value) -> Result<Track> {
    let title = v["title"]
        .as_str()
        .or(v["name"].as_str())
        .context("Track has no title")?;
    let platform = v["platform"].as_str().unwrap_or("youtube");
    let raw_id = v["id"]
        .as_str()
        .map(str::to_string)
        .or_else(|| v["id"].as_u64().map(|n| n.to_string()))
        .unwrap_or_default();
    let source = v["url"].as_str().or(v["source"].as_str()).unwrap_or("");
    let raw_path = v["path"]
        .as_str()
        .or(v["filePath"].as_str())
        .or(v["localPath"].as_str());
    let path = raw_path.map(PathBuf::from).or_else(|| {
        url::Url::parse(source)
            .ok()
            .and_then(|u| u.to_file_path().ok())
    });
    let id = if platform == "local" {
        format!("local:{}", crate::store::key(raw_path.unwrap_or(source)))
    } else if raw_id.starts_with("youtube:") || raw_id.starts_with("soundcloud:") {
        raw_id.clone()
    } else {
        format!(
            "{}:{raw_id}",
            if platform == "soundcloud" {
                "soundcloud"
            } else {
                "youtube"
            }
        )
    };
    let source = if source.is_empty() && platform != "local" {
        format!("https://www.youtube.com/watch?v={raw_id}")
    } else {
        source.into()
    };
    ensure!(
        platform != "local" || path.is_some(),
        "Local track has no file path"
    );
    Ok(Track {
        id,
        title: title.into(),
        artist: v["artist"].as_str().unwrap_or("").into(),
        album: v["album"].as_str().unwrap_or("").into(),
        duration: v["duration"]
            .as_f64()
            .filter(|d| d.is_finite() && *d >= 0.)
            .unwrap_or(0.),
        source,
        platform: platform.into(),
        cover: v["cover"]
            .as_str()
            .or(v["thumbnail"].as_str())
            .map(str::to_string),
        local_path: path,
        cache_path: None,
    })
}
