use riff_core::{Engine, model::Track};
use serde_json::json;
use std::time::{Duration, Instant};
fn main() -> anyhow::Result<()> {
    let seconds = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(120)
        .clamp(10, 3600);
    let root = tempfile::tempdir()?;
    let file = root.path().join("fixture.wav");
    riff_core::tools::run(
        "ffmpeg",
        [
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=8",
            file.to_str().unwrap(),
        ],
        Duration::from_secs(10),
    )?;
    let mut engine = Engine::open(root.path().into())?;
    for i in 0..2000 {
        let track: Track = serde_json::from_value(
            json!({"id":format!("local:{i}"),"title":format!("Fixture {i}"),"artist":"Controlled soak test","source":file,"local_path":file,"platform":"local","duration":8.}),
        )?;
        engine.store.data.tracks.insert(track.id.clone(), track);
    }
    engine.store.save()?;
    engine.dispatch(&json!({"command":"player.effects","params":{"muted":true}}))?;
    let ids: Vec<_> = (0..50).map(|i| format!("local:{i}")).collect();
    engine.dispatch(&json!({"command":"queue.play","params":{"tracks":ids}}))?;
    let started = Instant::now();
    let mut last_action = Instant::now();
    let mut last_report = Instant::now();
    let mut actions = 0;
    let mut pcm_observations = 0;
    let mut elapsed_samples = Vec::new();
    while started.elapsed() < Duration::from_secs(seconds) {
        let tick = Instant::now();
        if let Some(event) = engine.tick()? {
            anyhow::ensure!(
                !event["event"]
                    .as_str()
                    .is_some_and(|s| s.ends_with("_error")),
                "Unexpected playback failure: {event}"
            );
        }
        let state = engine.snapshot();
        if state["playing"] == true {
            pcm_observations += 1;
        }
        if state["pending_track_id"].is_null()
            && let Some(id) = engine.player.track_id.as_deref()
        {
            anyhow::ensure!(
                engine.store.data.queue.current() == Some(id),
                "Queue/playing identity mismatch"
            );
        }
        elapsed_samples.push(tick.elapsed().as_secs_f64() * 1000.);
        if last_action.elapsed() > Duration::from_millis(500) {
            actions += 1;
            last_action = Instant::now();
            match actions % 12 {
                0 => {
                    engine.dispatch(&json!({"command":"player.pause"}))?;
                }
                1 => {
                    if engine.player.track_id.is_some() {
                        engine.dispatch(&json!({"command":"player.resume"}))?;
                    }
                }
                2 => {
                    if engine.player.track_id.is_some() {
                        engine
                            .dispatch(&json!({"command":"player.seek","params":{"seconds":0.1}}))?;
                    }
                }
                3 => {
                    engine.dispatch(&json!({"command":"jobs.cancel_all"}))?;
                }
                _ => {
                    if engine.store.data.queue.index.is_none_or(|i| i >= 48) {
                        engine
                            .dispatch(&json!({"command":"queue.play","params":{"tracks":ids}}))?;
                    } else {
                        engine.dispatch(&json!({"command":"player.next.request"}))?;
                    }
                }
            }
        }
        if last_report.elapsed() > Duration::from_secs(20) {
            println!(
                "{}",
                json!({"elapsed_s":started.elapsed().as_secs(),"actions":actions,"pcm_observations":pcm_observations})
            );
            last_report = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    engine.dispatch(&json!({"command":"player.stop"}))?;
    engine.persist_session()?;
    let count = engine.store.data.tracks.len();
    drop(engine);
    let reopened = Engine::open(root.path().into())?;
    anyhow::ensure!(
        reopened.store.data.tracks.len() == count,
        "Library lost records on reopen"
    );
    elapsed_samples.sort_by(f64::total_cmp);
    let p95 = elapsed_samples[elapsed_samples.len() * 95 / 100];
    anyhow::ensure!(pcm_observations > 20, "No sustained native playback");
    println!(
        "{}",
        json!({"complete":true,"seconds":seconds,"tracks":count,"actions":actions,"pcm_observations":pcm_observations,"tick_p95_ms":p95,"tick_max_ms":elapsed_samples.last(),"reopened":true})
    );
    Ok(())
}
