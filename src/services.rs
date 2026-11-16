use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Use the caller's own Last.fm application credentials. No upstream secrets are copied.
pub fn lastfm(client: &reqwest::blocking::Client, p: &Value) -> Result<Value> {
    let operation = p["operation"]
        .as_str()
        .context("Missing Last.fm operation")?;
    let key = p["api_key"].as_str().context("Missing Last.fm api_key")?;
    let secret = p["api_secret"]
        .as_str()
        .context("Missing Last.fm api_secret")?;
    let mut fields = BTreeMap::<String, String>::new();
    fields.insert("api_key".into(), key.into());
    let method = match operation {
        "token" => "auth.getToken",
        "session" => {
            fields.insert("token".into(), required(p, "token")?);
            "auth.getSession"
        }
        "now_playing" | "scrobble" => {
            fields.insert("sk".into(), required(p, "session_key")?);
            let suffix = if operation == "scrobble" { "[0]" } else { "" };
            fields.insert(format!("artist{suffix}"), required(p, "artist")?);
            fields.insert(format!("track{suffix}"), required(p, "track")?);
            if let Some(album) = p["album"].as_str() {
                fields.insert(format!("album{suffix}"), album.into());
            }
            if operation == "scrobble" {
                fields.insert(
                    "timestamp[0]".into(),
                    p["timestamp"]
                        .as_u64()
                        .context("Missing scrobble Unix timestamp")?
                        .to_string(),
                );
            }
            if operation == "scrobble" {
                "track.scrobble"
            } else {
                "track.updateNowPlaying"
            }
        }
        _ => anyhow::bail!("Last.fm operation must be token, session, now_playing or scrobble"),
    };
    fields.insert("method".into(), method.into());
    let signature = sign_lastfm(&fields, secret);
    fields.insert("api_sig".into(), signature);
    fields.insert("format".into(), "json".into());
    let result: Value = client
        .post("https://ws.audioscrobbler.com/2.0/")
        .form(&fields)
        .send()?
        .error_for_status()?
        .json()?;
    ensure!(
        result.get("error").is_none(),
        "Last.fm: {}",
        result["message"].as_str().unwrap_or("request failed")
    );
    if operation == "token" {
        let token = result["token"].as_str().context("Missing Last.fm token")?;
        let mut url = url::Url::parse("https://www.last.fm/api/auth/")?;
        url.query_pairs_mut()
            .extend_pairs([("api_key", key), ("token", token)]);
        return Ok(json!({"token":token,"authorization_url":url.as_str()}));
    }
    Ok(result)
}
pub fn sign_lastfm(fields: &BTreeMap<String, String>, secret: &str) -> String {
    let mut body = String::new();
    for (k, v) in fields {
        if !["format", "callback", "api_sig"].contains(&k.as_str()) {
            body.push_str(k);
            body.push_str(v);
        }
    }
    body.push_str(secret);
    format!("{:x}", md5::compute(body))
}
fn required(p: &Value, name: &str) -> Result<String> {
    Ok(p[name]
        .as_str()
        .filter(|x| !x.is_empty())
        .with_context(|| format!("Missing {name}"))?
        .into())
}

/// Unofficial translation endpoint, matching Riff's best-effort external lookup behavior.
pub fn translate(client: &reqwest::blocking::Client, p: &Value) -> Result<Value> {
    let text = p["text"].as_str().context("Missing text")?;
    ensure!(text.len() <= 10000, "Translation text too large");
    let target = p["target"].as_str().context("Missing target language")?;
    ensure!(
        target.len() <= 16 && target.chars().all(|c| c.is_ascii_alphabetic() || c == '-'),
        "Invalid language code"
    );
    let source = p["source"].as_str().unwrap_or("auto");
    let value: Value = client
        .get("https://translate.googleapis.com/translate_a/single")
        .query(&[
            ("client", "gtx"),
            ("sl", source),
            ("tl", target),
            ("dt", "t"),
            ("q", text),
        ])
        .send()?
        .error_for_status()?
        .json()?;
    let translated: String = value[0]
        .as_array()
        .context("Invalid translation response")?
        .iter()
        .filter_map(|row| row[0].as_str())
        .collect();
    Ok(json!({"text":translated,"source":value[2],"target":target}))
}
