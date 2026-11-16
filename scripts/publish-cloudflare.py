"""Publish an immutable Music installer using an already authenticated Wrangler."""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import subprocess
import tomllib
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wrangler", default="wrangler")
    parser.add_argument("--config", required=True, type=Path, help="Existing Roynit library Wrangler config")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    version = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    binary = root / "dist/riff-rust-windows-x64.exe"
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    manifest = (binary.parent / (binary.name + ".sha256")).read_text().split()[0]
    if manifest != digest:
        raise SystemExit("Installer checksum does not match the packaged manifest")
    size = binary.stat().st_size
    if not 2 <= size <= 90 * 1024 * 1024 or binary.read_bytes()[:2] != b"MZ":
        raise SystemExit("Expected a Windows installer within the library upload limit")
    config = str(args.config.resolve())

    def run(*argv):
        return subprocess.check_output([args.wrangler, *argv, "--config", config], text=True, encoding="utf-8")

    def sql_literal(value):
        return "'" + str(value).replace("'", "''") + "'"

    def query(sql):
        return json.loads(run("d1", "execute", "roynit-library", "--remote", "--json", "--command", sql))

    existing = query("SELECT id,sha256,published_at FROM releases WHERE app_slug='roynit-music' AND version=" + sql_literal(version))[0]["results"]
    if existing:
        if existing[0]["sha256"] != digest or not existing[0]["published_at"]:
            raise SystemExit("Version already exists with different contents or is a draft; never overwrite it")
        print("Already published:", version, digest)
        return
    parts = tuple(map(int, version.split(".")))
    rank = (parts[0] * 100000 + parts[1]) * 100000 + parts[2]
    latest = query("SELECT MAX(version_rank) AS rank FROM releases WHERE app_slug='roynit-music' AND published_at IS NOT NULL")[0]["results"][0]["rank"]
    if latest is not None and rank <= latest:
        raise SystemExit("Refusing to publish a version older than the current release")
    release_id = str(uuid.uuid5(uuid.NAMESPACE_URL, "https://roynit.dangz.org/roynit-music/" + version + "/" + digest))
    run("r2", "object", "put", "roynit-library-releases/releases/" + release_id, "--remote", "--file", str(binary), "--content-type", "application/octet-stream")
    stamp = datetime.datetime.now(datetime.timezone.utc).isoformat()
    notes = (root / "CHANGELOG.md").read_text(encoding="utf-8")[:4000]
    app_values = ["roynit-music", "Roynit Music", "Trình nghe nhạc Rust cho Windows. Tìm nhạc Spotify và YouTube Music. Bản xem trước của dang.", "Windows x64", stamp]
    values = [release_id, "roynit-music", version, notes, "Roynit-Music-" + version + "-Setup.exe", size, digest, 0, rank, stamp, stamp]
    query("INSERT OR IGNORE INTO apps(slug,name,summary,platform,created_at) VALUES (" + ",".join(map(sql_literal, app_values)) + ");"
          + "INSERT INTO releases(id,app_slug,version,notes,filename,size,sha256,required,version_rank,created_at,published_at) SELECT "
          + ",".join(map(sql_literal, values)) + " WHERE NOT EXISTS (SELECT 1 FROM releases WHERE app_slug='roynit-music' AND published_at IS NOT NULL AND version_rank >= " + str(rank) + ");")
    published = query("SELECT sha256 FROM releases WHERE id=" + sql_literal(release_id) + " AND published_at IS NOT NULL")[0]["results"]
    if not published or published[0]["sha256"] != digest:
        raise SystemExit("Release publication was not confirmed")
    print("Published:", "https://roynit.dangz.org/get/roynit-music/" + version)
    print("SHA-256:", digest)


if __name__ == "__main__":
    main()
