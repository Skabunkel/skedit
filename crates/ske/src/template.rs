//! The `*.skedit.toml` template format (docs/02-template-system.md).
//!
//! A template teaches the editor a language: which Starlark rules exist, which
//! attributes carry files and deps, and what toolchain to run. Templates are
//! plain text so they can be shared and audited as GitHub repos.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::buck::Kind;

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub template: Meta,
    #[serde(default)]
    pub toolchain: Toolchain,
    pub lark: Lark,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Meta {
    pub name: String,
    #[serde(default = "default_engine")]
    pub engine: String,
    pub version: u32,
}

fn default_engine() -> String {
    "buck2".into()
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Toolchain {
    pub lsp: Option<Cmd>,
    pub fmt: Option<Cmd>,
    pub lint: Option<Cmd>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Cmd {
    pub cmd: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Lark {
    /// Attribute that names a target, e.g. "name".
    pub ident: String,
    #[serde(rename = "rule")]
    pub rules: Vec<Rule>,
    /// Rule name -> .bzl file to load() it from.
    #[serde(default)]
    pub load: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    /// The Starlark function name to look for, e.g. "rust_binary".
    pub call: String,
    pub kind: RuleKind,
    pub srcs: Option<FileAttr>,
    pub hdrs: Option<FileAttr>,
    pub deps: Option<AttrRef>,
    /// Which scaffold/ subdirectory `new target` copies.
    pub scaffold: Option<String>,
}

impl Rule {
    /// File-carrying attributes, in match order.
    pub fn file_attrs(&self) -> impl Iterator<Item = &FileAttr> {
        self.srcs.iter().chain(self.hdrs.iter())
    }

    /// Which attribute a file with this extension belongs in.
    pub fn attr_for_ext(&self, ext: &str) -> Option<&str> {
        self.file_attrs()
            .find(|f| f.ext.iter().any(|e| e == ext))
            .map(|f| f.attr.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleKind {
    Binary,
    Library,
    Test,
    Web,
    Other,
}

impl From<RuleKind> for Kind {
    fn from(k: RuleKind) -> Kind {
        match k {
            RuleKind::Binary => Kind::Binary,
            RuleKind::Library => Kind::Library,
            RuleKind::Test => Kind::Test,
            RuleKind::Web => Kind::Web,
            RuleKind::Other => Kind::Other,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileAttr {
    pub attr: String,
    #[serde(default)]
    pub ext: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AttrRef {
    pub attr: String,
}

pub fn parse_manifest(text: &str) -> Result<Manifest, String> {
    let manifest: Manifest = toml::from_str(text).map_err(|e| e.to_string())?;
    if manifest.template.version != 1 {
        return Err(format!(
            "unsupported manifest version {} (this build supports 1)",
            manifest.template.version
        ));
    }
    if manifest.lark.ident.is_empty() {
        return Err("lark.ident must not be empty".into());
    }
    if manifest.lark.rules.is_empty() {
        return Err("template declares no [[lark.rule]]".into());
    }
    Ok(manifest)
}

/// All active templates, indexed by rule call name.
#[derive(Debug, Clone, Default)]
pub struct TemplateSet {
    pub manifests: Vec<(Manifest, PathBuf)>,
    by_call: BTreeMap<String, (usize, usize)>, // call -> (manifest idx, rule idx)
    pub warnings: Vec<String>,
}

impl TemplateSet {
    /// Load every `*.skedit.toml` found directly inside subdirectories of the
    /// given roots (e.g. `templates/rust/rust.skedit.toml`). Later roots do not
    /// override earlier ones; rule-call collisions are reported as warnings and
    /// the first definition wins.
    pub fn load(roots: &[PathBuf]) -> Self {
        let mut set = TemplateSet::default();
        for root in roots {
            let Ok(entries) = std::fs::read_dir(root) else {
                continue;
            };
            let mut candidates: Vec<PathBuf> = Vec::new();
            for entry in entries.filter_map(|e| e.ok()) {
                let p = entry.path();
                if p.is_dir() {
                    if let Ok(inner) = std::fs::read_dir(&p) {
                        candidates.extend(
                            inner
                                .filter_map(|e| e.ok())
                                .map(|e| e.path())
                                .filter(|p| is_manifest(p)),
                        );
                    }
                } else if is_manifest(&p) {
                    candidates.push(p);
                }
            }
            candidates.sort();
            for path in candidates {
                set.add_file(&path);
            }
        }
        set
    }

    fn add_file(&mut self, path: &Path) {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                self.warnings.push(format!("{}: {e}", path.display()));
                return;
            }
        };
        match parse_manifest(&text) {
            Ok(m) => self.add(m, path.to_path_buf()),
            Err(e) => self.warnings.push(format!("{}: {e}", path.display())),
        }
    }

    pub fn add(&mut self, manifest: Manifest, path: PathBuf) {
        let mi = self.manifests.len();
        for (ri, rule) in manifest.lark.rules.iter().enumerate() {
            if let Some((prev, _)) = self.by_call.get(&rule.call) {
                self.warnings.push(format!(
                    "rule '{}' in template '{}' already defined by template '{}'; keeping the first",
                    rule.call, manifest.template.name, self.manifests[*prev].0.template.name
                ));
            } else {
                self.by_call.insert(rule.call.clone(), (mi, ri));
            }
        }
        self.manifests.push((manifest, path));
    }

    pub fn is_empty(&self) -> bool {
        self.manifests.is_empty()
    }

    pub fn rule(&self, call: &str) -> Option<(&Manifest, &Rule)> {
        let (mi, ri) = self.by_call.get(call)?;
        let (manifest, _) = &self.manifests[*mi];
        Some((manifest, &manifest.lark.rules[*ri]))
    }

    /// Target kind for a rule call name, if any active template declares it.
    pub fn kind_of(&self, call: &str) -> Option<Kind> {
        self.rule(call).map(|(_, r)| r.kind.into())
    }
}

fn is_manifest(p: &Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(".skedit.toml"))
}

/// Minimal scaffold substitution: `{{name}}` and `{{package}}` only, by design
/// (no logic — keeps shared templates auditable).
pub fn substitute(text: &str, name: &str, package: &str) -> String {
    text.replace("{{name}}", name).replace("{{package}}", package)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUST: &str = r#"
[template]
name = "rust"
engine = "buck2"
version = 1

[toolchain]
lsp = { cmd = "rust-analyzer" }
fmt = { cmd = "rustfmt", args = ["--edition", "2024"] }

[lark]
ident = "name"

[[lark.rule]]
call = "rust_binary"
kind = "binary"
srcs = { attr = "srcs", ext = ["rs"] }
deps = { attr = "deps" }
scaffold = "binary"

[[lark.rule]]
call = "rust_library"
kind = "library"
srcs = { attr = "srcs", ext = ["rs"] }
deps = { attr = "deps" }

[lark.load]
rust_binary = "@prelude//rust:rust_binary.bzl"
"#;

    const CC: &str = r#"
[template]
name = "cc"
version = 1

[lark]
ident = "name"

[[lark.rule]]
call = "cc_library"
kind = "library"
srcs = { attr = "srcs", ext = ["cc", "cpp", "c"] }
hdrs = { attr = "hdrs", ext = ["h", "hpp"] }
deps = { attr = "deps" }
"#;

    #[test]
    fn parses_rust_manifest() {
        let m = parse_manifest(RUST).unwrap();
        assert_eq!(m.template.name, "rust");
        assert_eq!(m.template.engine, "buck2");
        assert_eq!(m.lark.rules.len(), 2);
        assert_eq!(m.lark.rules[0].kind, RuleKind::Binary);
        assert_eq!(m.lark.rules[0].attr_for_ext("rs"), Some("srcs"));
        assert_eq!(m.lark.load["rust_binary"], "@prelude//rust:rust_binary.bzl");
        assert_eq!(m.toolchain.lsp.as_ref().unwrap().cmd, "rust-analyzer");
    }

    #[test]
    fn cc_headers_route_to_hdrs() {
        let m = parse_manifest(CC).unwrap();
        let rule = &m.lark.rules[0];
        assert_eq!(rule.attr_for_ext("h"), Some("hdrs"));
        assert_eq!(rule.attr_for_ext("cc"), Some("srcs"));
        assert_eq!(rule.attr_for_ext("zig"), None);
    }

    #[test]
    fn rejects_bad_manifests() {
        assert!(parse_manifest("not toml at all [").is_err());
        assert!(parse_manifest("[template]\nname=\"x\"\nversion=2\n[lark]\nident=\"name\"\nrule=[]").is_err());
    }

    #[test]
    fn collision_keeps_first_and_warns() {
        let mut set = TemplateSet::default();
        set.add(parse_manifest(RUST).unwrap(), "rust.skedit.toml".into());
        let mut clash = parse_manifest(CC).unwrap();
        clash.lark.rules[0].call = "rust_binary".into();
        set.add(clash, "cc.skedit.toml".into());
        assert_eq!(set.warnings.len(), 1);
        assert_eq!(set.kind_of("rust_binary"), Some(Kind::Binary));
        assert_eq!(set.kind_of("nope"), None);
    }

    #[test]
    fn substitutes_scaffold_vars() {
        assert_eq!(
            substitute("rust_binary(name = \"{{name}}\") # {{package}}", "app", "//crates/app"),
            "rust_binary(name = \"app\") # //crates/app"
        );
    }
}
