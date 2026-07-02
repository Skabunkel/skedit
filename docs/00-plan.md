# skedit — Plan

A template-based code editor built around Buck2/Bazel (Starlark) as a first-class citizen.
"Visual Studio but with Starlark." Not another VSCode.

## What the mockups say

The two mockups (`00_moc_code_view.png`, `01_moc_project_view.png`) define the target UX.

### Code view (`00_moc_code_view.png`)

- Custom titlebar (no OS decorations): `File / Edit / Selection / View / Go` menus, a prominent **Build** button top-right, min/max/close.
- Left icon rail (activity bar): file view, project/target view, graph view, terminal, settings.
- Sidebar: workspace tree that mixes *filesystem* and *build* entries — folders, `.rs` files, `BUCK` files, `Cargo.toml`, `Cargo.lock`. Git status shown inline (`M` marker on modified file, highlighted selection).
- Editor: tabs (with dirty-dot and per-tab file-type icon — note the `BUCK` tab gets a build-graph icon), breadcrumbs that go *deeper than the file* (`aviary-core > src > flock.rs > {} migrate` — symbol-level via LSP), line numbers, run gutter icon on the doc-comment of a runnable, current-line highlight, git diff markers in the gutter (green bars for added lines), minimap-ish scroll indicators right edge.
- Status: LSP health indicator top-right of editor ("rust-analyzer" with green dot).
- Wish-list item from README: Zed-style row:col + language in bottom right status bar.

### Project view (`01_moc_project_view.png`)

- Same shell, different sidebar + main pane.
- Sidebar header: workspace picker (`aviary — buck2 · 11 targets`).
- Sidebar tree is the **target tree**, not the file tree: cells (`//crates`, `//web`, `//tools`) with per-cell target counts, targets typed by rule (`rust_library`, `rust_test`, `rust_binary`, `js_bundle`) with distinct icons, build state per target (✓ cached, ● needs build). Expanding a library target shows its `srcs`.
- Main pane: **build graph** for the selected package. Nodes = targets (name + rule type + state dot), edges = deps, including external deps (`tokio`/`serde` shown as `cargo` nodes). Toolbar: "Affected" filter, layout toggle, **Build all**.
- Right inspector panel for the selected target: name, rule type, badges (`cached`, `8 deps`), target label, `srcs` glob + file count, deps list, visibility, last build time + cache source ("0.4s · remote cache"), Build button, test/file shortcut buttons.
- Legend bottom-left mapping colors to target kinds (binary/library/test/web). Zoom control bottom-right.

## Core idea

The editor knows nothing about languages. **Templates** teach it:

1. How to *read* Starlark build files (which rules are libraries/binaries/tests, which attrs are srcs/deps/name).
2. How to *edit* them (add file → append to `srcs`; add dep → append to `deps`) — this is what `skedit-cli` already does.
3. How to run the toolchain around a language: LSP server, linter, formatter.
4. How to scaffold a new project/target of that language.

Templates are plain, readable text files, shared via GitHub (`user/repo` → clone/fetch into a templates dir). See `02-template-system.md`.

## Scope decisions (proposed)

- **Buck2 first, Bazel later.** One label syntax, one CLI, one daemon protocol to start. The template format stays engine-neutral so Bazel can slot in.
- **Don't parse target languages.** Auto-detecting `use`/`#include` deps (README's "hard if not impossible") is out of scope for v1. The user adds deps via UI; the editor edits Starlark, not Rust/C.
- **Don't build a text engine.** Use iced's `text_editor` now; accept its limits. Revisit (ropey + custom widget, or embed a real engine) only when it blocks us.
- **Read build state from buck2, don't model it.** `buck2 targets`, `buck2 build`, `buck2 uquery`/`cquery` with `--json` give the target tree, dep graph, and cache state. The editor is a *view* over buck2, plus a Starlark editor (skedit-cli's core as a library).

## Milestones

Each milestone is usable on its own.

### M0 — Skeleton that opens real files ✅ (2026-07-02)
- iced app with custom titlebar, activity rail, sidebar, editor, status bar. ✅
- Real file tree from the working directory (lazy-loaded; gitignore-awareness still todo). ✅
- Open/save files in the editor; dirty tracking; tabs; ctrl+s/n/w. ✅
- Zed-style status bar: `row:col`, language. ✅ (encoding todo)
- Mockup theme: warm dark palette, orange accent, icon rail. ✅
- Window resize grips on all edges/corners (needed since decorations are off). ✅
- Right-click context menus: tree entries (new target here / open / add to
  selected target), targets (build / open BUCK / delete), packages (new target). ✅

