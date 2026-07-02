//! Workspace index built by parsing BUCK/BUILD files directly with the
//! starlark engine — no buck2 required. Answers both directions of the
//! file⇄target question (docs/00-plan.md) and gives the target view a data
//! source when buck2 isn't installed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::buck::Target;
use crate::template::TemplateSet;
use crate::{RuleSelector, list_entries, list_rules};

#[derive(Debug, Clone, Default)]
pub struct Index {
    pub targets: Vec<Target>,
    /// package (e.g. "//crates/ske") -> BUCK file that defines it
    pub buck_files: BTreeMap<String, PathBuf>,
    /// absolute source file -> indices into `targets`
    file_targets: BTreeMap<PathBuf, Vec<usize>>,
}

impl Index {
    /// Labels of every target that lists this file in a file-carrying attr.
    pub fn targets_of(&self, file: &Path) -> Vec<&str> {
        self.file_targets
            .get(file)
            .map(|is| is.iter().map(|i| self.targets[*i].label.as_str()).collect())
            .unwrap_or_default()
    }
}

const BUCK_NAMES: [&str; 3] = ["BUCK", "BUILD", "BUILD.bazel"];
const SKIP_DIRS: [&str; 3] = ["target", "node_modules", "buck-out"];

pub fn scan(root: &Path, templates: &TemplateSet) -> Index {
    let mut index = Index::default();
    let mut buck_files = Vec::new();
    collect_buck_files(root, &mut buck_files);
    buck_files.sort();

    for buck_file in buck_files {
        let Ok(source) = std::fs::read_to_string(&buck_file) else {
            continue;
        };
        let Ok(rules) = list_rules(&source) else {
            continue;
        };
        let pkg_dir = buck_file.parent().unwrap_or(root);
        let rel = pkg_dir
            .strip_prefix(root)
            .unwrap_or(pkg_dir)
            .to_string_lossy()
            .replace('\\', "/");
        let package = format!("//{rel}");
        index.buck_files.insert(package.clone(), buck_file.clone());

        for rule in rules {
            let Some(name) = rule.name else { continue };
            let call = rule.rule_type;

            let attrs_of = |attr: &str| {
                list_entries(
                    &source,
                    &RuleSelector {
                        rule_name: call.clone(),
                        attr: attr.into(),
                        name: Some(name.clone()),
                    },
                )
                .unwrap_or_default()
            };

            // Which attributes carry files: ask the templates, else assume srcs.
            let file_attrs: Vec<String> = match templates.rule(&call) {
                Some((_, r)) => r.file_attrs().map(|f| f.attr.clone()).collect(),
                None => vec!["srcs".into()],
            };
            let dep_attr = templates
                .rule(&call)
                .and_then(|(_, r)| r.deps.as_ref().map(|d| d.attr.clone()))
                .unwrap_or_else(|| "deps".into());

            let mut srcs = Vec::new();
            let mut files = Vec::new();
            for attr in &file_attrs {
                for entry in attrs_of(attr) {
                    // Only plain relative paths become file links; labels and
                    // globs stay display-only.
                    if !entry.starts_with(':')
                        && !entry.starts_with("//")
                        && !entry.contains('*')
                    {
                        files.push(pkg_dir.join(&entry));
                    }
                    srcs.push(entry);
                }
            }

            let deps = attrs_of(&dep_attr);
            let visibility = attrs_of("visibility");
            let _ = &attrs_of;
            let ti = index.targets.len();
            index.targets.push(Target {
                label: format!("{package}:{name}"),
                name,
                package: package.clone(),
                rule_type: call,
                srcs,
                deps,
                visibility,
            });
            for f in files {
                index.file_targets.entry(f).or_default().push(ti);
            }
        }
    }
    index
}

fn collect_buck_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if !name.starts_with('.') && !SKIP_DIRS.contains(&name.as_ref()) {
                collect_buck_files(&path, out);
            }
        } else if BUCK_NAMES.contains(&name.as_ref()) {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_a_workspace() {
        let root = std::env::temp_dir().join(format!("ske-index-test-{}", std::process::id()));
        let pkg = root.join("app");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(
            pkg.join("BUCK"),
            r#"rust_binary(
    name = "app",
    srcs = ["src/main.rs", "src/util.rs"],
    deps = [":lib"],
)

rust_library(
    name = "lib",
    srcs = ["src/lib.rs"],
)
"#,
        )
        .unwrap();

        let index = scan(&root, &TemplateSet::default());
        assert_eq!(index.targets.len(), 2);
        assert_eq!(index.targets[0].label, "//app:app");
        assert_eq!(index.targets[0].deps, vec![":lib"]);
        assert_eq!(index.buck_files["//app"], pkg.join("BUCK"));
        assert_eq!(
            index.targets_of(&pkg.join("src/main.rs")),
            vec!["//app:app"]
        );
        // lib.rs belongs to the library, main.rs does not
        assert_eq!(index.targets_of(&pkg.join("src/lib.rs")), vec!["//app:lib"]);
        assert!(index.targets_of(&pkg.join("src/other.rs")).is_empty());

        std::fs::remove_dir_all(&root).ok();
    }
}
