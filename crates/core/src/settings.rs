//! User settings, kept as one small JSON file.

use crate::theme::DEFAULT_THEME;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// The global shortcut that shows and hides the launcher.
    pub hotkey: String,
    pub theme: String,
    /// Hide when another window takes focus. Turn off while using devtools.
    pub hide_on_blur: bool,
    /// Show the timing readout at the bottom of the window.
    pub show_timings: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            // Alt+Space is the usual choice but is often already taken
            // (PowerToys Run, Raycast), so the spike starts somewhere free.
            hotkey: "Ctrl+Shift+Space".into(),
            theme: DEFAULT_THEME.into(),
            hide_on_blur: true,
            show_timings: true,
        }
    }
}

impl Settings {
    /// Missing file, unreadable file or unknown fields all fall back to defaults.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_partial_file_keeps_defaults_for_the_rest() {
        let settings: Settings = serde_json::from_str(r#"{"theme":"cobalt"}"#).unwrap();
        assert_eq!(settings.theme, "cobalt");
        assert_eq!(settings.hotkey, Settings::default().hotkey);
        assert!(settings.hide_on_blur);
    }

    #[test]
    fn round_trips_through_a_file() {
        let dir = std::env::temp_dir().join(format!("launcher-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        assert_eq!(Settings::load(&path), Settings::default());

        let changed = Settings { theme: "celadon".into(), show_timings: false, ..Settings::default() };
        changed.save(&path).unwrap();
        assert_eq!(Settings::load(&path), changed);
        std::fs::remove_dir_all(dir).ok();
    }
}
