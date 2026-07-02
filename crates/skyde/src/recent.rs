//! Recently-opened workspaces (goal.md #3): a plain text file in the skyde
//! user dir, one `<name> <path> <epoch-last-opened>` per line.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub epoch: u64,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Newest first. Unparseable lines are skipped.
pub fn load() -> Vec<Entry> {
    let Ok(text) = std::fs::read_to_string(ske::dirs::recents_file()) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = text
        .lines()
        .filter_map(|line| {
            // The path may contain spaces; name is the first token and the
            // epoch the last, everything between is the path.
            let mut parts: Vec<&str> = line.split(' ').collect();
            if parts.len() < 3 {
                return None;
            }
            let epoch = parts.pop()?.parse().ok()?;
            let name = parts.remove(0).to_owned();
            Some(Entry {
                name,
                path: PathBuf::from(parts.join(" ")),
                epoch,
            })
        })
        .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.epoch));
    entries
}

fn save(entries: &[Entry]) {
    let file = ske::dirs::recents_file();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text: String = entries
        .iter()
        .take(20)
        .map(|e| format!("{} {} {}\n", e.name, e.path.display(), e.epoch))
        .collect();
    let _ = std::fs::write(file, text);
}

/// Record that `root` was opened just now.
pub fn touch(root: &Path) {
    let Ok(root) = root.canonicalize() else {
        return;
    };
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().replace(' ', "-"))
        .unwrap_or_else(|| "workspace".into());
    let mut entries = load();
    entries.retain(|e| e.path != root);
    entries.insert(
        0,
        Entry {
            name,
            path: root,
            epoch: now(),
        },
    );
    save(&entries);
}

/// "3 days ago"-style label for the dialog.
pub fn ago(epoch: u64) -> String {
    let dt = now().saturating_sub(epoch);
    match dt {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", dt / 60),
        3600..=86399 => format!("{}h ago", dt / 3600),
        _ => format!("{}d ago", dt / 86400),
    }
}
