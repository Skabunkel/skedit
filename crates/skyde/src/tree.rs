use std::path::{Path, PathBuf};

pub struct Node {
    pub name: String,
    pub path: PathBuf,
    pub hidden: bool,
    pub kind: Kind,
}

pub enum Kind {
    Dir {
        expanded: bool,
        // None = not read from disk yet (children load lazily on expand)
        children: Option<Vec<Node>>,
    },
    File,
}

pub fn read_dir(dir: &Path) -> Vec<Node> {
    let mut nodes: Vec<Node> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let path = e.path();
                let name = e.file_name().to_string_lossy().into_owned();
                let hidden = name.starts_with('.');
                let kind = if e.file_type().ok()?.is_dir() {
                    Kind::Dir {
                        expanded: false,
                        children: None,
                    }
                } else {
                    Kind::File
                };
                Some(Node {
                    name,
                    path,
                    hidden,
                    kind,
                })
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    nodes.sort_by(|a, b| {
        let a_dir = matches!(a.kind, Kind::Dir { .. });
        let b_dir = matches!(b.kind, Kind::Dir { .. });
        b_dir
            .cmp(&a_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    nodes
}

pub fn toggle(nodes: &mut [Node], target: &Path) {
    for node in nodes {
        if node.path == target {
            if let Kind::Dir { expanded, children } = &mut node.kind {
                *expanded = !*expanded;
                if *expanded && children.is_none() {
                    *children = Some(read_dir(&node.path));
                }
            }
            return;
        }
        if target.starts_with(&node.path) {
            if let Kind::Dir {
                children: Some(children),
                ..
            } = &mut node.kind
            {
                toggle(children, target);
            }
            return;
        }
    }
}
