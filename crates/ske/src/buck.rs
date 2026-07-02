//! Driver for the `buck2` CLI. Everything goes through the binary; we never
//! link against buck2 internals (docs/01-architecture.md).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

#[derive(Debug, Clone)]
pub struct Target {
    pub label: String,
    pub name: String,
    pub package: String,
    pub rule_type: String,
    pub srcs: Vec<String>,
    pub deps: Vec<String>,
    pub visibility: Vec<String>,
}

/// Kind of a target, inferred from its rule type name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Binary,
    Library,
    Test,
    Web,
    Other,
}

impl Target {
    pub fn kind(&self) -> Kind {
        let t = &self.rule_type;
        if t.contains("test") {
            Kind::Test
        } else if t.contains("binary") {
            Kind::Binary
        } else if t.contains("library") {
            Kind::Library
        } else {
            Kind::Other
        }
    }

    /// Short rule name: "prelude//rules.bzl:rust_binary" -> "rust_binary".
    pub fn rule_name(&self) -> &str {
        self.rule_type
            .rsplit(':')
            .next()
            .unwrap_or(&self.rule_type)
    }
}

/// Walk up from `start` looking for a `.buckconfig`.
pub fn find_root(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        if d.join(".buckconfig").is_file() {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

pub fn list_targets(root: &Path) -> Result<Vec<Target>, String> {
    let out = Command::new("buck2")
        .args(["targets", "//...", "--json"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("failed to run buck2: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    parse_targets(&out.stdout)
}

fn parse_targets(json: &[u8]) -> Result<Vec<Target>, String> {
    let parsed: serde_json::Value =
        serde_json::from_slice(json).map_err(|e| format!("bad json from buck2: {e}"))?;
    let mut targets = Vec::new();
    for t in parsed
        .as_array()
        .ok_or("expected a json array from buck2 targets")?
    {
        let str_of = |key: &str| {
            t.get(key)
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string()
        };
        let name = str_of("name");
        // buck2 prints labels with the root cell name ("root//demo"); the
        // local index and the rest of the app use the cell-relative "//demo"
        // form, so strip that cell prefix everywhere it shows up.
        let raw_package = str_of("buck.package");
        let cell = raw_package.split("//").next().unwrap_or("").to_string();
        let strip_cell = |s: String| match s.strip_prefix(&cell) {
            Some(rest) if !cell.is_empty() && rest.starts_with("//") => rest.to_string(),
            _ => s,
        };
        let package = strip_cell(raw_package);
        // srcs come fully qualified ("//demo/src/main.rs"); make them
        // package-relative like the BUCK file writes them.
        let strip_pkg = |s: String| {
            let full = strip_cell(s);
            match full.strip_prefix(&format!("{package}/")) {
                Some(rel) => rel.to_string(),
                None => full,
            }
        };
        targets.push(Target {
            label: format!("{package}:{name}"),
            name,
            rule_type: str_of("buck.type"),
            srcs: str_list(t.get("srcs")).into_iter().map(strip_pkg).collect(),
            deps: str_list(t.get("deps")).into_iter().map(strip_cell).collect(),
            visibility: str_list(t.get("visibility")),
            package,
        });
    }
    Ok(targets)
}

fn str_list(v: Option<&serde_json::Value>) -> Vec<String> {
    v.and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Spawn `buck2 build <label>` with piped stdout/stderr so callers can
/// stream the output line by line. `--console simple` keeps stderr free of
/// superconsole redraw escapes.
pub fn build_child(root: &Path, label: &str) -> std::io::Result<Child> {
    Command::new("buck2")
        .args(["build", label, "--console", "simple"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

/// Run `buck2 build <label>`; returns the combined output either way.
pub fn build(root: &Path, label: &str) -> Result<String, String> {
    let out = Command::new("buck2")
        .args(["build", label])
        .current_dir(root)
        .output()
        .map_err(|e| format!("failed to run buck2: {e}"))?;
    let mut log = String::from_utf8_lossy(&out.stdout).into_owned();
    log.push_str(&String::from_utf8_lossy(&out.stderr));
    if out.status.success() { Ok(log) } else { Err(log) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_buck2_targets_json() {
        let json = br#"[
            {
                "name": "ske",
                "buck.type": "prelude//rules.bzl:rust_library",
                "buck.package": "root//crates/ske",
                "srcs": ["root//crates/ske/src/lib.rs", "root//crates/ske/src/buck.rs"],
                "deps": ["root//third-party:serde_json"],
                "visibility": ["PUBLIC"]
            },
            {
                "name": "skyde",
                "buck.type": "prelude//rules.bzl:rust_binary",
                "buck.package": "root//crates/skyde"
            }
        ]"#;
        let ts = parse_targets(json).unwrap();
        assert_eq!(ts.len(), 2);
        assert_eq!(ts[0].label, "//crates/ske:ske");
        assert_eq!(ts[0].package, "//crates/ske");
        assert_eq!(ts[0].kind(), Kind::Library);
        assert_eq!(ts[0].rule_name(), "rust_library");
        assert_eq!(ts[0].srcs, ["src/lib.rs", "src/buck.rs"]);
        assert_eq!(ts[0].deps, ["//third-party:serde_json"]);
        assert_eq!(ts[1].kind(), Kind::Binary);
        assert!(ts[1].deps.is_empty());
    }
}
