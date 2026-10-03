//! Frecency: how often and how recently each item was activated.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A launch from two weeks ago counts half as much as one from today.
const HALF_LIFE_DAYS: f64 = 14.0;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Entry {
    pub count: u32,
    pub last_used: u64,
}

#[derive(Debug, Default)]
pub struct Usage {
    entries: HashMap<String, Entry>,
    path: Option<PathBuf>,
}

impl Usage {
    /// Usage that lives only in memory (tests, benchmarks).
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Load from `path`, starting empty if the file is missing or unreadable.
    pub fn load(path: &Path) -> Self {
        let entries = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self {
            entries,
            path: Some(path.to_path_buf()),
        }
    }

    pub fn record(&mut self, id: &str, now: u64) {
        let entry = self.entries.entry(id.to_string()).or_default();
        entry.count = entry.count.saturating_add(1);
        entry.last_used = now;
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string(&self.entries).map_err(std::io::Error::other)?;
        std::fs::write(path, text)
    }

    /// 0.0 for an item never used; grows with use and decays with age.
    pub fn weight(&self, id: &str, now: u64) -> f64 {
        let Some(entry) = self.entries.get(id) else {
            return 0.0;
        };
        let age_days = now.saturating_sub(entry.last_used) as f64 / 86_400.0;
        let decay = 0.5_f64.powf(age_days / HALF_LIFE_DAYS);
        (1.0 + entry.count as f64).ln() * decay
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;

    #[test]
    fn unused_items_weigh_nothing() {
        assert_eq!(Usage::in_memory().weight("app:x", 1_000), 0.0);
    }

    #[test]
    fn more_use_weighs_more() {
        let mut usage = Usage::in_memory();
        usage.record("a", 100);
        for _ in 0..5 {
            usage.record("b", 100);
        }
        assert!(usage.weight("b", 100) > usage.weight("a", 100));
    }

    #[test]
    fn weight_halves_after_the_half_life() {
        let mut usage = Usage::in_memory();
        usage.record("a", 0);
        let fresh = usage.weight("a", 0);
        let aged = usage.weight("a", 14 * DAY);
        assert!((aged - fresh / 2.0).abs() < 1e-9);
    }

    #[test]
    fn survives_a_round_trip_to_disk() {
        let dir = std::env::temp_dir().join(format!("launcher-usage-{}", std::process::id()));
        let path = dir.join("usage.json");
        let mut usage = Usage::load(&path);
        usage.record("a", 42);
        usage.save().unwrap();
        let reloaded = Usage::load(&path);
        assert!(reloaded.weight("a", 42) > 0.0);
        std::fs::remove_dir_all(dir).ok();
    }
}
