use crate::{store, tools};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub executable: PathBuf,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub hooks: Vec<String>,
}
fn valid(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 64
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c)),
        "Invalid plugin ID"
    );
    Ok(())
}
fn path(root: &Path, id: &str) -> Result<PathBuf> {
    valid(id)?;
    Ok(root.join("plugins").join(format!("{id}.json")))
}
pub fn import(root: &Path, manifest: &Path) -> Result<Plugin> {
    ensure!(
        std::fs::metadata(manifest)?.len() <= 1024 * 1024,
        "Plugin manifest exceeds 1 MB"
    );
    let mut plugin: Plugin = serde_json::from_reader(std::fs::File::open(manifest)?)?;
    valid(&plugin.id)?;
    ensure!(
        plugin.hooks.iter().all(|h| h == "track_changed"),
        "Unsupported native plugin hook"
    );
    if plugin.executable.is_relative() {
        plugin.executable = manifest
            .parent()
            .unwrap_or(Path::new("."))
            .join(&plugin.executable);
    }
    plugin.executable = plugin
        .executable
        .canonicalize()
        .context("Plugin executable not found")?;
    ensure!(
        plugin.executable.is_file(),
        "Plugin executable must be a file"
    );
    #[cfg(windows)]
    ensure!(
        plugin
            .executable
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("exe")),
        "Native Windows plugins must be compiled executables"
    );
    plugin.sha256 = store::file_hash(&plugin.executable)?;
    plugin.enabled = false;
    store::write_json(&path(root, &plugin.id)?, &plugin)?;
    Ok(plugin)
}
pub fn report(root: &Path) -> Result<Value> {
    let mut valid_plugins = Vec::new();
    let mut warnings = Vec::new();
    for entry in std::fs::read_dir(root.join("plugins"))? {
        let entry = entry?;
        let p = entry.path();
        if !p.extension().is_some_and(|x| x == "json") {
            continue;
        }
        let parsed = (|| -> Result<Plugin> {
            ensure!(
                entry.file_type()?.is_file() && entry.metadata()?.len() <= 1024 * 1024,
                "Plugin manifest must be a regular file below1MB"
            );
            let plugin: Plugin = serde_json::from_reader(std::fs::File::open(&p)?)?;
            valid(&plugin.id)?;
            ensure!(
                p.file_stem().and_then(|s| s.to_str()) == Some(plugin.id.as_str()),
                "Plugin ID differs from filename"
            );
            ensure!(
                plugin.hooks.iter().all(|h| h == "track_changed"),
                "Unsupported native plugin hook"
            );
            Ok(plugin)
        })();
        match parsed {
            Ok(plugin) => valid_plugins.push(plugin),
            Err(error) => warnings.push(
                json!({"file":entry.file_name().to_string_lossy(),"error":error.to_string()}),
            ),
        }
    }
    Ok(json!({"plugins":valid_plugins,"warnings":warnings}))
}
pub fn list(root: &Path) -> Result<Vec<Plugin>> {
    Ok(serde_json::from_value(report(root)?["plugins"].clone())?)
}
pub fn set_enabled(root: &Path, id: &str, enabled: bool) -> Result<Plugin> {
    let p = path(root, id)?;
    let mut plugin: Plugin = serde_json::from_reader(std::fs::File::open(&p)?)?;
    plugin.enabled = enabled;
    store::write_json(&p, &plugin)?;
    Ok(plugin)
}
pub fn remove(root: &Path, id: &str) -> Result<()> {
    std::fs::remove_file(path(root, id)?)?;
    Ok(())
}
pub fn execute(root: &Path, id: &str, params: Value) -> Result<Value> {
    let plugin: Plugin = serde_json::from_reader(std::fs::File::open(path(root, id)?)?)?;
    ensure!(plugin.enabled, "Plugin is disabled");
    ensure!(
        store::file_hash(&plugin.executable)? == plugin.sha256,
        "Plugin executable changed; import it again before enabling"
    );
    let payload = serde_json::to_vec(&json!({"protocol_version":1,"params":params}))?;
    ensure!(
        payload.len() <= 8 * 1024 * 1024,
        "Plugin request exceeds 8 MB"
    );
    let output = tools::run_with_input(
        plugin
            .executable
            .to_str()
            .context("Invalid executable path")?,
        std::iter::empty::<&str>(),
        Duration::from_secs(10),
        Some(payload),
    )?;
    let value: Value = serde_json::from_slice(&output)
        .context("Plugin must return one JSON response on stdout")?;
    let commands = value["commands"]
        .as_array()
        .context("Plugin response needs commands array")?;
    ensure!(
        commands.len() <= 16,
        "Plugin may return at most 16 commands"
    );
    for command in commands {
        ensure!(
            [
                "queue.reorder",
                "queue.mode",
                "favorites.set",
                "playlist.save",
                "settings.set"
            ]
            .contains(&command["command"].as_str().unwrap_or("")),
            "Plugin requested a disallowed command"
        );
    }
    Ok(value)
}
