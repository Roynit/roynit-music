# Roynit Music distribution

The Windows app uses `https://roynit.dangz.org/api/library/roynit-music/update` for the official update channel. GitHub holds the Rust source. Custom GitHub release repositories still work when explicitly configured.

The shared Roynit library Worker stores metadata in D1 and installers in R2. Music uses app slug `roynit-music`; Eye, Android and Share remain separate apps. The manifest exposes a stable version, executable filename and SHA-256. The app constructs download/checksum URLs on the fixed Roynit HTTPS origin, downloads a bounded executable, verifies SHA-256, then stages it. Installation retains the existing data directory and a previous executable for rollback.

After tests and packaging, publish with an authenticated Wrangler installation:

```powershell
python scripts/publish-cloudflare.py --wrangler <wrangler.cmd> --config <library/wrangler.jsonc>
```

This requires the Roynit library account and existing D1/R2 permissions. Credentials remain in Wrangler's user configuration and must never be committed. The script uploads the installer before publishing metadata, refuses version replacement or downgrade, and verifies publication. Check the public manifest and download hash afterwards.

The shared Worker changes belong in the separate Roynit web workspace. Do not redeploy unrelated unfinished web or Eye changes. The existing Manager can publish future Music releases through its normal app/release workflow. Music releases must use stable versions and `.exe` installers.

Unsigned builds are explicitly reported in `dist/signing-status.json`. SHA-256 checks detect a corrupted download; Windows publisher signing requires a real certificate separately.
