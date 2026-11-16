use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::path::Path;
pub fn detect_source(text: &str) -> Option<&'static str> {
    if text.chars().any(|c| ('ぁ'..='ヿ').contains(&c)) {
        return Some("ja");
    }
    if text.chars().any(|c| ('가'..='힣').contains(&c)) {
        return Some("ko");
    }
    if text.chars().any(|c| ('一'..='鿿').contains(&c)) {
        return Some("zh");
    }
    if text.chars().any(|c| ('А'..='я').contains(&c)) {
        return Some("ru");
    }
    if text
        .chars()
        .any(|c| "ăâđêôơưĂÂĐÊÔƠƯạảấầẩẫậắằẳẵặẹẻẽếềểễệịỉĩọỏốồổỗộớờởỡợụủũứừửữựỳỵỷỹ".contains(c))
    {
        return Some("vi");
    }
    let english = text
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| {
            [
                "i", "you", "the", "and", "with", "are", "your", "this", "that", "love", "like",
                "hello", "my",
            ]
            .contains(&w.to_lowercase().as_str())
        })
        .count();
    (english >= 2).then_some("en")
}
pub fn chunks(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        if current.len() + c.len_utf8() > 450 {
            chunks.push(std::mem::take(&mut current));
        }
        current.push(c);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}
pub fn fetch(client: &reqwest::blocking::Client, root: &Path, params: &Value) -> Result<Value> {
    let text = params["text"]
        .as_str()
        .context("Missing translation text")?;
    ensure!(
        !text.trim().is_empty() && text.len() <= 10000,
        "Translation text must be 1..10000 bytes"
    );
    let target = params["target"]
        .as_str()
        .context("Missing target language")?;
    let source = params["source"].as_str().unwrap_or("auto");
    ensure!(
        [source, target]
            .iter()
            .all(|s| s.len() <= 16 && s.chars().all(|c| c.is_ascii_alphabetic() || c == '-')),
        "Invalid language code"
    );
    let provider = params["provider"].as_str().unwrap_or("auto");
    ensure!(
        ["auto", "google", "mymemory"].contains(&provider),
        "Unknown translation provider"
    );
    let cache = root.join("translations").join(format!(
        "{}.json",
        crate::store::key(&format!("{source}|{target}|{provider}|{text}"))
    ));
    if cache.is_file() {
        let mut value: Value = serde_json::from_reader(std::fs::File::open(cache)?)?;
        value["cached"] = json!(true);
        return Ok(value);
    }
    if provider != "mymemory" {
        match crate::services::translate(client, params) {
            Ok(mut value) => {
                value["provider"] = json!("google");
                crate::store::write_json(&cache, &value)?;
                return Ok(value);
            }
            Err(error) if provider == "google" => return Err(error),
            Err(_) => {}
        }
    }
    let source = if source == "auto" {
        detect_source(text).context("Translation service unavailable. Select the source language to use the fallback provider.")?
    } else {
        source
    };
    let mut output = String::new();
    if source == target {
        output = text.into();
    } else {
        for chunk in chunks(text) {
            crate::jobs::check_cancelled()?;
            let result: Value = client
                .get("https://api.mymemory.translated.net/get")
                .query(&[("q", chunk), ("langpair", format!("{source}|{target}"))])
                .send()?
                .error_for_status()?
                .json()?;
            ensure!(
                result["responseStatus"].as_u64() == Some(200) && result["quotaFinished"] != true,
                "Fallback translation quota or service error; retry later"
            );
            let translated = result["responseData"]["translatedText"]
                .as_str()
                .context("Fallback returned no translation")?;
            let fragment = scraper::Html::parse_fragment(translated);
            output.push_str(&fragment.root_element().text().collect::<String>());
        }
    }
    let value =
        json!({"text":output,"source":source,"target":target,"provider":"mymemory","cached":false});
    crate::store::write_json(&cache, &value)?;
    Ok(value)
}
