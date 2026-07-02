# Architecture

## Crate layout

```
skedit/                  workspace root
├── crates/
│   ├── ske/             core library (no UI):
│   │   ├── starlark edit/format engine   (extracted from skedit-cli)
│   │   ├── template loading + validation (the *.skedit.toml format)
│   │   ├── buck2 driver  (spawn buck2, parse --json output, stream events)
│   │   └── workspace model (cells, packages, targets, file⇄target index)
│   └── skyde/           the iced GUI, depends on ske
├── skedit-cli/          thin CLI over ske's starlark editor (kept as-is for scripting)
└── docs/                these documents
```

Rule of thumb: **skyde renders and dispatches; ske decides.** Anything testable without
a window lives in `ske`.

## The workspace model (ske)

Two trees over the same directory, cross-linked:

```
FileTree:    dirs/files from walkdir (gitignore-aware), git status per file
TargetTree:  cells → packages → targets, from `buck2 targets //... --json`

FileTargetIndex: file path -> [target labels]   (resolved from srcs globs)
                 target    -> [file paths]
```

The index answers the README's hard question ("how do I communicate what files belong
in what target") in both directions: file view can badge each file with its targets
(0 targets = orphan warning, 2+ = shared), target view expands a target into its files
(as the project-view mockup shows under `aviary-core`).

A file can be in a lib, a test, and a binary target at once — the model is many-to-many
by construction; the UI just renders the same node in several places.

## Buck2 integration

Everything through the `buck2` binary; no linking against buck2 internals.

| Need | Command |
|---|---|
| workspace detection | find `.buckconfig` upward |
| target list | `buck2 targets //... --json` (name, type, package) |
| target attrs (inspector) | `buck2 uquery "attrfilter(...)" --output-all-attributes` / `buck2 targets <label> --json` |
| dep graph (graph view) | `buck2 uquery "deps(//pkg:target)" --json` |
| reverse deps ("affected") | `buck2 uquery "rdeps(//..., <changed targets>)"` |
| build | `buck2 build <label>` with `--event-log` / parse stderr progress |
| cache state ("cached", "0.4s · remote cache") | build event stream; degrade gracefully if unavailable |

Long-running commands run on a tokio runtime inside `ske`; skyde receives progress as
iced messages (`Task::perform` / subscription channel). Never block the UI thread on buck2.

Bazel later: same trait (`BuildEngine`) with a `bazel query`/`bazel build` impl. Template
files declare `engine` so labels/queries can be dialected.

## GUI (skyde, iced 0.14)

Current single-file `main.rs` grows into modules; state stays one `Skyde` struct
(Elm architecture — iced forces this and it's right for us):

```
skyde/src/
├── main.rs          app wiring, theme, subscriptions
├── shell/           titlebar, activity rail, status bar
├── panes/
│   ├── files.rs     file tree sidebar
│   ├── targets.rs   target tree sidebar (project view)
│   ├── editor.rs    tabs + text_editor + gutter + breadcrumbs
│   ├── inspector.rs target detail panel (right side)
│   ├── graph.rs     build-graph canvas (iced canvas widget)
│   └── console.rs   build output / terminal panel
└── theme.rs         palette matching the mockups (warm dark, orange accent)
```

- **Text editing**: iced `text_editor` + `iced_highlighter` (syntect) for syntax colors.
  Known limits (no multi-cursor, minimap, folding). Accepted for now; the editor pane is
  isolated so it can be replaced wholesale later.
- **Graph view**: `iced::widget::canvas`. Per-package graphs are small (tens of nodes);
  a layered layout (topo-sort into columns by dep depth, then order within column to
  reduce crossings) matches the mockup's left→right flow. Pan/zoom on the canvas.
- **Icons**: Nerd Font glyphs (already in place). Rule-type → icon comes from the
  template, not hardcoded.

## LSP (M5)

- One LSP client per language server, spawned per template `lsp.cmd`, stdio transport.
- Use `async-lsp` or `lsp-server` + `lsp-types` crates; run in ske, expose a message
  stream to skyde.
- v1 features in priority order: diagnostics → hover → goto-def → document symbols
  (breadcrumbs) → completion. Completion last: it needs the most editor plumbing.

## Testing

- `ske` starlark editing: golden-file tests (input BUCK + op → expected BUCK). skedit-cli
  behavior today is the spec.
- buck2 driver: fixture workspace under `testfiles/` exercised in CI when buck2 is on
  PATH; JSON-parsing tests from recorded outputs otherwise.
- skyde: keep logic out; snapshot-test view-model structs, not pixels.
