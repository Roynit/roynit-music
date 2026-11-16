use riff_core::jobs::Jobs;
use serde_json::json;
use std::time::{Duration, Instant};
#[test]
fn cancelling_worker_reports_cancelled_and_does_not_publish_success() {
    let mut jobs = Jobs::default();
    let v = jobs
        .submit("test".into(), None, || {
            for _ in 0..100 {
                riff_core::jobs::check_cancelled()?;
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(json!({"should_not_commit":true}))
        })
        .unwrap();
    let id = v["job_id"].as_str().unwrap();
    jobs.cancel(id).unwrap();
    let start = Instant::now();
    loop {
        let completed = jobs.drain();
        if let Some(job) = completed.first() {
            assert!(job.result.is_err());
            jobs.complete(job);
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(2));
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(jobs.get(id).unwrap()["state"], "cancelled");
    assert!(jobs.cancel(id).is_err());
}
#[test]
#[cfg(windows)]
fn dpapi_vault_roundtrip_is_encrypted_and_rejects_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oauth.dpapi");
    let secret = json!({"tokens":{"test":"fake-access-token-for-test-only"}});
    riff_core::vault::save(&path, &secret).unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("fake-access-token"));
    assert_eq!(riff_core::vault::load(&path).unwrap().unwrap(), secret);
    let n = bytes.len() - 1;
    bytes[n] ^= 1;
    std::fs::write(&path, bytes).unwrap();
    assert!(riff_core::vault::load(&path).is_err());
}

#[test]
fn account_logout_cancels_private_jobs_without_cancelling_unrelated_work() {
    let mut jobs = Jobs::default();
    let a = jobs
        .submit("spotify.library".into(), None, || {
            std::thread::sleep(Duration::from_millis(30));
            Ok(json!("private"))
        })
        .unwrap();
    let b = jobs
        .submit("music.search".into(), None, || Ok(json!("public")))
        .unwrap();
    assert_eq!(
        jobs.cancel_operations(&["spotify.library", "spotify.import"]),
        1
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut results = Vec::new();
    while results.len() < 2 && Instant::now() < deadline {
        for done in jobs.drain() {
            jobs.complete(&done);
            results.push(done.id);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        jobs.get(a["job_id"].as_str().unwrap()).unwrap()["state"],
        "cancelled"
    );
    assert_eq!(
        jobs.get(b["job_id"].as_str().unwrap()).unwrap()["state"],
        "completed"
    );
}
