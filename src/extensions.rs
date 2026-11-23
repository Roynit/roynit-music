use crate::{Engine, model::Track, store};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// Native extensions are compiled into the Rust host. Legacy JS is never evaluated.
pub trait Extension: Send {
    fn id(&self) -> &str;
    fn on_track_changed(&mut self, _track: &Track) -> Result<()> {
        Ok(())
    }
    fn command(&mut self, _engine: &mut Engine, _command: &str, _params: &Value) -> Result<Value>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tokens: std::collections::BTreeMap<String, Value>,
    #[serde(default)]
    pub enabled: bool,
}
pub fn save_theme(root: &Path, theme: &Theme) -> Result<()> {
    validate_id(&theme.id)?;
    crate::appearance::Appearance::from_tokens(serde_json::to_value(&theme.tokens)?)?;
    ensure!(!theme.name.trim().is_empty(), "Theme needs a name");
    ensure!(
        serde_json::to_vec(theme)?.len() < 2 * 1024 * 1024,
        "Theme exceeds 2 MB"
    );
    store::write_json(
        &root.join("themes").join(format!("{}.json", theme.id)),
        theme,
    )
}
pub fn theme_report(root: &Path) -> Result<Value> {
    let mut valid = Vec::new();
    let mut errors = Vec::new();
    for entry in std::fs::read_dir(root.join("themes"))? {
        let entry = entry?;
        let path = entry.path();
        if !path.extension().is_some_and(|x| x == "json") {
            continue;
        }
        let parsed = (|| -> Result<Theme> {
            ensure!(
                entry.file_type()?.is_file() && entry.metadata()?.len() < 2 * 1024 * 1024,
                "Theme must be a regular file below2MB"
            );
            let theme: Theme = serde_json::from_reader(std::fs::File::open(&path)?)?;
            validate_id(&theme.id)?;
            ensure!(
                path.file_stem().and_then(|s| s.to_str()) == Some(theme.id.as_str()),
                "Theme ID differs from filename"
            );
            crate::appearance::Appearance::from_tokens(serde_json::to_value(&theme.tokens)?)?;
            Ok(theme)
        })();
        match parsed {
            Ok(theme) => valid.push(theme),
            Err(error) => {
                errors.push(serde_json::json!({"file":entry.file_name().to_string_lossy(),"error":error.to_string()}))
            }
        }
    }
    Ok(serde_json::json!({"themes":valid,"warnings":errors}))
}
pub fn themes(root: &Path) -> Result<Vec<Theme>> {
    Ok(serde_json::from_value(
        theme_report(root)?["themes"].clone(),
    )?)
}
pub fn import_theme(root: &Path, path: &Path) -> Result<Value> {
    ensure!(
        std::fs::metadata(path)?.len() < 2 * 1024 * 1024,
        "Theme exceeds2MB"
    );
    let value: Value = serde_json::from_reader(std::fs::File::open(path)?)?;
    ensure!(value.get("tokens").is_some(), "This is not a Roy Music theme file");
    let theme = serde_json::from_value::<Theme>(value)?;
    save_theme(root, &theme)?;
    Ok(serde_json::json!({"theme":theme}))
}
pub fn delete_theme(root: &Path, id: &str) -> Result<()> {
    validate_id(id)?;
    std::fs::remove_file(root.join("themes").join(format!("{id}.json")))?;
    Ok(())
}
fn validate_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 64
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "Invalid theme ID"
    );
    Ok(())
}
