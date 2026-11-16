use serde_json::{Value, json};
fn main() -> anyhow::Result<()> {
    let client = riff_core::network::Config::default().client()?;
    let mut report = Vec::new();
    for (name, url) in [
        ("deezer", "https://www.deezer.com/playlist/908622995"),
        (
            "apple_album",
            "https://music.apple.com/us/album/25/1544494115",
        ),
        (
            "apple_playlist",
            "https://music.apple.com/us/playlist/todays-hits/pl.f4d106fed2bd41149aaacabb233eb5eb",
        ),
    ] {
        let result = match riff_core::catalog::metadata(&client, url, None) {
            Ok(v) => {
                json!({"service":name,"reachable":true,"tracks":v["tracks"].as_array().map(Vec::len),"partial":v["partial"],"total":v["total"]})
            }
            Err(e) => json!({"service":name,"reachable":false,"error":e.to_string()}),
        };
        println!("{result}");
        report.push(result);
    }
    let result = match riff_core::translation::fetch(
        &client,
        std::path::Path::new("riff/rust/test-output/translation-audit"),
        &json!({"text":"I like music.","target":"vi","source":"en","provider":"mymemory"}),
    ) {
        Ok(v) => {
            json!({"service":"translation","reachable":true,"characters":v["text"].as_str().map(str::len)})
        }
        Err(e) => json!({"service":"translation","reachable":false,"error":e.to_string()}),
    };
    println!("{result}");
    report.push(result);
    for source in ["lrclib", "netease", "kugou", "musixmatch", "genius"] {
        let result = match riff_core::lyric_providers::fetch(
            &client, source, "Hello", "Adele", 295.,
        ) {
            Ok(v) => {
                json!({"service":source,"reachable":true,"characters":v["text"].as_str().map(str::len),"lines":v["lines"].as_array().map(Vec::len)})
            }
            Err(e) => json!({"service":source,"reachable":false,"error":e.to_string()}),
        };
        println!("{result}");
        report.push(result);
    }
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "riff/rust/test-output/service-audit.json".into());
    riff_core::store::write_json(std::path::Path::new(&path), &Value::Array(report))?;
    Ok(())
}
