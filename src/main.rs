use anyhow::Result;
use clap::Parser;
use riff_core::Engine;
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
    sync::mpsc,
    time::Duration,
};

#[derive(Parser)]
#[command(
    version,
    about = "Roynit Music headless engine. One JSON request per line; EOF exits."
)]
struct Args {
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// Execute one request and exit (use session mode for playback).
    #[arg(long)]
    request: Option<String>,
    /// Report embedded notice hashes without opening user data.
    #[arg(long)]
    license_info: bool,
}
fn reply(request: &Value, result: anyhow::Result<Value>) -> Value {
    match result {
        Ok(data) => json!({"id":request.get("id"),"ok":true,"data":data}),
        Err(e) => {
            json!({"id":request.get("id"),"ok":false,"error":{"code":"command_failed","message":format!("{e:#}")}})
        }
    }
}
fn emit(value: &Value) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    serde_json::to_writer(&mut out, value)?;
    writeln!(out)?;
    out.flush()?;
    Ok(())
}
fn main() -> Result<()> {
    let args = Args::parse();
    if args.license_info {
        emit(
            &json!({"version":env!("CARGO_PKG_VERSION"),"notices_sha256":riff_core::store::key(include_str!("../THIRD_PARTY_LICENSES.txt"))}),
        )?;
        return Ok(());
    }
    let mut engine = Engine::open(args.data_dir.map(Ok).unwrap_or_else(Engine::default_root)?)?;
    if let Some(input) = args.request {
        let request: Value = serde_json::from_str(&input)?;
        let result = engine.dispatch(&request);
        let failed = result.is_err();
        emit(&reply(&request, result))?;
        if failed {
            std::process::exit(1);
        }
        return Ok(());
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for row in io::stdin().lock().lines() {
            match row {
                Ok(row) => {
                    if tx.send(row).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    emit(&json!({"event":"ready","protocol_version":1,"version":env!("CARGO_PKG_VERSION")}))?;
    loop {
        match rx.recv_timeout(Duration::from_millis(40)) {
            Ok(row) => {
                if row.trim().is_empty() {
                    continue;
                }
                if row.len() > 8 * 1024 * 1024 {
                    emit(
                        &json!({"ok":false,"error":{"code":"request_too_large","message":"Maximum request is 8 MB"}}),
                    )?;
                    continue;
                }
                match serde_json::from_str::<Value>(&row) {
                    Ok(request) => {
                        let result = engine.dispatch(&request);
                        emit(&reply(&request, result))?;
                    }
                    Err(e) => emit(
                        &json!({"ok":false,"error":{"code":"invalid_json","message":e.to_string()}}),
                    )?,
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        match engine.tick() {
            Ok(Some(event)) => emit(&event)?,
            Ok(None) => {}
            Err(e) => {
                engine.player.stop();
                emit(&json!({"event":"playback_error","error":format!("{e:#}")}))?;
            }
        }
    }
    engine.persist_session()?;
    engine.player.stop();
    engine.store.save()?;
    Ok(())
}
