//! Per-user skyde directories, platform-appropriate:
//! Linux/BSD `~/.local/share/skyde` (XDG), Windows `%APPDATA%\skyde`,
//! macOS `~/Library/Application Support/skyde`.

use std::path::PathBuf;

/// The per-user data directory for skyde. Falls back to the current
/// directory only if the platform gives us no home at all.
pub fn data_dir() -> PathBuf {
    base_dir().unwrap_or_else(|| PathBuf::from(".")).join("skyde")
}

#[cfg(target_os = "windows")]
fn base_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn base_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn base_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
}

/// Where shared, per-user templates live.
pub fn templates_dir() -> PathBuf {
    data_dir().join("templates")
}

/// The recently-opened-workspaces list: one `<name> <path> <epoch>` per line.
pub fn recents_file() -> PathBuf {
    data_dir().join("recent-workspaces")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_ends_with_skyde() {
        assert!(data_dir().ends_with("skyde"));
    }
}
