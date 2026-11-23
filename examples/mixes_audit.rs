//! Prints the Mixes built from a data directory's listening history (read-only copy recommended).
//! Run: cargo run --release --example mixes_audit --features desktop -- <data dir>
use riff_core::Engine;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    let root = std::env::args().nth(1).expect("data directory");
    let mut e = Engine::open(root.into())?;
    let started = Instant::now();
    let job = e.dispatch(&json!({"command":"music.mixes","params":{}}))?;
    let data: Value = loop {
        e.tick()?;
        let status = e.dispatch(&json!({"command":"job.get","params":{"id":job["job_id"]}}))?;
        match status["state"].as_str() {
            Some("completed") => break status["data"].clone(),
            Some("failed") => anyhow::bail!("{}", status["error"]),
            _ => {}
        }
        anyhow::ensure!(started.elapsed() < Duration::from_secs(60), "timeout");
        std::thread::sleep(Duration::from_millis(20));
    };
    println!("built in {:.1}s", started.elapsed().as_secs_f64());
    for mix in data["mixes"].as_array().into_iter().flatten() {
        let tracks: Vec<String> = mix["tracks"]
            .as_array()
            .into_iter()
            .flatten()
            .take(6)
            .map(|t| format!("{} - {}", t["artist"].as_str().unwrap_or("?"), t["title"].as_str().unwrap_or("?")))
            .collect();
        println!("\n## {} ({} tracks)\n   {}", mix["name"].as_str().unwrap_or("?"), mix["tracks"].as_array().map_or(0, Vec::len), tracks.join("\n   "));
    }
    Ok(())
}
