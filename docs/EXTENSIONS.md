# Native Rust plugin integration

The Rust host does not execute renderer JavaScript or CSS. Existing extensions that rely on the Electron DOM need a native rewrite. No automatic conversion is promised.

Roynit has native Classic, Balanced and Smart Shuffle modes in the player. Smart Shuffle uses YouTube Music radio for related picks and inserts them between queued tracks when available. The original Better Shuffle plugin's search expansion and skip/listen affinity profile are not implemented.

A complete Rust example is examples/shuffle_queue.rs. Build with cargo build --release --example shuffle_queue, then import examples/shuffle_queue.json in Settings. The imported manifest is disabled and the host records the executable SHA-256; enable only code you trust. Reimport after rebuilding the executable because the hash changes. Plugin removal deletes the registration, not the user's executable.

The host sends one JSON document to stdin: {"protocol_version":1,"params":...}. Return one JSON document on stdout with commands:[{command,params}], and an optional message. The example preserves the current queue slot and shuffles only upcoming items. UI Run supplies queue/player state; track_changed hooks also receive event,track,queue,player. hooks:[] means manual invocation; hooks:["track_changed"] opts into automatic invocation after committed track changes.

Limits: input8MB, manifest1MB, at most16 returned commands,10second subprocess deadline, bounded captured helper output. Allowed commands: queue.reorder,queue.mode,favorites.set,playlist.save,settings.set. These command checks do not sandbox the native executable: it runs as the Windows user. Engine applies normal validation to returned commands.

An invalid manifest is isolated from healthy plugins and reported under Extension health. No executable is launched merely because a manifest is imported. A changed executable fails the hash check.
