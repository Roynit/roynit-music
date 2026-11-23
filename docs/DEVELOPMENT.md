# Development

Use Rust 1.100.0. From this repository root:

```powershell
cargo fmt --check
cargo clippy --locked --features desktop --all-targets -- -D warnings
cargo test --locked --features desktop --all-targets
cargo build --release --locked --features desktop --bins --example shuffle_queue
```

Ignored integration tests require network access, media helpers and a native audio device. Run explicitly with `--include-ignored --test-threads=1`. Audio tests are muted. Personal accounts and browser cookies are never used automatically by tests. `examples/soak_audit.rs` accepts a duration in seconds for controlled large-library/native playback stress tests; it does not certify long-term stability.

`roy.exe` is the headless host: one JSON request per stdin line, JSON responses/events on stdout. Keep stdin open during playback; `--request` runs one command and exits. Start with `{"command":"capabilities"}`. Hosts keep Engine on a worker and call tick regularly. Slow operations use job.submit and queued/running/completed/failed/cancelled states. New playback requests supersede pending playback. Preserve recording identity, duration verification and stale-result checks.

The desktop uses egui and Windows SMTC. Preserve source disclosure, real Connecting/Buffering/Playing states, cancellation, keyboard/accessibility and the existing data location. UI polish must not introduce mock results or an Audio Studio editor.

Support reports exclude track titles/IDs and private paths/credentials. Never commit test-output, target, dist, cookies, account stores or signing material. GitHub CI uses synthetic/unit tests; real account checks require local sign-in.

Run the Publish Windows release workflow with a stable tag matching Cargo.toml. It validates/builds/packages and publishes a new version, refusing to replace an existing release. The hosted workflow currently publishes unsigned builds and records that status. Real publisher signing must be performed with an authorized certificate before packaging; see SIGNING.md.
