use riff_core::Engine;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
fn finish(e: &mut Engine, job: Value) -> anyhow::Result<Value> {
    let start = Instant::now();
    loop {
        e.tick()?;
        let status = e.dispatch(&json!({"command":"job.get","params":{"id":job["job_id"]}}))?;
        if status["state"] == "completed" {
            return Ok(status["data"].clone());
        }
        if status["state"] == "failed" {
            anyhow::bail!("{}", status["error"]);
        }
        anyhow::ensure!(
            start.elapsed() < Duration::from_secs(90),
            "benchmark timeout"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn main() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let mut e = Engine::open(root.path().into())?;
    for (cmd, params) in [
        (
            "music.search",
            json!({"query":"Rick Astley Never Gonna Give You Up","filter":"songs"}),
        ),
        (
            "spotify.search",
            json!({"query":"Rick Astley Never Gonna Give You Up","limit":10}),
        ),
    ] {
        for round in 0..2 {
            let started = Instant::now();
            let job = e.dispatch(&json!({"command":cmd,"params":params}))?;
            let result = finish(&mut e, job)?;
            println!(
                "{}",
                json!({"operation":cmd,"round":round,"elapsed_ms":started.elapsed().as_secs_f64()*1000.,"tracks":result["tracks"].as_array().map(Vec::len)})
            );
        }
    }
    for limit in [30, 10] {
        let started = Instant::now();
        let tracks = e
            .providers
            .search("Rick Astley Never Gonna Give You Up", "youtube", limit)?;
        println!(
            "{}",
            json!({"operation":"youtube.search","limit":limit,"elapsed_ms":started.elapsed().as_secs_f64()*1000.,"tracks":tracks.len()})
        );
    }
    let id = "youtube:jNQXAC9IVRw";
    e.dispatch(&json!({"command":"library.add","params":{"track":{"id":id,"title":"Me at the zoo","source":"https://www.youtube.com/watch?v=jNQXAC9IVRw","platform":"youtube"}}}))?;
    e.dispatch(&json!({"command":"player.effects","params":{"muted":true}}))?;
    for round in 0..2 {
        let started = Instant::now();
        let job = e.dispatch(&json!({"command":"player.request","params":{"id":id}}))?;
        finish(&mut e, job)?;
        let prepared = started.elapsed().as_secs_f64() * 1000.;
        while e.player.position() < 0.01 {
            e.tick()?;
            anyhow::ensure!(
                started.elapsed() < Duration::from_secs(60),
                "audio startup timeout"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        println!(
            "{}",
            json!({"operation":"playback","round":round,"prepared_ms":prepared,"first_pcm_ms":started.elapsed().as_secs_f64()*1000.})
        );
        e.dispatch(&json!({"command":"player.stop"}))?;
    }
    Ok(())
}
