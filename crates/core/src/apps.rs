//! Finding installed applications.
//!
//! Spike scope: Start Menu shortcuts on Windows and `.app` bundles on macOS.
//! Store (UWP) apps on Windows are not in the Start Menu folders and are not
//! found yet; that needs the shell's AppsFolder and is a known gap.

use crate::Item;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Every application found on this machine, deduplicated by name.
pub fn discover() -> Vec<Item> {
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for root in roots() {
        collect(&root.dir, root.depth, &mut |path| {
            let Some(title) = title_for(path) else {
                return;
            };
            if seen.insert(title.to_lowercase()) {
                items.push(Item::app(title, path.to_string_lossy()));
            }
        });
    }
    items
}

struct Root {
    dir: PathBuf,
    /// How many folder levels below `dir` to look into.
    depth: u8,
}

#[cfg(windows)]
fn roots() -> Vec<Root> {
    // The user's own Start Menu comes first so it wins when both list an app.
    ["APPDATA", "ProgramData"]
        .iter()
        .filter_map(|var| std::env::var_os(var))
        .map(|base| Root {
            dir: PathBuf::from(base).join(r"Microsoft\Windows\Start Menu\Programs"),
            depth: 4,
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn roots() -> Vec<Root> {
    let mut roots = vec![
        Root { dir: "/Applications".into(), depth: 1 },
        Root { dir: "/System/Applications".into(), depth: 1 },
    ];
    if let Some(home) = std::env::var_os("HOME") {
        roots.insert(0, Root { dir: PathBuf::from(home).join("Applications"), depth: 1 });
    }
    roots
}

#[cfg(all(unix, not(target_os = "macos")))]
fn roots() -> Vec<Root> {
    // Linux is not a target; this exists so the spike runs on a dev box.
    let mut roots = vec![Root { dir: "/usr/share/applications".into(), depth: 0 }];
    if let Some(home) = std::env::var_os("HOME") {
        roots.insert(0, Root { dir: PathBuf::from(home).join(".local/share/applications"), depth: 0 });
    }
    roots
}

/// Calls `found` for each application entry under `dir`.
fn collect(dir: &Path, depth: u8, found: &mut dyn FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if is_app(&path) {
            found(&path);
        } else if depth > 0 && path.is_dir() {
            collect(&path, depth - 1, found);
        }
    }
}

fn extension_is(path: &Path, wanted: &str) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(wanted))
}

#[cfg(windows)]
fn is_app(path: &Path) -> bool {
    extension_is(path, "lnk")
}

#[cfg(target_os = "macos")]
fn is_app(path: &Path) -> bool {
    extension_is(path, "app")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn is_app(path: &Path) -> bool {
    extension_is(path, "desktop")
}

/// The name to show for an application, or `None` to leave it out.
#[cfg(not(all(unix, not(target_os = "macos"))))]
fn title_for(path: &Path) -> Option<String> {
    let title = path.file_stem()?.to_str()?.trim().to_string();
    is_wanted(&title).then_some(title)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn title_for(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    if text.lines().any(|line| line.trim() == "NoDisplay=true") {
        return None;
    }
    let title = text.lines().find_map(|line| line.strip_prefix("Name="))?.trim().to_string();
    is_wanted(&title).then_some(title)
}

/// Start Menu folders are full of uninstallers and readme links; drop them.
fn is_wanted(title: &str) -> bool {
    if title.is_empty() {
        return false;
    }
    let lower = title.to_lowercase();
    !["uninstall", "readme", "release notes", "license"]
        .iter()
        .any(|noise| lower.contains(noise))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_installer_noise() {
        assert!(is_wanted("Firefox"));
        assert!(!is_wanted("Uninstall Firefox"));
        assert!(!is_wanted("README"));
        assert!(!is_wanted(""));
    }

    #[test]
    fn matches_extensions_without_caring_about_case() {
        assert!(extension_is(Path::new("Thing.LNK"), "lnk"));
        assert!(!extension_is(Path::new("Thing.txt"), "lnk"));
        assert!(!extension_is(Path::new("Thing"), "lnk"));
    }

    #[test]
    fn walks_only_as_deep_as_asked() {
        let ext = if cfg!(windows) {
            "lnk"
        } else if cfg!(target_os = "macos") {
            "app"
        } else {
            "desktop"
        };
        let root = std::env::temp_dir().join(format!("launcher-apps-{}", std::process::id()));
        let nested = root.join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        for dir in [&root, &root.join("a"), &nested] {
            std::fs::write(dir.join(format!("x.{ext}")), "Name=X\n").unwrap();
        }

        let count = |depth| {
            let mut n = 0;
            collect(&root, depth, &mut |_| n += 1);
            n
        };
        assert_eq!(count(0), 1);
        assert_eq!(count(1), 2);
        assert_eq!(count(2), 3);
        std::fs::remove_dir_all(root).ok();
    }
}
