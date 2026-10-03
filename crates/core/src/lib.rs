//! Everything the launcher does that is not drawing pixels.
//!
//! This crate has no dependency on Tauri or on any UI toolkit. The shell in
//! `src-tauri` is a thin adapter over it, so the same core can sit behind a
//! different front end later without being rewritten.

pub mod apps;
pub mod search;
pub mod settings;
pub mod theme;
pub mod usage;

use serde::{Deserialize, Serialize};

/// What happens when an item is activated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Action {
    /// Hand a path to the operating system to open (an app, shortcut, or file).
    Open { path: String },
    /// Run something inside the launcher itself.
    Command { name: String, arg: Option<String> },
}

/// One row the launcher can show and activate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// Stable across restarts; usage history is keyed on it.
    pub id: String,
    pub title: String,
    /// Shown on the right of the row: "Application", "Command", and so on.
    pub kind: String,
    pub action: Action,
}

impl Item {
    pub fn app(title: impl Into<String>, path: impl Into<String>) -> Self {
        let path = path.into();
        Self {
            id: format!("app:{}", path.to_lowercase()),
            title: title.into(),
            kind: "Application".into(),
            action: Action::Open { path },
        }
    }

    pub fn command(title: impl Into<String>, name: &str, arg: Option<&str>) -> Self {
        Self {
            id: match arg {
                Some(arg) => format!("cmd:{name}:{arg}"),
                None => format!("cmd:{name}"),
            },
            title: title.into(),
            kind: "Command".into(),
            action: Action::Command {
                name: name.into(),
                arg: arg.map(Into::into),
            },
        }
    }
}

/// Seconds since the Unix epoch.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
