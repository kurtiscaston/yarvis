//! Themes: a named set of design tokens the UI applies as CSS variables.
//!
//! A theme is a small JSON file. Three ship inside the binary; any `*.json`
//! dropped into the user's themes folder is loaded alongside them, and a user
//! theme with the same file name as a bundled one replaces it.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const BUNDLED: &[(&str, &str)] = &[
    ("dusk", include_str!("../../../themes/dusk.json")),
    ("celadon", include_str!("../../../themes/celadon.json")),
    ("cobalt", include_str!("../../../themes/cobalt.json")),
];

pub const DEFAULT_THEME: &str = "dusk";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Theme {
    /// The file name without `.json`. Not part of the file itself.
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// "dark" or "light"; tells the webview how to draw scrollbars and carets.
    #[serde(default = "dark")]
    pub appearance: String,
    /// Token name to CSS value, for example `"select": "#F2B33D"`.
    pub tokens: BTreeMap<String, String>,
}

fn dark() -> String {
    "dark".into()
}

/// Bundled themes plus whatever is in `user_dir`, sorted by name.
/// The second value lists files that were skipped and why.
pub fn load_all(user_dir: Option<&Path>) -> (Vec<Theme>, Vec<String>) {
    let mut themes: BTreeMap<String, Theme> = BTreeMap::new();
    let mut problems = Vec::new();

    for (id, text) in BUNDLED {
        match parse(id, text) {
            Ok(theme) => {
                themes.insert(theme.id.clone(), theme);
            }
            Err(why) => problems.push(format!("bundled theme {id}: {why}")),
        }
    }

    let files = user_dir
        .and_then(|dir| std::fs::read_dir(dir).ok())
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"));
    for path in files {
        let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let result = std::fs::read_to_string(&path)
            .map_err(|err| err.to_string())
            .and_then(|text| parse(id, &text));
        match result {
            Ok(theme) => {
                themes.insert(theme.id.clone(), theme);
            }
            Err(why) => problems.push(format!("{}: {why}", path.display())),
        }
    }

    let mut themes: Vec<Theme> = themes.into_values().collect();
    themes.sort_by_cached_key(|theme| theme.name.to_lowercase());
    (themes, problems)
}

pub fn parse(id: &str, text: &str) -> Result<Theme, String> {
    let mut theme: Theme = serde_json::from_str(text).map_err(|err| err.to_string())?;
    theme.id = id.to_string();
    if theme.name.trim().is_empty() {
        return Err("name is empty".into());
    }
    if theme.appearance != "dark" && theme.appearance != "light" {
        return Err(format!("appearance must be \"dark\" or \"light\", not {:?}", theme.appearance));
    }
    for (token, value) in &theme.tokens {
        if !is_token_name(token) {
            return Err(format!("token name {token:?} must be lowercase letters, digits and dashes"));
        }
        if !is_safe_value(value) {
            return Err(format!("token {token:?} has a value that is not a plain CSS value"));
        }
    }
    Ok(theme)
}

fn is_token_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Themes are meant to be shared between users, so a value may only be a
/// plain colour, length or font list. Anything that could load a resource or
/// break out of the declaration is refused.
fn is_safe_value(value: &str) -> bool {
    let lower = value.to_lowercase();
    !value.is_empty()
        && value.len() <= 160
        && !value.contains([';', '{', '}', '<', '>', '\\', '\n'])
        && !["url(", "@import", "expression(", "image("]
            .iter()
            .any(|banned| lower.contains(banned))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_themes_all_load() {
        let (themes, problems) = load_all(None);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(themes.len(), BUNDLED.len());
        assert!(themes.iter().any(|theme| theme.id == DEFAULT_THEME));
    }

    #[test]
    fn bundled_themes_define_the_same_tokens() {
        let (themes, _) = load_all(None);
        let expected: Vec<&String> = themes[0].tokens.keys().collect();
        for theme in &themes {
            assert_eq!(theme.tokens.keys().collect::<Vec<_>>(), expected, "{}", theme.id);
        }
    }

    #[test]
    fn refuses_values_that_could_load_or_inject() {
        for value in ["url(https://x.test/a.png)", "red; background: blue", "x } body { y", "@import 'a'"] {
            let text = format!(r#"{{"name":"T","tokens":{{"text":{}}}}}"#, serde_json::to_string(value).unwrap());
            assert!(parse("t", &text).is_err(), "{value}");
        }
    }

    #[test]
    fn refuses_odd_token_names_and_appearances() {
        assert!(parse("t", r##"{"name":"T","tokens":{"Text":"#fff"}}"##).is_err());
        assert!(parse("t", r##"{"name":"T","tokens":{"--x":"#fff"}}"##).is_err());
        assert!(parse("t", r#"{"name":"T","appearance":"neon","tokens":{}}"#).is_err());
        assert!(parse("t", r#"{"name":" ","tokens":{}}"#).is_err());
    }

    #[test]
    fn a_user_theme_replaces_a_bundled_one_with_the_same_file_name() {
        let dir = std::env::temp_dir().join(format!("yarvis-themes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("dusk.json"), r##"{"name":"My Dusk","tokens":{"text":"#fff"}}"##).unwrap();
        std::fs::write(dir.join("mine.json"), r##"{"name":"Mine","appearance":"light","tokens":{}}"##).unwrap();
        std::fs::write(dir.join("broken.json"), "{").unwrap();

        let (themes, problems) = load_all(Some(&dir));
        assert_eq!(themes.len(), BUNDLED.len() + 1);
        assert_eq!(themes.iter().find(|theme| theme.id == "dusk").unwrap().name, "My Dusk");
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("broken.json"));
        std::fs::remove_dir_all(dir).ok();
    }
}