### M1 — Buck2 awareness (read-only) ✅ (2026-07-02)
- Detect a buck2 workspace (`.buckconfig`); run `buck2 targets //... --json`. ✅ (`ske::buck`)
- Target view in sidebar: packages → targets, typed icons. ✅
- Click target → inspector panel (name, type, srcs, deps, visibility). ✅
- Build button runs `buck2 build`, output streams line-by-line into the console
  panel (`--console simple`, reader threads → iced stream). ✅ (per-target ✓/●
  cache state still todo)
- Verified against a real buck2 workspace (2026-07-02, bundled prelude):
  `buck2 targets --json` parses, cell prefixes (`root//demo`) and qualified srcs
  are normalized to the index's `//demo` + `src/main.rs` forms, dep edges resolve
  in the graph, `buck2 build`/`run` succeed. ✅

### M2 — Template system v1 ✅ (2026-07-02)
- Define + parse the template file format (`02-template-system.md`). ✅ (`ske::template`, unit-tested)
- Ship `templates/rust/` and `templates/cc/` in-repo as references (manifest + scaffolds). ✅
- `skedit template add github:user/repo[@ref]` / `list` / `update` → `~/.config/skedit/templates/`. ✅ (root `skedit` binary; prints toolchain commands at install as the audit step)
- Editor loads templates (workspace `.skedit/templates` → repo `templates/` → user dir) and uses them for target kinds/icons. ✅ (srcs/deps attr routing lands with M3 editing)

### M3 — Editing the build graph (skedit-cli as a library) 🟠 mostly done (2026-07-02)
- Split skedit-cli into `crates/ske` (library) + thin CLI. ✅ (was already split)
- `ske::index`: parses BUCK/BUILD files directly with the starlark engine — target
  list + file⇄target reverse index with **no buck2 required**; templates route which
  attrs carry files. ✅ (literal paths only; glob resolution todo)
- Target view falls back to the local index when buck2 is absent. ✅
- UI actions: new target (rule picker + name, created in the selected directory),
  add selected file to target (extension-routed to srcs/hdrs via template),
  delete target. ✅ Add dep via target picker + rename: todo.
- File badges in the tree: green count of owning targets. ✅ (0/2+ warning styling todo)
- Scaffold file creation on new target (copy scaffold/ + substitute): todo.

### M4 — Graph view 🟠 core done (2026-07-02)
- Build-graph canvas: nodes/edges from the workspace target list (local index or
  buck2), hand-rolled longest-path layering, left→right like the mockup. ✅
- Unresolved deps render as muted "extern" nodes (cargo/prelude deps). ✅
- Kind-colored state dots, click→inspector, pointer cursor on nodes. ✅
- Double-click on a node opens its BUCK file with the rule's first line
  selected (`ske::rule_line`). ✅
- Todo: pan/zoom, per-package filtering (currently whole workspace), "Affected"
  filter (needs buck2 + git diff), edge crossing reduction within layers.

### M5 — Language services
- Spawn LSP from template config (`lsp.cmd`), speak LSP over stdio (`lsp-types` + `lsp-server` or `async-lsp` crates).
- Diagnostics in gutter, hover, goto-def, symbol breadcrumbs (mockup's `{} migrate`).
- This is the biggest lift; it's deliberately *after* the build-graph features because those are the differentiator.

### Later / dreams
- Bazel support (template `engine = "bazel"`, label + query dialect differences).
- Remote deps helpers (crates.io/Maven pickers writing to the right place per template).
- Container image targets (distroless-style) as just-another-rule the template describes.
- Test explorer driven by `rust_test`-kind rules.

## Open questions (from README, with current answers)

| Question | Current answer |
|---|---|
| Communicate the idea well enough? | These docs + mockups + template spec are the pitch. |
| Can I build an editor? | Yes for the shell; text-editing depth is the risk. Lean on iced, defer fancy editing. |
| Template system sharable via GitHub? | Yes — single readable TOML file per language, fetched by repo ref. See 02. |
| Buck2 first-class? | Yes — via buck2's own CLI/JSON, not by reimplementing it. |
| Interproject deps easy? | UI dep-picker → `ske` lib edits `deps`. M3. |
| Remote deps easy? | Template declares the mechanism; later milestone. |
| Which files belong to which target? | Reverse srcs-index + badges; both file-view and target-view answer it. M3. |
