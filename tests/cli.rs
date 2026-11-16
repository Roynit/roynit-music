use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn session_recovers_from_invalid_json_and_preserves_request_ids() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_riff"))
        .arg("--data-dir")
        .arg(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let input = child.stdin.as_mut().unwrap();
        writeln!(input, "{{broken").unwrap();
        writeln!(
            input,
            "{}",
            json!({"id":"settings-write","command":"settings.set","params":{"theme":"dark"}})
        )
        .unwrap();
        writeln!(input, "{}", json!({"id":42,"command":"settings.get"})).unwrap();
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let rows: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(rows[0]["event"], "ready");
    assert_eq!(rows[1]["error"]["code"], "invalid_json");
    assert_eq!(rows[2]["id"], "settings-write");
    assert_eq!(rows[3]["id"], 42);
    assert_eq!(rows[3]["data"]["theme"], "dark");
    assert!(dir.path().join("library.json").is_file());
}

#[test]
fn one_shot_reports_failure_through_exit_status_and_json() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_riff"))
        .arg("--data-dir")
        .arg(dir.path())
        .arg("--request")
        .arg(json!({"id":7,"command":"not.implemented"}).to_string())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let row: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(row["id"], 7);
    assert_eq!(row["ok"], false);
}
