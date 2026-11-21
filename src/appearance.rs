use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Appearance {
    pub mode: String,
    pub accent: [u8; 3],
    pub font_size: f32,
    pub reduced_motion: bool,
    pub visualizer: String,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            mode: "dark".into(),
            accent: [170, 152, 255],
            font_size: 15.,
            reduced_motion: false,
            visualizer: "off".into(),
        }
    }
}
impl Appearance {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            ["dark", "light"].contains(&self.mode.as_str()),
            "Theme mode must be dark or light"
        );
        ensure!(
            self.font_size.is_finite() && (12.0..=24.).contains(&self.font_size),
            "Font size must be 12..24"
        );
        ensure!(
            ["off", "bars", "waveform"].contains(&self.visualizer.as_str()),
            "Unknown visualizer"
        );
        Ok(())
    }
    pub fn from_tokens(value: Value) -> Result<Self> {
        let mut value = value;
        if let Some(color) = value["accent"].as_str() {
            let color = color.trim_start_matches('#');
            ensure!(
                color.len() == 6 && color.bytes().all(|c| c.is_ascii_hexdigit()),
                "Accent needs #RRGGBB"
            );
            value["accent"] = serde_json::json!([
                u8::from_str_radix(&color[0..2], 16)?,
                u8::from_str_radix(&color[2..4], 16)?,
                u8::from_str_radix(&color[4..6], 16)?
            ]);
        }
        let appearance: Self = serde_json::from_value(value)?;
        appearance.validate()?;
        Ok(appearance)
    }
}
