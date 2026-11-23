//! Times "press Play" until audio is flowing: once cold, once after the warm-up the app does at start.
//! Needs network, FFmpeg and an audio device. Run: cargo run --release --example first_play_audit --features desktop
use riff_core::Engine;
use serde_json::json;
use std::time::{Duration, Instant};

fn play_and_time(e: &mut Engine, id: &str) -> anyhow::Result<f64> {
    let started = Instant::now();
    let t = Instant::now();
    e.dispatch(&json!({"command":"player.request","params":{"id":id}}))?;
    println!("  dispatch took {:.2}s", t.elapsed().as_secs_f64());
    loop {
        let t = Instant::now();
        e.tick()?;
        if t.elapsed() > Duration::from_millis(200) {
            println!("  tick blocked {:.2}s at {:.2}s", t.elapsed().as_secs_f64(), started.elapsed().as_secs_f64());
        }
        let s = e.dispatch(&json!({"command":"player.state","params":{}}))?;
        if s["playing"] == true && s["buffering"] == false {
            let secs = started.elapsed().as_secs_f64();
            e.dispatch(&json!({"command":"player.stop","params":{}}))?;
            return Ok(secs);
        }
        anyhow::ensure!(started.elapsed() < Duration::from_secs(30), "never started");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn main() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let mut e = Engine::open(root.path().into())?;
    let mut ids = ["dQw4w9WgXcQ", "9bZkp7q19f0", "kJQP7kiw5Fk"];
    let lengths = [213.0, 252.0, 282.0];
    let mut lengths = lengths;
    if std::env::args().any(|a| a == "--reverse") {
        ids.reverse();
        lengths.reverse();
    }
    let mut tracks = vec![];
    for (n, id) in ids.iter().enumerate() {
        let track = json!({"id":format!("youtube:{id}"),"title":id,"artist":"x","album":"","duration":lengths[n],
            "source":format!("https://music.youtube.com/watch?v={id}"),"platform":"ytmusic","cover":null});
        e.dispatch(&json!({"command":"library.add","params":{"track":track}}))?;
        tracks.push(format!("youtube:{id}"));
    }
    let warm = std::env::args().any(|a| a == "--warm");
    if warm {
        riff_core::warm_up();
        riff_core::warm_streams(ids.iter().map(|id| format!("https://music.youtube.com/watch?v={id}")).collect());
        std::thread::sleep(Duration::from_secs(5));
    }
    for (i, id) in tracks.iter().enumerate() {
        println!("{} play {} -> audio after {:.2}s", if warm { "warm" } else { "cold" }, i + 1, play_and_time(&mut e, id)?);
    }
    Ok(())
}
