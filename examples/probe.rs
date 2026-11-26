use riff_core::youtube::Music;
use serde_json::Value;
fn main() {
    let client = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(20)).build().unwrap();
    let music = Music { client: &client, cookies: None };
    let args: Vec<String> = std::env::args().collect();
    let out: Value = match args[1].as_str() {
        "browse" => music.browse(&args[2], args.get(3).map(String::as_str)).unwrap(),
        "search" => music.search_filtered(&args[2], &args[3]).unwrap(),
        "podcast" => music.podcast(&args[2]).unwrap(),
        "get" => {
            use riff_core::{model::Track, provider::Providers};
            let track = Track { id: args[2].clone(), title: String::new(), artist: String::new(), album: String::new(), duration: 0., source: format!("https://music.youtube.com/watch?v={}", args[2]), platform: "ytmusic".into(), cover: None, local_path: None, cache_path: None };
            let v = Providers { cookies: None, proxy: None, browser: None }.resolve(&track).unwrap_or_else(|e| panic!("{e:#} [{}]", riff_core::youtube_stream::last_notes()));
            println!("notes: {}", riff_core::youtube_stream::last_notes());
            let url = v["url"].as_str().unwrap().to_owned();
            println!("url {url}");
            let t = std::time::Instant::now();
            let mut r = client.get(&url).send().unwrap();
            println!("status {} len {:?}", r.status(), r.content_length());
            let mut total = 0u64; let mut buf = [0u8; 65536];
            loop { match std::io::Read::read(&mut r, &mut buf) { Ok(0) => break, Ok(n) => total += n as u64, Err(e) => { println!("ERR after {total}: {e}"); break } } }
            println!("read {total} in {:?}", t.elapsed());
            return;
        }
        "src" => {
            let v = match args[3].as_str() {
                "youtube" => riff_core::sources::youtube_search(&client, &args[2]),
                "soundcloud" => riff_core::sources::soundcloud_search(&client, &args[2], 5),
                _ => riff_core::sources::spotify_search(&client, &args[2], 5),
            };
            match v { Ok(v) => { println!("count {}", v["tracks"].as_array().map_or(0, Vec::len)); for t in v["tracks"].as_array().into_iter().flatten().take(3) { println!("{} | {} | {} | {} | {}", t["id"], t["title"], t["artist"], t["duration"], t["source"]); } }, Err(e) => println!("ERR {e:#}") }
            return;
        }
        "sc" => { println!("{:?}", riff_core::sources::soundcloud_resolve(&client, &args[2]).map(|v| v["url"].as_str().unwrap_or("").chars().take(60).collect::<String>())); return; }
        "match" => { println!("{:?}", riff_core::sources::match_video(&music, &args[2], &args[3], args[4].parse().unwrap())); return; }
        "raw" => { let v = music.browse_raw(&args[2]).unwrap(); dump(&v, "", 0); return; }
        _ => panic!(),
    };
    let items = out["items"].as_array().cloned().unwrap_or_default();
    println!("tracks={} items={}", out["tracks"].as_array().map_or(0, Vec::len), items.len());
    for t in out["tracks"].as_array().into_iter().flatten().take(4) { println!("T {} | {} | {}", t["id"], t["title"], t["source"]); }
    for i in items.iter().take(12) { println!("I {} | {} | {} | {}", i["kind"], i["id"], i["title"], i["subtitle"]); }
}

fn dump(v: &Value, path: &str, depth: usize) {
    match v {
        Value::Object(m) => for (k, x) in m { if k == "videoId" || k == "title" && x.is_object() || k=="text" || k == "browseId" { if let Some(t) = x.as_str() { println!("{path}/{k} = {t}"); } } dump(x, &format!("{path}/{k}"), depth + 1); },
        Value::Array(a) => for (i, x) in a.iter().enumerate().take(6) { dump(x, &format!("{path}[{i}]"), depth + 1); },
        _ => {}
    }
}
