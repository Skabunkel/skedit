# Template system

Templates teach the editor a language. One template = one directory with a manifest,
all plain text, so a template repo on GitHub is readable, diffable, and reviewable —
you can audit exactly what "someone's Java setup" will do before using it.

## Sharing model

```
skedit template add github:someuser/skedit-java        # clone to templates dir
skedit template add github:someuser/skedit-java@v2     # pin a tag/branch/commit
skedit template list
skedit template update java
```

- Install dir: `~/.config/skedit/templates/<name>/` (plus `<workspace>/.skedit/templates/`
  for project-local overrides — highest priority wins).
- A template repo is just files; "publishing" is pushing to GitHub. No registry, no
  packaging step. Discovery is a GitHub topic (`skedit-template`).
- Pinning matters because templates can name commands to run (lsp, fmt). `add` shows
  the manifest's commands and asks for confirmation before first use.

## Repo layout

```
skedit-rust/
├── rust.skedit.toml        the manifest (below)
└── scaffold/               file trees for `new project` / `new target`
    ├── binary/
    │   ├── BUCK.tmpl
    │   └── src/main.rs.tmpl
    └── library/
        ├── BUCK.tmpl
        └── src/lib.rs.tmpl
```

## Manifest format (`<lang>.skedit.toml`)

TOML: comments allowed, forgiving to hand-edit, serde support. This formalizes the
README's `.rules` sketch:

```toml
# rust.skedit.toml
[template]
name = "rust"
engine = "buck2"            # or "bazel"; affects label/query dialect
version = 1                 # manifest format version

[toolchain]
lsp = { cmd = "rust-analyzer" }
fmt = { cmd = "rustfmt", args = ["--edition", "2024"] }
lint = { cmd = "clippy-driver" }        # optional

# ---- the "lark" section: how to read & edit Starlark for this language ----

# Attributes shared by every rule of this language
[lark]
ident = "name"              # attr that names a target

# One [[lark.rule]] per rule kind the template understands.
[[lark.rule]]
call = "rust_binary"        # the Starlark function name to look for
kind = "binary"             # editor semantic: binary|library|test|web|other
srcs = { attr = "srcs", ext = ["rs"] }
deps = { attr = "deps" }
scaffold = "binary"         # which scaffold/ dir `new target` uses

[[lark.rule]]
call = "rust_library"
kind = "library"
srcs = { attr = "srcs", ext = ["rs"] }
deps = { attr = "deps" }
scaffold = "library"

[[lark.rule]]
call = "rust_test"
kind = "test"
srcs = { attr = "srcs", ext = ["rs"] }
deps = { attr = "deps" }

# Loads to insert at the top of a BUCK file when a rule is first used
[lark.load]
rust_binary  = "@prelude//rust:rust_binary.bzl"
rust_library = "@prelude//rust:rust_library.bzl"
```

C/C++ shows why `srcs` is a table and why there can be several file-carrying attrs
(the README's `ext` / `hdrs` problem):

```toml
[[lark.rule]]
call = "cc_library"
kind = "library"
srcs = { attr = "srcs", ext = ["cc", "cpp", "c"] }
hdrs = { attr = "hdrs", ext = ["h", "hpp"] }
deps = { attr = "deps" }
```

The editor's "add file to target" flow: match the file's extension against the rule's
file-carrying attrs → know it goes in `hdrs` not `srcs` → call the `ske` edit engine.
Extension collisions (a `.h` matching both a cc and an objc template) prompt the user
once, remembered per-workspace in `.skedit/config.toml`.

## Scaffolds

`*.tmpl` files with minimal substitution — `{{name}}`, `{{package}}`, nothing more
(no logic, no loops; keep templates auditable). `new target: rust binary "aviaryd"`
copies `scaffold/binary/`, substitutes, then registers the target in the package's
BUCK file via the lark rules above.

```python
# scaffold/binary/BUCK.tmpl
rust_binary(
    name = "{{name}}",
    srcs = ["src/main.rs"],
)
```

## What templates deliberately can't do (v1)

- No arbitrary code execution at install/load time. Commands run only as the
  user-visible toolchain actions (lsp/fmt/lint), shown at install.
- No parsing of the target language (auto-deps from `use`/`#include`). A template
  *may* later declare an `import-scan` regex as a hint, but v1 keeps deps manual.
- No engine-specific query logic — that lives in ske's `BuildEngine` impls; templates
  only pick `engine`.

## Resolution & validation

- `skedit` validates a manifest on load: unknown keys warn (forward compat),
  missing `ident`/`call`/`kind` error.
- Multiple templates can be active in one workspace (rust + cc + js). Rule `call`
  names must be unique across active templates; collision = load error naming both.
- Version field gates format migrations; loader supports N and N-1.
