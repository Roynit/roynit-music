"""Publish the Roy Music Android APK to the Roynit library (same store and checks as the Windows installer)."""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import tomllib
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wrangler", default="wrangler")
    parser.add_argument("--config", required=True, type=Path, help="Existing Roynit library Wrangler config")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    import re
    version = re.search(r'versionName = "([0-9.]+)"', (root / "android" / "app" / "build.gradle.kts").read_text(encoding="utf-8")).group(1)
    source = root / "dist" / "android" / "roy-music-android-arm64.apk"
    binary = source.with_name(f"Roy-Music-Android-{version}.apk")
    binary.write_bytes(source.read_bytes())
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    manifest = (source.parent / (source.name + ".sha256")).read_text().split()[0]
    if manifest != digest:
        raise SystemExit("APK checksum does not match the one published to GitHub")
    size = binary.stat().st_size
    if not 2 <= size <= 90 * 1024 * 1024 or binary.read_bytes()[:2] != b"PK":
        raise SystemExit("Expected an APK within the library upload limit")
    config = str(args.config.resolve())

    def run(*argv):
        return subprocess.check_output([args.wrangler, *argv, "--config", config], text=True, encoding="utf-8")

    def sql_literal(value):
        return "'" + str(value).replace("'", "''") + "'"

    def query(sql, write=False):
        if not write:
            return json.loads(run("d1", "execute", "roynit-library", "--remote", "--json", "--command", sql))
        # A UTF-8 file preserves multiline release notes through Windows .cmd launchers.
        with tempfile.TemporaryDirectory(prefix="roynit-publish-") as folder:
            script = Path(folder) / "query.sql"
            script.write_text(sql, encoding="utf-8")
            try:
                run("d1", "execute", "roynit-library", "--remote", "--json", "--yes", "--file", str(script))
            except subprocess.CalledProcessError as error:
                raise SystemExit(error.output) from error
            return []

    existing = query("SELECT id,sha256,published_at FROM releases WHERE app_slug='roynit-music-android' AND version=" + sql_literal(version))[0]["results"]
    if existing:
        if existing[0]["sha256"] != digest or not existing[0]["published_at"]:
            raise SystemExit("Version already exists with different contents or is a draft; never overwrite it")
        print("Already published:", version, digest)
        return
    parts = tuple(map(int, version.split(".")))
    rank = (parts[0] * 100000 + parts[1]) * 100000 + parts[2]
    latest = query("SELECT MAX(version_rank) AS rank FROM releases WHERE app_slug='roynit-music-android' AND published_at IS NOT NULL")[0]["results"][0]["rank"]
    if latest is not None and rank <= latest:
        raise SystemExit("Refusing to publish a version older than the current release")
    release_id = str(uuid.uuid5(uuid.NAMESPACE_URL, "https://roynit.dangz.org/roynit-music-android/" + version + "/" + digest))
    run("r2", "object", "put", "roynit-library-releases/releases/" + release_id, "--remote", "--file", str(binary), "--content-type", "application/octet-stream")
    stamp = datetime.datetime.now(datetime.timezone.utc).isoformat()
    notes = (root / "android" / "RELEASE_NOTES.md").read_text(encoding="utf-8")[:4000]
    app_values = ["roynit-music-android", "Roy Music Mobile", "Roy Music cho Android: nghe nhạc đa nguồn, lời bài hát từng từ, nhập playlist và đồng bộ thư viện với Roynit ID.", "Android", stamp]
    values = [release_id, "roynit-music-android", version, notes, binary.name, size, digest, 0, rank, stamp, stamp]
    query("INSERT OR IGNORE INTO apps(slug,name,summary,platform,created_at) VALUES (" + ",".join(map(sql_literal, app_values)) + ");"
          + "UPDATE apps SET name=" + sql_literal(app_values[1]) + ", summary=" + sql_literal(app_values[2]) + " WHERE slug='roynit-music-android';"
          + "INSERT INTO releases(id,app_slug,version,notes,filename,size,sha256,required,version_rank,created_at,published_at) SELECT "
          + ",".join(map(sql_literal, values)) + " WHERE NOT EXISTS (SELECT 1 FROM releases WHERE app_slug='roynit-music-android' AND published_at IS NOT NULL AND version_rank >= " + str(rank) + ");", write=True)
    published = query("SELECT sha256 FROM releases WHERE id=" + sql_literal(release_id) + " AND published_at IS NOT NULL")[0]["results"]
    if not published or published[0]["sha256"] != digest:
        raise SystemExit("Release publication was not confirmed")
    print("Published:", "https://roynit.dangz.org/get/roynit-music-android/" + version)
    print("SHA-256:", digest)


if __name__ == "__main__":
    main()
