mod graph;
mod gutter;
mod tree;

use std::path::{Path, PathBuf};

use iced::border;
use iced::highlighter;
use iced::keyboard;
use iced::mouse;
use iced::widget::{
    Space, button, canvas, column, container, mouse_area, pick_list, responsive, row, rule,
    scrollable, space, stack, text, text_editor, text_input,
};
use iced::widget::text::{LineHeight, Wrapping};
use iced::window;
use iced::{
    Alignment, Background, Border, Color, Element, Fill, Font, Length, Point, Subscription, Task,
    Theme,
};

const NERD_FONT: Font = Font::with_name("MesloLGM Nerd Font Mono");

/// Widget id of the context menu's new-file/new-folder name input.
const NEW_ENTRY_INPUT: &str = "ctx-new-entry";

// Nerd Font codepoints. If a glyph renders as a box, swap the codepoint here.
const ICON_CHEVRON_RIGHT: &str = "\u{eab6}"; // nf-cod-chevron_right
const ICON_CHEVRON_DOWN: &str = "\u{eab4}"; // nf-cod-chevron_down
const ICON_FILE: &str = "\u{ea7b}"; // nf-cod-file
const ICON_TARGET_BUILD: &str = "\u{f0aef}"; // nf-md-alpha_b
const ICON_TARGET_LIB: &str = "\u{f0af9}"; // nf-md-alpha_l
const ICON_TARGET_TEST: &str = "\u{f0b01}"; // nf-md-alpha_t
const ICON_REFRESH: &str = "\u{eb37}"; // nf-cod-refresh
const ICON_ROCKET: &str = "\u{eb44}"; // nf-cod-rocket

// Palette lifted from the mockups: warm near-black, orange accent, olive green.
const BG_CANVAS: Color = Color::from_rgb8(0x14, 0x11, 0x0F); // titlebar, rail, status
const BG_PANEL: Color = Color::from_rgb8(0x1B, 0x17, 0x14); // sidebar, tab strip
const BG_EDITOR: Color = Color::from_rgb8(0x18, 0x14, 0x12);
const BORDER: Color = Color::from_rgb8(0x2C, 0x26, 0x21);
const TEXT: Color = Color::from_rgb8(0xD9, 0xD2, 0xC5);
const MUTED: Color = Color::from_rgb8(0x80, 0x77, 0x6B);
const ACCENT: Color = Color::from_rgb8(0xE2, 0x64, 0x3F);
const ACCENT_BG: Color = Color::from_rgb8(0x33, 0x24, 0x1D); // selection tint
const GREEN: Color = Color::from_rgb8(0x97, 0xB0, 0x6A);
const YELLOW: Color = Color::from_rgb8(0xD9, 0xA0, 0x5B);
const RED: Color = Color::from_rgb8(0xD9, 0x53, 0x4F);

fn main() -> iced::Result {
    iced::application(Skyde::new, Skyde::update, Skyde::view)
        .title(Skyde::title)
        .theme(Skyde::theme)
        .subscription(Skyde::subscription)
        .window(window::Settings {
            decorations: false,
            ..window::Settings::default()
        })
        .run()
}

struct Buffer {
    path: Option<PathBuf>,
    content: text_editor::Content,
    dirty: bool,
    // syntect token for the highlighter, e.g. "rs"
    syntax: String,
    // Editor scroll offset in pixels, mirrored from the actions the editor
    // publishes — iced doesn't expose its internal scroll, and the gutter
    // needs it to stay lined up.
    scroll: f32,
}

impl Buffer {
    fn untitled() -> Self {
        Self {
            path: None,
            content: text_editor::Content::new(),
            dirty: false,
            syntax: "txt".into(),
            scroll: 0.0,
        }
    }

    fn title(&self) -> String {
        match &self.path {
            Some(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string()),
            None => "untitled".into(),
        }
    }
}

struct Skyde {
    root: PathBuf,
    tree: Vec<tree::Node>,
    selected: Option<PathBuf>,
    open: Vec<Buffer>,
    active: Option<usize>,
    view_mode: ViewMode,
    status: String,
    window_id: Option<window::Id>,
    maximized: bool,
    buck_root: Option<PathBuf>,
    templates: ske::template::TemplateSet,
    index: ske::index::Index,
    new_target_name: String,
    new_target_rule: Option<String>,
    targets: Vec<ske::buck::Target>,
    targets_err: Option<String>,
    collapsed_pkgs: std::collections::HashSet<String>,
    selected_target: Option<usize>,
    console: Option<String>,
    building: bool,
    // Height of the editor's text area, measured in view() by `responsive` —
    // update() needs it to clamp scroll the same way cosmic-text does.
    editor_text_h: std::cell::Cell<f32>,
    // Last cursor position in window coordinates, for placing the context
    // menu. mouse_area's right-press message carries no position.
    cursor: Point,
    context: Option<(Point, CtxKind)>,
    new_entry: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Files,
    Targets,
}

impl std::fmt::Display for ViewMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ViewMode::Files => "Files",
            ViewMode::Targets => "Targets",
        })
    }
}

#[derive(Debug, Clone)]
enum Message {
    Edit(text_editor::Action),
    Open(PathBuf),
    ToggleDir(PathBuf),
    SelectTab(usize),
    CloseTab(usize),
    CloseActiveTab,
    Save,
    NewFile,
    RefreshTree,
    Build,
    TargetsLoaded(Result<Vec<ske::buck::Target>, String>),
    SelectTarget(usize),
    TogglePkg(String),
    NewTargetNameChanged(String),
    NewTargetRulePicked(String),
    CreateTarget,
    DeleteTarget(String),
    AddSelectedFile(String),
    BuildTarget(String),
    BuildEvent(BuildEvent),
    CloseConsole,
    Menu(&'static str),
    ChangeView(ViewMode),
    WindowOpened(window::Id),
    DragWindow,
    MinimizeWindow,
    ToggleMaximize,
    CloseWindow,
    ResizeWindow(window::Direction),
    MouseMoved(Point),
    OpenContext(CtxKind),
    CtxAction(Box<Message>),
    CloseContext,
    NewTargetIn(PathBuf),
    OpenTargetSource(usize),
    NewEntryPrompt { dir: PathBuf, folder: bool },
    NewEntryName(String),
    NewEntryCreate,
    AddFileAs { label: String, attr: String },
    SelectTargetByLabel(String),
    WorkspacePrompt { create: bool },
    WorkspaceSet,
}

/// One line / completion event from a streaming `buck2 build`.
#[derive(Debug, Clone)]
enum BuildEvent {
    Line(String),
    Done(bool),
}

/// What got right-clicked, for the context menu.
#[derive(Debug, Clone)]
enum CtxKind {
    Entry { path: PathBuf, is_dir: bool },
    Target(usize),
    Package(String),
    /// The menu morphed into a "name this new file/folder" prompt.
    NewEntry { dir: PathBuf, folder: bool },
    /// The titlebar File menu (reuses the context-menu plumbing).
    FileMenu,
    /// Path prompt for creating/opening a workspace.
    Workspace { create: bool },
}

/// Mirror cosmic-text's keep-the-cursor-in-view behaviour so the gutter
/// tracks jumps the same way the editor does.
fn ensure_cursor_visible(buf: &mut Buffer, view_h: f32) {
    let top = buf.content.cursor().position.line as f32 * gutter::LINE_H;
    if top < buf.scroll {
        buf.scroll = top;
    } else if top + gutter::LINE_H > buf.scroll + view_h {
        buf.scroll = top + gutter::LINE_H - view_h;
    }
}

/// Stream `buck2 build` output line by line. Plain reader threads feed the
/// iced channel; the stream closes once every sender clone is dropped.
fn build_stream(
    root: PathBuf,
    label: String,
) -> impl iced::futures::Stream<Item = BuildEvent> {
    use iced::futures::SinkExt;
    use iced::futures::channel::mpsc::Sender;

    fn pump(
        reader: impl std::io::Read + Send + 'static,
        mut tx: Sender<BuildEvent>,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(reader)
                .lines()
                .map_while(Result::ok)
            {
                let _ = iced::futures::executor::block_on(tx.send(BuildEvent::Line(line)));
            }
        })
    }

    iced::stream::channel(100, async move |mut out: Sender<BuildEvent>| {
        let mut child = match ske::buck::build_child(&root, &label) {
            Ok(child) => child,
            Err(e) => {
                let _ = out
                    .send(BuildEvent::Line(format!("failed to run buck2: {e}")))
                    .await;
                let _ = out.send(BuildEvent::Done(false)).await;
                return;
            }
        };
        let stdout = child.stdout.take().map(|p| pump(p, out.clone()));
        let stderr = child.stderr.take().map(|p| pump(p, out.clone()));
        std::thread::spawn(move || {
            // Drain both pipes fully before reporting completion.
            if let Some(h) = stdout {
                let _ = h.join();
            }
            if let Some(h) = stderr {
                let _ = h.join();
            }
            let ok = child.wait().map(|s| s.success()).unwrap_or(false);
            let _ = iced::futures::executor::block_on(out.send(BuildEvent::Done(ok)));
        });
    })
}

fn syntax_for(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if matches!(name.as_str(), "BUCK" | "BUILD" | "BUILD.bazel" | "WORKSPACE") {
        return "py".into(); // starlark: python grammar is close enough for now
    }
    match path.extension().and_then(|e| e.to_str()) {
        Some("bzl" | "bxl") => "py".into(),
        Some(ext) => ext.to_lowercase(),
        None => "txt".into(),
    }
}

fn language_name(syntax: &str) -> &'static str {
    match syntax {
        "rs" => "Rust",
        "py" => "Starlark/Python",
        "toml" => "TOML",
        "md" => "Markdown",
        "json" => "JSON",
        "sh" | "fish" => "Shell",
        "c" | "h" => "C",
        "cc" | "cpp" | "hpp" => "C++",
        "js" => "JavaScript",
        "ts" => "TypeScript",
        _ => "Plain Text",
    }
}

impl Skyde {
    fn new() -> (Self, Task<Message>) {
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let tree = tree::read_dir(&root);
        let buck_root = ske::buck::find_root(&root);
        let load = match buck_root.clone() {
            Some(r) => load_targets(r),
            None => Task::none(),
        };
        let templates = load_templates(&root);
        let index = ske::index::scan(&root, &templates);
        let status = if templates.is_empty() {
            "ready (no templates found)".into()
        } else {
            format!(
                "ready · templates: {}",
                templates
                    .manifests
                    .iter()
                    .map(|(m, _)| m.template.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let app = Self {
            root,
            tree,
            selected: None,
            open: Vec::new(),
            active: None,
            view_mode: if std::env::args().any(|a| a == "--targets") {
                ViewMode::Targets
            } else {
                ViewMode::Files
            },
            status,
            window_id: None,
            maximized: false,
            buck_root,
            templates,
            index,
            new_target_name: String::new(),
            new_target_rule: None,
            targets: Vec::new(),
            targets_err: None,
            collapsed_pkgs: std::collections::HashSet::new(),
            selected_target: None,
            console: None,
            building: false,
            editor_text_h: std::cell::Cell::new(600.0),
            cursor: Point::ORIGIN,
            context: None,
            new_entry: String::new(),
        };
        // `skyde path/to/file` opens it on startup.
        let open = std::env::args()
            .skip(1)
            .filter(|a| !a.starts_with("--"))
            .map(PathBuf::from)
            .find(|p| p.is_file())
            .map(|p| Task::done(Message::Open(p)))
            .unwrap_or_else(Task::none);
        (app, Task::batch([load, open]))
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            window::open_events().map(Message::WindowOpened),
            iced::event::listen_with(|event, _status, _window| match event {
                iced::Event::Keyboard(keyboard::Event::KeyPressed {
                    key, modifiers, ..
                }) if modifiers.command() => match key.as_ref() {
                    keyboard::Key::Character("s") => Some(Message::Save),
                    keyboard::Key::Character("n") => Some(Message::NewFile),
                    keyboard::Key::Character("w") => Some(Message::CloseActiveTab),
                    _ => None,
                },
                iced::Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    ..
                }) => Some(Message::CloseContext),
                _ => None,
            }),
        ])
    }

    fn title(&self) -> String {
        match self.active.and_then(|i| self.open.get(i)) {
            Some(buf) => format!("skyde — {}", buf.title()),
            None => "skyde".into(),
        }
    }

    fn theme(&self) -> Theme {
        Theme::custom(
            "skedit",
            iced::theme::Palette {
                background: BG_EDITOR,
                text: TEXT,
                primary: ACCENT,
                success: GREEN,
                warning: YELLOW,
                danger: RED,
            },
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Edit(action) => {
                if let Some(buf) = self.active.and_then(|i| self.open.get_mut(i)) {
                    let view_h = self.editor_text_h.get();
                    let scrolled = if let text_editor::Action::Scroll { lines } = &action {
                        buf.scroll += *lines as f32 * gutter::LINE_H;
                        true
                    } else {
                        if action.is_edit() {
                            buf.dirty = true;
                        }
                        false
                    };
                    buf.content.perform(action);
                    // Mirror cosmic-text so the gutter stays in sync: clamp to
                    // the content, and keep the cursor line in view on any
                    // action that isn't a wheel scroll.
                    let max = (buf.content.line_count() as f32 * gutter::LINE_H - view_h).max(0.0);
                    buf.scroll = buf.scroll.clamp(0.0, max);
                    if !scrolled {
                        ensure_cursor_visible(buf, view_h);
                    }
                }
            }
            Message::Open(path) => {
                self.selected = Some(path.clone());
                if let Some(i) = self
                    .open
                    .iter()
                    .position(|b| b.path.as_deref() == Some(&path))
                {
                    self.active = Some(i);
                    return Task::none();
                }
                match std::fs::read_to_string(&path) {
                    Ok(src) => {
                        self.open.push(Buffer {
                            syntax: syntax_for(&path),
                            content: text_editor::Content::with_text(&src),
                            dirty: false,
                            path: Some(path),
                            scroll: 0.0,
                        });
                        self.active = Some(self.open.len() - 1);
                        self.status = "opened".into();
                    }
                    Err(e) => {
                        self.status = format!("open failed: {e}");
                    }
                }
            }
            Message::ToggleDir(path) => {
                tree::toggle(&mut self.tree, &path);
                self.selected = Some(path);
            }
            Message::SelectTab(i) => {
                if i < self.open.len() {
                    self.active = Some(i);
                }
            }
            Message::CloseTab(i) => {
                if i < self.open.len() {
                    let buf = self.open.remove(i);
                    if buf.dirty {
                        self.status = format!("closed {} (unsaved changes discarded)", buf.title());
                    }
                    self.active = match self.active {
                        Some(_) if self.open.is_empty() => None,
                        Some(a) if a > i => Some(a - 1),
                        Some(a) if a >= self.open.len() => Some(self.open.len() - 1),
                        other => other,
                    };
                }
            }
            Message::CloseActiveTab => {
                if let Some(i) = self.active {
                    return self.update(Message::CloseTab(i));
                }
            }
            Message::Save => {
                if let Some(buf) = self.active.and_then(|i| self.open.get_mut(i)) {
                    match &buf.path {
                        Some(p) => match std::fs::write(p, buf.content.text()) {
                            Ok(()) => {
                                buf.dirty = false;
                                self.status = format!("saved {}", p.display());
                            }
                            Err(e) => self.status = format!("save failed: {e}"),
                        },
                        None => {
                            self.status = "untitled buffer: save-as not implemented yet".into();
                        }
                    }
                }
            }
            Message::NewFile => {
                self.open.push(Buffer::untitled());
                self.active = Some(self.open.len() - 1);
                self.status = "new file".into();
            }
            Message::RefreshTree => {
                self.reload_tree();
                self.templates = load_templates(&self.root);
                self.index = ske::index::scan(&self.root, &self.templates);
                self.status = "tree + templates + index refreshed".into();
                if let Some(root) = self.buck_root.clone() {
                    return load_targets(root);
                }
            }
            Message::Build => {
                let label = self
                    .selected_target
                    .and_then(|i| self.all_targets().get(i))
                    .map(|t| t.label.clone())
                    .unwrap_or_else(|| "//...".into());
                return self.update(Message::BuildTarget(label));
            }
            Message::TargetsLoaded(Ok(targets)) => {
                self.status = format!("{} targets", targets.len());
                self.targets = targets;
                self.targets_err = None;
            }
            Message::TargetsLoaded(Err(e)) => {
                self.targets_err = Some(e);
                self.status = "buck2 target listing failed".into();
            }
            Message::SelectTarget(i) => {
                if i < self.all_targets().len() {
                    self.selected_target = Some(i);
                }
            }
            Message::TogglePkg(pkg) => {
                if !self.collapsed_pkgs.remove(&pkg) {
                    self.collapsed_pkgs.insert(pkg);
                }
            }
            Message::NewTargetNameChanged(s) => {
                self.new_target_name = s;
            }
            Message::NewTargetRulePicked(r) => {
                self.new_target_rule = Some(r);
            }
            Message::CreateTarget => {
                let name = self.new_target_name.trim().to_string();
                let Some(call) = self.new_target_rule.clone() else {
                    self.status = "pick a rule type first".into();
                    return Task::none();
                };
                if name.is_empty() {
                    self.status = "target needs a name".into();
                    return Task::none();
                }
                let pkg_dir = self.selected_dir();
                let buck_file = existing_buck_file(&pkg_dir).unwrap_or(pkg_dir.join("BUCK"));
                let source = std::fs::read_to_string(&buck_file).unwrap_or_default();
                match ske::create_rule(&source, &call, &name) {
                    Ok(new_src) => match std::fs::write(&buck_file, new_src) {
                        Ok(()) => {
                            self.status = format!("created {call} '{name}' in {}", buck_file.display());
                            self.new_target_name.clear();
                            self.after_buck_edit(&buck_file);
                        }
                        Err(e) => self.status = format!("write failed: {e}"),
                    },
                    Err(e) => self.status = format!("create failed: {e:?}"),
                }
            }
            Message::DeleteTarget(label) => {
                let Some((buck_file, t)) = self.locate(&label) else {
                    self.status = format!("cannot locate BUCK file for {label}");
                    return Task::none();
                };
                let (call, name) = (t.rule_type.clone(), t.name.clone());
                match std::fs::read_to_string(&buck_file)
                    .map_err(|e| format!("{e}"))
                    .and_then(|src| {
                        ske::remove_rule(&src, &call, &name).map_err(|e| format!("{e:?}"))
                    })
                    .and_then(|new_src| {
                        std::fs::write(&buck_file, new_src).map_err(|e| format!("{e}"))
                    }) {
                    Ok(()) => {
                        self.status = format!("deleted {label}");
                        self.selected_target = None;
                        self.after_buck_edit(&buck_file);
                    }
                    Err(e) => self.status = format!("delete failed: {e}"),
                }
            }
            Message::AddSelectedFile(label) => {
                self.add_selected_file(&label, None);
            }
            Message::AddFileAs { label, attr } => {
                self.add_selected_file(&label, Some(&attr));
            }
            Message::BuildTarget(label) => {
                let Some(root) = self.buck_root.clone() else {
                    self.status = "no buck2 workspace (.buckconfig not found)".into();
                    return Task::none();
                };
                if self.building {
                    self.status = "a build is already running".into();
                    return Task::none();
                }
                self.building = true;
                self.status = format!("building {label}…");
                self.console = Some(format!("$ buck2 build {label}\n"));
                return Task::run(build_stream(root, label), Message::BuildEvent);
            }
            Message::BuildEvent(event) => match event {
                BuildEvent::Line(line) => {
                    if let Some(console) = &mut self.console {
                        console.push_str(&line);
                        console.push('\n');
                    }
                }
                BuildEvent::Done(ok) => {
                    self.building = false;
                    self.status = if ok { "build ok" } else { "build failed" }.into();
                }
            },
            Message::CloseConsole => {
                self.console = None;
            }
            Message::Menu(name) => {
                if name == "File" {
                    self.context = match self.context.take() {
                        Some((_, CtxKind::FileMenu)) => None,
                        _ => Some((self.cursor, CtxKind::FileMenu)),
                    };
                } else {
                    self.status = format!("{name} menu: not implemented yet");
                }
            }
            Message::ChangeView(mode) => {
                self.view_mode = mode;
                self.status = format!("view: {mode}");
            }
            Message::WindowOpened(id) => {
                self.window_id = Some(id);
            }
            Message::DragWindow => {
                if let Some(id) = self.window_id {
                    return window::drag(id);
                }
            }
            Message::MinimizeWindow => {
                if let Some(id) = self.window_id {
                    return window::minimize(id, true);
                }
            }
            Message::ToggleMaximize => {
                if let Some(id) = self.window_id {
                    self.maximized = !self.maximized;
                    return window::maximize(id, self.maximized);
                }
            }
            Message::CloseWindow => {
                if let Some(id) = self.window_id {
                    return window::close(id);
                }
            }
            Message::ResizeWindow(direction) => {
                if let Some(id) = self.window_id {
                    return window::drag_resize(id, direction);
                }
            }
            Message::MouseMoved(p) => {
                self.cursor = p;
            }
            Message::OpenContext(kind) => {
                // Right-click selects, like every file manager.
                match &kind {
                    CtxKind::Entry { path, .. } => self.selected = Some(path.clone()),
                    CtxKind::Target(i) => self.selected_target = Some(*i),
                    CtxKind::Package(_)
                    | CtxKind::NewEntry { .. }
                    | CtxKind::FileMenu
                    | CtxKind::Workspace { .. } => {}
                }
                self.context = Some((self.cursor, kind));
            }
            Message::CtxAction(msg) => {
                self.context = None;
                return self.update(*msg);
            }
            Message::CloseContext => {
                self.context = None;
            }
            Message::NewTargetIn(dir) => {
                self.selected = Some(dir);
                self.view_mode = ViewMode::Targets;
                self.status = "pick a rule and a name in the sidebar form".into();
            }
            Message::OpenTargetSource(i) => {
                self.context = None;
                let Some(t) = self.all_targets().get(i) else {
                    return Task::none();
                };
                let (label, rule, name) = (t.label.clone(), t.rule_name().to_owned(), t.name.clone());
                let Some((file, _)) = self.locate(&label) else {
                    self.status = format!("cannot find the BUCK file for {label}");
                    return Task::none();
                };
                let task = self.update(Message::Open(file.clone()));
                self.view_mode = ViewMode::Files;
                if let Some(buf) = self
                    .active
                    .and_then(|j| self.open.get_mut(j))
                    .filter(|b| b.path.as_deref() == Some(file.as_path()))
                {
                    let src = buf.content.text();
                    match ske::rule_line(&src, &rule, &name) {
                        Ok(line) => {
                            buf.content
                                .perform(text_editor::Action::Move(text_editor::Motion::DocumentStart));
                            for _ in 0..line {
                                buf.content.perform(text_editor::Action::Move(text_editor::Motion::Down));
                            }
                            buf.content.perform(text_editor::Action::SelectLine);
                            let view_h = self.editor_text_h.get();
                            let max =
                                (buf.content.line_count() as f32 * gutter::LINE_H - view_h).max(0.0);
                            buf.scroll = buf.scroll.clamp(0.0, max);
                            ensure_cursor_visible(buf, view_h);
                        }
                        Err(e) => self.status = format!("found the file, not the rule: {e}"),
                    }
                }
                return task;
            }
            Message::NewEntryPrompt { dir, folder } => {
                self.new_entry.clear();
                self.context = Some((self.cursor, CtxKind::NewEntry { dir, folder }));
                return iced::widget::operation::focus(NEW_ENTRY_INPUT);
            }
            Message::NewEntryName(name) => {
                self.new_entry = name;
            }
            Message::NewEntryCreate => {
                let Some((_, CtxKind::NewEntry { dir, folder })) = self.context.take() else {
                    return Task::none();
                };
                let name = self.new_entry.trim().to_owned();
                if name.is_empty() || name.contains(std::path::is_separator) {
                    self.status = "give it a plain name (no path separators)".into();
                    return Task::none();
                }
                let path = dir.join(&name);
                if path.exists() {
                    self.status = format!("{name} already exists");
                    return Task::none();
                }
                let result = if folder {
                    std::fs::create_dir(&path)
                } else {
                    std::fs::write(&path, "")
                };
                match result {
                    Ok(()) => {
                        self.reload_tree();
                        self.index = ske::index::scan(&self.root, &self.templates);
                        self.selected = Some(path.clone());
                        self.status = format!("created {name}");
                        if !folder {
                            return self.update(Message::Open(path));
                        }
                    }
                    Err(e) => self.status = format!("create failed: {e}"),
                }
            }
            Message::SelectTargetByLabel(label) => {
                self.selected_target = self.all_targets().iter().position(|t| t.label == label);
                self.status = format!("active target: {label}");
            }
            Message::WorkspacePrompt { create } => {
                self.new_entry = if create {
                    self.root
                        .parent()
                        .unwrap_or(&self.root)
                        .join("my-project")
                        .display()
                        .to_string()
                } else {
                    self.root.display().to_string()
                };
                self.context = Some((self.cursor, CtxKind::Workspace { create }));
                return iced::widget::operation::focus(NEW_ENTRY_INPUT);
            }
            Message::WorkspaceSet => {
                let Some((_, CtxKind::Workspace { create })) = self.context.take() else {
                    return Task::none();
                };
                let input = self.new_entry.trim().to_owned();
                if input.is_empty() {
                    return Task::none();
                }
                let path = match input.strip_prefix("~/") {
                    Some(rest) => match std::env::var_os("HOME") {
                        Some(home) => PathBuf::from(home).join(rest),
                        None => PathBuf::from(&input),
                    },
                    None => PathBuf::from(&input),
                };
                if create {
                    if let Err(e) = std::fs::create_dir_all(&path) {
                        self.status = format!("cannot create {}: {e}", path.display());
                        return Task::none();
                    }
                    match ske::buck::init(&path) {
                        Ok(()) => self.status = "workspace created (buck2 init)".into(),
                        Err(e) => {
                            self.status = format!("folder created; buck2 init failed: {e}");
                        }
                    }
                } else if !path.is_dir() {
                    self.status = format!("{} is not a folder", path.display());
                    return Task::none();
                }
                return self.switch_root(path);
            }
        }
        Task::none()
    }

    /// Point the whole app at a different workspace root.
    fn switch_root(&mut self, root: PathBuf) -> Task<Message> {
        self.root = root;
        self.tree = tree::read_dir(&self.root);
        self.buck_root = ske::buck::find_root(&self.root);
        self.templates = load_templates(&self.root);
        self.index = ske::index::scan(&self.root, &self.templates);
        self.targets.clear();
        self.targets_err = None;
        self.selected = None;
        self.selected_target = None;
        self.collapsed_pkgs.clear();
        self.status = format!("workspace: {}", self.root.display());
        match self.buck_root.clone() {
            Some(r) => load_targets(r),
            None => Task::none(),
        }
    }

    /// Add the currently selected file to `label` — under `attr` if given,
    /// otherwise wherever the template routes its extension.
    fn add_selected_file(&mut self, label: &str, attr: Option<&str>) {
        let Some(file) = self.selected.clone().filter(|p| p.is_file()) else {
            self.status = "select a file in the Files view first".into();
            return;
        };
        let Some((buck_file, t)) = self.locate(label) else {
            self.status = format!("cannot locate BUCK file for {label}");
            return;
        };
        let rule_name = t.rule_name().to_owned();
        let target_name = t.name.clone();
        let package = t.package.clone();
        let pkg_dir = buck_file.parent().unwrap_or(&self.root).to_path_buf();
        let Ok(rel) = file.strip_prefix(&pkg_dir) else {
            self.status = format!("{} is outside package {package}", file.display());
            return;
        };
        let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
        let attr = match attr {
            Some(a) => a.to_owned(),
            None => self
                .templates
                .rule(&rule_name)
                .and_then(|(_, r)| r.attr_for_ext(ext))
                .unwrap_or("srcs")
                .to_owned(),
        };
        let selector = ske::RuleSelector {
            rule_name,
            attr: attr.clone(),
            name: Some(target_name),
        };
        let entry = rel.to_string_lossy().replace('\\', "/");
        match std::fs::read_to_string(&buck_file)
            .map_err(|e| format!("{e}"))
            .and_then(|src| ske::add_entry(&src, &selector, &entry).map_err(|e| format!("{e:?}")))
            .and_then(|new_src| std::fs::write(&buck_file, new_src).map_err(|e| format!("{e}")))
        {
            Ok(()) => {
                self.status = format!("added {entry} to {label} ({attr})");
                self.after_buck_edit(&buck_file);
            }
            Err(e) => self.status = format!("add failed: {e}"),
        }
    }

    /// Re-read the file tree, keeping directories expanded.
    fn reload_tree(&mut self) {
        fn expanded_dirs(nodes: &[tree::Node], out: &mut Vec<PathBuf>) {
            for n in nodes {
                if let tree::Kind::Dir {
                    expanded: true,
                    children,
                } = &n.kind
                {
                    out.push(n.path.clone());
                    if let Some(children) = children {
                        expanded_dirs(children, out);
                    }
                }
            }
        }
        let mut expanded = Vec::new();
        expanded_dirs(&self.tree, &mut expanded);
        self.tree = tree::read_dir(&self.root);
        // Parents come before children, so each toggle finds its node loaded.
        for dir in expanded {
            tree::toggle(&mut self.tree, &dir);
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let main_pane = match self.view_mode {
            ViewMode::Files => self.editor_pane(),
            ViewMode::Targets => self.graph_pane(),
        };
        let mut center = column![main_pane].spacing(6).height(Fill);
        if self.console.is_some() {
            center = center.push(self.console_panel());
        }

        let mut content = row![self.rail(), self.sidebar(), center]
            .spacing(6)
            .height(Fill);
        if self.view_mode == ViewMode::Targets {
            if let Some(t) = self.selected_target.and_then(|i| self.all_targets().get(i)) {
                content = content.push(self.inspector(t));
            }
        }

        let body = container(content)
            .padding([6, 6])
            .height(Fill)
            .width(Fill)
            .style(canvas_bg);

        let ui = column![self.titlebar(), body, self.status_bar()];

        // Stack so the context menu and window-resize grips can float on top.
        // The mouse_area only records the cursor for context-menu placement.
        let mut layers = stack![mouse_area(ui).on_move(Message::MouseMoved)]
            .width(Fill)
            .height(Fill);
        if let Some((at, kind)) = &self.context {
            layers = layers.push(
                mouse_area(Space::new().width(Fill).height(Fill))
                    .on_press(Message::CloseContext)
                    .on_right_press(Message::CloseContext),
            );
            layers = layers.push(self.context_menu(*at, kind));
        }
        if !self.maximized {
            for grip in resize_grips() {
                layers = layers.push(grip);
            }
        }
        layers.into()
    }

    fn context_menu(&self, at: Point, kind: &CtxKind) -> Element<'_, Message> {
        let item = |label: String, msg: Message| -> Element<'_, Message> {
            button(text(label).size(12).color(TEXT))
                .on_press(Message::CtxAction(Box::new(msg)))
                .width(Fill)
                .padding([5, 10])
                .style(menu_button)
                .into()
        };
        let mut items: Vec<Element<'_, Message>> = Vec::new();
        match kind {
            CtxKind::Entry { path, is_dir } => {
                let is_buck = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| matches!(n, "BUCK" | "BUILD" | "BUILD.bazel"));
                if *is_dir {
                    items.push(item(
                        "new file…".into(),
                        Message::NewEntryPrompt {
                            dir: path.clone(),
                            folder: false,
                        },
                    ));
                    items.push(item(
                        "new folder…".into(),
                        Message::NewEntryPrompt {
                            dir: path.clone(),
                            folder: true,
                        },
                    ));
                    items.push(item(
                        "new build target here…".into(),
                        Message::NewTargetIn(path.clone()),
                    ));
                } else if is_buck {
                    // Whole-package build label: "//demo:" builds every
                    // target this BUCK file defines.
                    let pkg = self
                        .index
                        .buck_files
                        .iter()
                        .find(|(_, f)| f.as_path() == path.as_path())
                        .map(|(p, _)| p.clone())
                        .or_else(|| {
                            let base = self.buck_root.as_deref().unwrap_or(&self.root);
                            path.parent()
                                .and_then(|d| d.strip_prefix(base).ok())
                                .map(|rel| format!("//{}", rel.display()))
                        });
                    if let Some(pkg) = pkg {
                        items.push(item(
                            format!("build {pkg}:"),
                            Message::BuildTarget(format!("{pkg}:")),
                        ));
                    }
                    if let Some(dir) = path.parent() {
                        items.push(item(
                            "add target…".into(),
                            Message::NewTargetIn(dir.to_path_buf()),
                        ));
                    }
                } else {
                    let owners = self.index.targets_of(path);
                    if let Some(t) = self
                        .selected_target
                        .and_then(|i| self.all_targets().get(i))
                        .filter(|t| !owners.contains(&t.label.as_str()))
                    {
                        // One item per file-carrying attr the template knows
                        // for this rule (srcs, hdrs, …), so a .h can go to
                        // either explicitly.
                        let mut attrs: Vec<String> = Vec::new();
                        if let Some((_, r)) = self.templates.rule(t.rule_name()) {
                            attrs.extend(r.srcs.iter().map(|a| a.attr.clone()));
                            attrs.extend(r.hdrs.iter().map(|a| a.attr.clone()));
                        }
                        if attrs.is_empty() {
                            attrs.push("srcs".into());
                        }
                        attrs.dedup();
                        for attr in attrs {
                            items.push(item(
                                format!("add to {} ({attr})", t.name),
                                Message::AddFileAs {
                                    label: t.label.clone(),
                                    attr,
                                },
                            ));
                        }
                    }
                    if let Some(dir) = path.parent() {
                        items.push(item(
                            "new file…".into(),
                            Message::NewEntryPrompt {
                                dir: dir.to_path_buf(),
                                folder: false,
                            },
                        ));
                        items.push(item(
                            "new build target here…".into(),
                            Message::NewTargetIn(dir.to_path_buf()),
                        ));
                    }
                }
            }
            CtxKind::Target(i) => {
                if let Some(t) = self.all_targets().get(*i) {
                    items.push(item(
                        format!("build {}", t.name),
                        Message::BuildTarget(t.label.clone()),
                    ));
                    items.push(item("open BUCK file".into(), Message::OpenTargetSource(*i)));
                    items.push(item(
                        "delete target".into(),
                        Message::DeleteTarget(t.label.clone()),
                    ));
                }
            }
            CtxKind::Package(pkg) => {
                let dir = self
                    .index
                    .buck_files
                    .get(pkg)
                    .and_then(|f| Some(f.parent()?.to_path_buf()))
                    .or_else(|| {
                        pkg.split_once("//").map(|(_, p)| {
                            self.buck_root.as_deref().unwrap_or(&self.root).join(p)
                        })
                    });
                if let Some(dir) = dir {
                    items.push(item(
                        "new target in this package…".into(),
                        Message::NewTargetIn(dir),
                    ));
                }
            }
            CtxKind::NewEntry { dir, folder } => {
                let rel = dir.strip_prefix(&self.root).unwrap_or(dir);
                items.push(
                    container(
                        text(format!(
                            "new {} in {}/",
                            if *folder { "folder" } else { "file" },
                            rel.display()
                        ))
                        .size(11)
                        .color(MUTED),
                    )
                    .padding([2, 6])
                    .into(),
                );
                items.push(
                    text_input("name", &self.new_entry)
                        .id(NEW_ENTRY_INPUT)
                        .on_input(Message::NewEntryName)
                        .on_submit(Message::NewEntryCreate)
                        .size(12)
                        .padding([4, 8])
                        .into(),
                );
            }
            CtxKind::FileMenu => {
                items.push(item("new file (ctrl+n)".into(), Message::NewFile));
                items.push(item("save (ctrl+s)".into(), Message::Save));
                items.push(item(
                    "new workspace…".into(),
                    Message::WorkspacePrompt { create: true },
                ));
                items.push(item(
                    "open workspace…".into(),
                    Message::WorkspacePrompt { create: false },
                ));
            }
            CtxKind::Workspace { create } => {
                items.push(
                    container(
                        text(if *create {
                            "create a workspace at (runs buck2 init):"
                        } else {
                            "open the workspace at:"
                        })
                        .size(11)
                        .color(MUTED),
                    )
                    .padding([2, 6])
                    .into(),
                );
                items.push(
                    text_input("path", &self.new_entry)
                        .id(NEW_ENTRY_INPUT)
                        .on_input(Message::NewEntryName)
                        .on_submit(Message::WorkspaceSet)
                        .size(12)
                        .padding([4, 8])
                        .into(),
                );
            }
        }
        let width = match kind {
            // Paths need room.
            CtxKind::Workspace { .. } => 340.0,
            _ => 200.0,
        };
        container(
            container(column(items).width(Length::Fixed(width)))
                .padding(4)
                .style(context_panel),
        )
        .padding(iced::Padding {
            top: at.y,
            left: at.x,
            right: 0.0,
            bottom: 0.0,
        })
        .into()
    }

    /// Nodes for the graph canvas: every known target, plus one external node
    /// per unresolved dep label (e.g. cargo/prelude deps).
    fn graph_nodes(&self) -> Vec<graph::Node> {
        let targets = self.all_targets();
        let mut nodes: Vec<graph::Node> = targets
            .iter()
            .enumerate()
            .map(|(i, t)| graph::Node {
                name: t.name.clone(),
                sub: t.rule_name().to_string(),
                color: kind_icon(self.target_kind(t)).1,
                external: false,
                target_idx: Some(i),
                deps: Vec::new(),
            })
            .collect();

        let mut externs: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for i in 0..targets.len() {
            for d in targets[i].deps.clone() {
                let resolved = if let Some(name) = d.strip_prefix(':') {
                    targets
                        .iter()
                        .position(|o| o.package == targets[i].package && o.name == name)
                } else {
                    targets.iter().position(|o| o.label == d)
                };
                let ni = match resolved {
                    Some(j) => j,
                    None => *externs.entry(d.clone()).or_insert_with(|| {
                        let display = d.rsplit(':').next().unwrap_or(&d).to_string();
                        nodes.push(graph::Node {
                            name: display,
                            sub: "extern".into(),
                            color: MUTED,
                            external: true,
                            target_idx: None,
                            deps: Vec::new(),
                        });
                        nodes.len() - 1
                    }),
                };
                nodes[i].deps.push(ni);
            }
        }
        nodes
    }

    fn graph_pane(&self) -> Element<'_, Message> {
        let header = container(
            row![
                text("Build graph").size(13).color(TEXT),
                text(format!("{} targets", self.all_targets().len()))
                    .size(12)
                    .color(MUTED),
                space::horizontal(),
                button(text("Build all").size(12).color(ACCENT))
                    .on_press(Message::BuildTarget("//...".into()))
                    .padding([4, 12])
                    .style(accent_outline_button)
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .padding([6, 10]),
        )
        .width(Fill)
        .style(tabstrip_bg);

        let nodes = self.graph_nodes();
        let body: Element<'_, Message> = if nodes.is_empty() {
            container(
                text("no targets to graph\n(create one in the sidebar)")
                    .size(13)
                    .color(MUTED),
            )
            .center(Fill)
            .into()
        } else {
            let positions = graph::layout(&nodes);
            iced::widget::canvas(graph::Program {
                nodes,
                positions,
                selected: self.selected_target,
            })
            .width(Fill)
            .height(Fill)
            .into()
        };

        container(column![header, rule::horizontal(1), body].height(Fill))
            .width(Fill)
            .height(Fill)
            .clip(true)
            .style(editor_panel)
            .into()
    }

    fn console_panel(&self) -> Element<'_, Message> {
        let log = self.console.as_deref().unwrap_or("");
        let header = row![
            text(if self.building {
                "build output (running…)"
            } else {
                "build output"
            })
            .size(12)
            .color(MUTED),
            space::horizontal(),
            button(text("✕").size(10).color(MUTED))
                .on_press(Message::CloseConsole)
                .padding([2, 6])
                .style(menu_button),
        ]
        .align_y(Alignment::Center);

        container(
            column![
                header,
                // anchored bottom: follows new output while streaming
                scrollable(text(log).size(12).font(NERD_FONT).color(TEXT))
                    .anchor_bottom()
                    .height(Fill)
            ]
            .spacing(4)
            .padding(8),
        )
        .width(Fill)
        .height(Length::Fixed(180.0))
        .style(editor_panel)
        .into()
    }

    /// Targets to show: prefer live buck2 output, fall back to our own parse
    /// of the BUCK files (works without buck2 installed).
    fn all_targets(&self) -> &[ske::buck::Target] {
        if self.targets.is_empty() {
            &self.index.targets
        } else {
            &self.targets
        }
    }

    /// Directory the "new target" form creates into: the selected tree entry
    /// (itself if a directory, its parent if a file), else the workspace root.
    fn selected_dir(&self) -> PathBuf {
        match &self.selected {
            Some(p) if p.is_dir() => p.clone(),
            Some(p) => p.parent().unwrap_or(&self.root).to_path_buf(),
            None => self.root.clone(),
        }
    }

    /// Find a target by label and the BUCK file that defines it.
    fn locate(&self, label: &str) -> Option<(PathBuf, &ske::buck::Target)> {
        let t = self.all_targets().iter().find(|t| t.label == label)?;
        if let Some(f) = self.index.buck_files.get(&t.package) {
            return Some((f.clone(), t));
        }
        // buck2 packages look like "cell//path"; ours like "//path"
        let rel = t.package.split_once("//").map(|(_, p)| p)?;
        let base = self.buck_root.as_deref().unwrap_or(&self.root);
        existing_buck_file(&base.join(rel)).map(|f| (f, t))
    }

    /// Re-scan the index and reload the edited BUCK file if it's open.
    fn after_buck_edit(&mut self, buck_file: &Path) {
        self.index = ske::index::scan(&self.root, &self.templates);
        let mut warn_dirty = false;
        if let Some(buf) = self
            .open
            .iter_mut()
            .find(|b| b.path.as_deref() == Some(buck_file))
        {
            if buf.dirty {
                warn_dirty = true;
            } else if let Ok(src) = std::fs::read_to_string(buck_file) {
                buf.content = text_editor::Content::with_text(&src);
                buf.scroll = 0.0;
            }
        }
        if warn_dirty {
            self.status = format!(
                "{} (note: open buffer has unsaved edits, not reloaded)",
                self.status
            );
        }
        if let Some(root) = self.buck_root.clone() {
            let _ = root; // buck2 target list refresh happens on manual refresh
        }
    }

    fn target_kind(&self, t: &ske::buck::Target) -> ske::buck::Kind {
        self.templates
            .kind_of(t.rule_name())
            .unwrap_or_else(|| t.kind())
    }

    fn inspector(&self, t: &ske::buck::Target) -> Element<'_, Message> {
        let (icon, color) = kind_icon(self.target_kind(t));

        let attr = |label: &'static str| text(label).size(11).color(MUTED);
        let mut body = column![
            row![
                text(icon).size(16).font(NERD_FONT).color(color),
                text(t.name.clone()).size(15).color(TEXT)
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            text(t.rule_name().to_owned()).size(12).color(MUTED),
            Space::new().height(Length::Fixed(8.0)),
            attr("target"),
            text(t.label.clone()).size(12).color(TEXT),
        ]
        .spacing(4);

        if !t.srcs.is_empty() {
            body = body.push(attr("srcs"));
            for s in &t.srcs {
                body = body.push(text(s.clone()).size(12).color(TEXT));
            }
        }
        if !t.deps.is_empty() {
            body = body.push(attr("deps"));
            for d in &t.deps {
                body = body.push(text(d.clone()).size(12).color(TEXT));
            }
        }
        if !t.visibility.is_empty() {
            body = body.push(attr("visibility"));
            body = body.push(text(t.visibility.join(", ")).size(12).color(TEXT));
        }

        let build = button(
            row![
                text(ICON_ROCKET).size(12).font(NERD_FONT).color(ACCENT),
                text(if self.building { "Building…" } else { "Build" })
                    .size(13)
                    .color(ACCENT)
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .on_press(Message::BuildTarget(t.label.clone()))
        .padding([6, 14])
        .width(Fill)
        .style(accent_outline_button);

        let mut actions = column![build].spacing(6);
        if let Some(f) = self.selected.as_ref().filter(|p| p.is_file()) {
            let fname = f
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            actions = actions.push(
                button(text(format!("add {fname}")).size(12).color(TEXT))
                    .on_press(Message::AddSelectedFile(t.label.clone()))
                    .padding([5, 12])
                    .width(Fill)
                    .style(menu_button),
            );
        }
        actions = actions.push(
            button(text("delete target").size(12).color(RED))
                .on_press(Message::DeleteTarget(t.label.clone()))
                .padding([5, 12])
                .width(Fill)
                .style(menu_button),
        );

        container(
            column![scrollable(body).height(Fill), actions]
                .spacing(8)
                .padding(10),
        )
        .width(Length::Fixed(260.0))
        .height(Fill)
        .clip(true)
        .style(sidebar_panel)
        .into()
    }

    fn titlebar(&self) -> Element<'_, Message> {
        let mut menus = row![].spacing(0).align_y(Alignment::Center);
        for name in ["File", "Edit", "Selection", "View", "Go"] {
            menus = menus.push(
                button(text(name).size(13).color(TEXT))
                    .on_press(Message::Menu(name))
                    .padding([4, 10])
                    .style(menu_button),
            );
        }
        let drag_area = mouse_area(
            container(space::horizontal())
                .height(Length::Fixed(34.0))
                .width(Fill),
        )
        .on_press(Message::DragWindow);

        let build_button = button(
            row![
                text(ICON_ROCKET).size(12).font(NERD_FONT).color(ACCENT),
                text("Build").size(13).color(ACCENT)
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .on_press(Message::Build)
        .padding([4, 14])
        .style(accent_outline_button);

        let window_controls = row![
            button(text("—").size(12).color(MUTED))
                .on_press(Message::MinimizeWindow)
                .padding([4, 12])
                .style(menu_button),
            button(
                text(if self.maximized { "❐" } else { "▢" })
                    .size(12)
                    .color(MUTED)
            )
            .on_press(Message::ToggleMaximize)
            .padding([4, 12])
            .style(menu_button),
            button(text("✕").size(12).color(MUTED))
                .on_press(Message::CloseWindow)
                .padding([4, 12])
                .style(close_button),
        ]
        .spacing(0)
        .align_y(Alignment::Center);

        container(
            row![menus, drag_area, build_button, window_controls]
                .align_y(Alignment::Center)
                .spacing(8)
                .padding([0, 4]),
        )
        .width(Fill)
        .style(canvas_bg)
        .into()
    }

    fn rail(&self) -> Element<'_, Message> {
        let item = |icon: &'static str, active: bool, msg: Message| {
            button(
                text(icon)
                    .size(16)
                    .font(NERD_FONT)
                    .color(if active { ACCENT } else { MUTED }),
            )
            .on_press(msg)
            .padding([6, 8])
            .style(move |_t: &Theme, status| rail_button(active, status))
        };

        container(
            column![
                item(
                    ICON_FILE,
                    self.view_mode == ViewMode::Files,
                    Message::ChangeView(ViewMode::Files)
                ),
                item(
                    ICON_TARGET_BUILD,
                    self.view_mode == ViewMode::Targets,
                    Message::ChangeView(ViewMode::Targets)
                ),
                space::vertical(),
                item(ICON_REFRESH, false, Message::RefreshTree),
            ]
            .spacing(4)
            .align_x(Alignment::Center),
        )
        .width(Length::Fixed(40.0))
        .height(Fill)
        .padding([4, 0])
        .style(canvas_bg)
        .into()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let root_name = self
            .root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.root.display().to_string());

        let header = container(
            row![
                text(root_name).size(13).color(TEXT),
                space::horizontal(),
                button(text(ICON_REFRESH).size(12).font(NERD_FONT).color(MUTED))
                    .on_press(Message::RefreshTree)
                    .padding([2, 6])
                    .style(menu_button),
            ]
            .align_y(Alignment::Center),
        )
        .padding([8, 10])
        .width(Fill);

        let body: Element<'_, Message> = match self.view_mode {
            ViewMode::Files => {
                let mut rows: Vec<Element<'_, Message>> = Vec::new();
                tree_rows(
                    &self.tree,
                    0,
                    self.selected.as_deref(),
                    &self.index,
                    &mut rows,
                );
                column![
                    scrollable(column(rows).spacing(1).padding([0, 6])).height(Fill),
                    container(self.active_target_picker()).padding(8)
                ]
                .into()
            }
            ViewMode::Targets => self.targets_body(),
        };

        container(column![header, body].width(Fill))
            .width(Length::Fixed(240.0))
            .height(Fill)
            .clip(true)
            .style(sidebar_panel)
            .into()
    }

    /// Bottom-of-the-file-tree dropdown: which target "add to …" acts on.
    fn active_target_picker(&self) -> Element<'_, Message> {
        let labels: Vec<String> = self.all_targets().iter().map(|t| t.label.clone()).collect();
        if labels.is_empty() {
            return Space::new().into();
        }
        let selected = self
            .selected_target
            .and_then(|i| self.all_targets().get(i))
            .map(|t| t.label.clone());
        column![
            text("active target").size(11).color(MUTED),
            pick_list(labels, selected, Message::SelectTargetByLabel)
                .placeholder("none")
                .text_size(12)
                .padding([4, 8])
                .width(Fill),
        ]
        .spacing(4)
        .into()
    }

    fn targets_body(&self) -> Element<'_, Message> {
        let targets = self.all_targets();
        if targets.is_empty() {
            let note: Element<'_, Message> = if let Some(err) = &self.targets_err {
                column![
                    text("buck2 targets failed:").size(13).color(RED),
                    text(err.clone()).size(12).color(MUTED)
                ]
                .spacing(6)
                .into()
            } else {
                text("no targets found\n(no BUCK/BUILD files in workspace)")
                    .size(13)
                    .color(MUTED)
                    .into()
            };
            return container(column![note, self.new_target_form()].spacing(16))
                .padding(12)
                .into();
        }

        let mut packages: std::collections::BTreeMap<&str, Vec<usize>> =
            std::collections::BTreeMap::new();
        for (i, t) in targets.iter().enumerate() {
            packages.entry(t.package.as_str()).or_default().push(i);
        }

        let mut rows: Vec<Element<'_, Message>> = Vec::new();
        for (pkg, indices) in packages {
            let collapsed = self.collapsed_pkgs.contains(pkg);
            let short = pkg.split_once("//").map(|(_, p)| p).unwrap_or(pkg);
            let pkg_row = button(
                row![
                    text(if collapsed {
                        ICON_CHEVRON_RIGHT
                    } else {
                        ICON_CHEVRON_DOWN
                    })
                    .size(12)
                    .font(NERD_FONT)
                    .color(MUTED),
                    text(format!("//{short}")).size(13).color(TEXT),
                    space::horizontal(),
                    text(indices.len().to_string()).size(11).color(MUTED)
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .on_press(Message::TogglePkg(pkg.to_owned()))
            .width(Fill)
            .padding([3, 8])
            .style(move |_t: &Theme, status| tree_button(false, status));
            rows.push(
                mouse_area(pkg_row)
                    .on_right_press(Message::OpenContext(CtxKind::Package(pkg.to_owned())))
                    .into(),
            );
            if collapsed {
                continue;
            }
            for i in indices {
                let t = &targets[i];
                let is_selected = self.selected_target == Some(i);
                let (icon, color) = kind_icon(self.target_kind(t));
                rows.push(
                    mouse_area(
                        button(
                            row![
                                Space::new().width(Length::Fixed(14.0)),
                                text(icon).size(12).font(NERD_FONT).color(color),
                                text(&t.name).size(13).color(TEXT)
                            ]
                            .spacing(6)
                            .align_y(Alignment::Center),
                        )
                        .on_press(Message::SelectTarget(i))
                        .width(Fill)
                        .padding([3, 8])
                        .style(move |_t: &Theme, status| tree_button(is_selected, status)),
                    )
                    .on_right_press(Message::OpenContext(CtxKind::Target(i)))
                    .into(),
                );
            }
        }
        column![
            scrollable(column(rows).spacing(1).padding([0, 6])).height(Fill),
            container(self.new_target_form()).padding(8)
        ]
        .into()
    }

    fn new_target_form(&self) -> Element<'_, Message> {
        let calls: Vec<String> = self
            .templates
            .manifests
            .iter()
            .flat_map(|(m, _)| m.lark.rules.iter().map(|r| r.call.clone()))
            .collect();
        if calls.is_empty() {
            return text("no templates loaded — cannot create targets")
                .size(12)
                .color(MUTED)
                .into();
        }
        let dir = self.selected_dir();
        let rel = dir.strip_prefix(&self.root).unwrap_or(&dir);
        column![
            text(format!("new target in //{}", rel.display()))
                .size(11)
                .color(MUTED),
            pick_list(calls, self.new_target_rule.clone(), Message::NewTargetRulePicked)
                .placeholder("rule")
                .text_size(12)
                .padding([4, 8])
                .width(Fill),
            row![
                text_input("name", &self.new_target_name)
                    .on_input(Message::NewTargetNameChanged)
                    .on_submit(Message::CreateTarget)
                    .size(12)
                    .padding([4, 8]),
                button(text("＋").size(13).color(ACCENT))
                    .on_press(Message::CreateTarget)
                    .padding([4, 10])
                    .style(accent_outline_button)
            ]
            .spacing(4)
            .align_y(Alignment::Center)
        ]
        .spacing(4)
        .into()
    }

    fn editor_pane(&self) -> Element<'_, Message> {
        let Some(buf) = self.active.and_then(|i| self.open.get(i)) else {
            return container(
                text("open a file from the tree\n(ctrl+n new · ctrl+s save · ctrl+w close)")
                    .size(14)
                    .color(MUTED),
            )
            .center(Fill)
            .style(editor_panel)
            .into();
        };

        let tabs = row(self.open.iter().enumerate().map(|(i, b)| {
            let active = Some(i) == self.active;
            let name = text(b.title())
                .size(13)
                .color(if active { TEXT } else { MUTED });
            let mut label = row![
                text(ICON_FILE).size(11).font(NERD_FONT).color(if active {
                    ACCENT
                } else {
                    MUTED
                }),
                name
            ]
            .spacing(6)
            .align_y(Alignment::Center);
            if b.dirty {
                label = label.push(text("●").size(9).color(ACCENT));
            }
            let select = button(label)
                .on_press(Message::SelectTab(i))
                .padding([6, 10])
                .style(move |_t: &Theme, status| tab_button(active, status));
            let close = button(text("✕").size(10).color(MUTED))
                .on_press(Message::CloseTab(i))
                .padding([6, 6])
                .style(menu_button);
            row![select, close].spacing(0).align_y(Alignment::Center).into()
        }))
        .spacing(2);

        let breadcrumb: Element<'_, Message> = match &buf.path {
            Some(p) => {
                let rel = p.strip_prefix(&self.root).unwrap_or(p);
                let crumbs = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("  ›  ");
                text(crumbs).size(12).color(MUTED).into()
            }
            None => text("untitled").size(12).color(MUTED).into(),
        };

        let header = container(
            column![
                scrollable(tabs).width(Fill),
                container(breadcrumb).padding([4, 4])
            ]
            .spacing(2)
            .padding([4, 8]),
        )
        .width(Fill)
        .style(tabstrip_bg);

        // Gutter is a canvas that mirrors the editor's scroll offset; size and
        // line height are pinned to the same values on both so rows line up.
        let line_count = buf.content.line_count().max(1);
        let digits = line_count.to_string().len().max(2);
        let gutter_w = digits as f32 * 8.0 + 20.0;
        let cursor_line = buf.content.cursor().position.line;
        let scroll = buf.scroll;

        let body = responsive(move |size| {
            // The scroll clamp in update() needs the real text-area height
            // (minus the editor's 8px vertical padding).
            self.editor_text_h.set(size.height - 16.0);
            let numbers = canvas(gutter::Gutter {
                line_count,
                scroll,
                current: cursor_line,
            })
            .width(gutter_w)
            .height(Fill);
            let editor = text_editor(&buf.content)
                .placeholder("start typing")
                .on_action(Message::Edit)
                // Tab inserts four spaces; the default binding drops it.
                .key_binding(|kp| match kp.key.as_ref() {
                    keyboard::Key::Named(keyboard::key::Named::Tab) => {
                        Some(text_editor::Binding::Sequence(
                            (0..4).map(|_| text_editor::Binding::Insert(' ')).collect(),
                        ))
                    }
                    _ => text_editor::Binding::from_key_press(kp),
                })
                .font(NERD_FONT)
                .size(gutter::TEXT_SIZE)
                .line_height(LineHeight::Absolute(gutter::LINE_H.into()))
                .wrapping(Wrapping::None)
                .highlight(&buf.syntax, highlighter::Theme::Base16Eighties)
                .padding([8, 12])
                .style(flat_editor)
                .height(Fill);
            row![container(numbers).padding([8, 0]).height(Fill), editor]
                .height(Fill)
                .into()
        });

        container(column![header, rule::horizontal(1), body].height(Fill))
        .width(Fill)
        .height(Fill)
        .clip(true)
        .style(editor_panel)
        .into()
    }

    fn status_bar(&self) -> Element<'_, Message> {
        let right: Element<'_, Message> = match self.active.and_then(|i| self.open.get(i)) {
            Some(buf) => {
                let cursor = buf.content.cursor();
                let (line, col) = (cursor.position.line, cursor.position.column);
                text(format!(
                    "Ln {}, Col {}   {}",
                    line + 1,
                    col + 1,
                    language_name(&buf.syntax),
                ))
                .size(12)
                .color(MUTED)
                .into()
            }
            None => text(format!("{}", self.view_mode)).size(12).color(MUTED).into(),
        };

        container(
            row![
                text(&self.status).size(12).color(MUTED),
                space::horizontal(),
                right,
            ]
            .padding([4, 12]),
        )
        .width(Fill)
        .style(canvas_bg)
        .into()
    }
}

fn tree_rows<'a>(
    nodes: &'a [tree::Node],
    depth: u16,
    selected: Option<&Path>,
    index: &ske::index::Index,
    rows: &mut Vec<Element<'a, Message>>,
) {
    for node in nodes {
        let is_selected = selected == Some(node.path.as_path());
        let (icon, icon_color, msg) = match &node.kind {
            tree::Kind::Dir { expanded, .. } => (
                if *expanded {
                    ICON_CHEVRON_DOWN
                } else {
                    ICON_CHEVRON_RIGHT
                },
                MUTED,
                Message::ToggleDir(node.path.clone()),
            ),
            tree::Kind::File => (
                if matches!(node.name.as_str(), "BUCK" | "BUILD" | "BUILD.bazel") {
                    ICON_TARGET_BUILD
                } else {
                    ICON_FILE
                },
                if matches!(node.name.as_str(), "BUCK" | "BUILD" | "BUILD.bazel") {
                    GREEN
                } else {
                    MUTED
                },
                Message::Open(node.path.clone()),
            ),
        };

        let label_color = if is_selected {
            TEXT
        } else if node.hidden {
            MUTED
        } else {
            TEXT
        };

        let mut content = row![
            Space::new().width(Length::Fixed(f32::from(depth) * 12.0)),
            text(icon).size(12).font(NERD_FONT).color(if is_selected {
                ACCENT
            } else {
                icon_color
            }),
            text(&node.name).size(13).color(if node.hidden {
                MUTED
            } else {
                label_color
            })
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        // Badge: how many targets list this file in a file-carrying attr.
        if matches!(node.kind, tree::Kind::File) {
            let n = index.targets_of(&node.path).len();
            if n > 0 {
                content = content.push(space::horizontal());
                content = content.push(text(n.to_string()).size(10).color(GREEN));
            }
        }

        rows.push(
            mouse_area(
                button(content)
                    .on_press(msg)
                    .width(Fill)
                    .padding([3, 8])
                    .style(move |_t: &Theme, status| tree_button(is_selected, status)),
            )
            .on_right_press(Message::OpenContext(CtxKind::Entry {
                path: node.path.clone(),
                is_dir: matches!(node.kind, tree::Kind::Dir { .. }),
            }))
            .into(),
        );

        if let tree::Kind::Dir {
            expanded: true,
            children: Some(children),
        } = &node.kind
        {
            tree_rows(children, depth + 1, selected, index, rows);
        }
    }
}

fn kind_icon(kind: ske::buck::Kind) -> (&'static str, Color) {
    match kind {
        ske::buck::Kind::Binary => (ICON_TARGET_BUILD, ACCENT),
        ske::buck::Kind::Library => (ICON_TARGET_LIB, GREEN),
        ske::buck::Kind::Test => (ICON_TARGET_TEST, YELLOW),
        ske::buck::Kind::Web => (ICON_FILE, YELLOW),
        ske::buck::Kind::Other => (ICON_FILE, MUTED),
    }
}

fn existing_buck_file(dir: &Path) -> Option<PathBuf> {
    ["BUCK", "BUILD", "BUILD.bazel"]
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.is_file())
}

fn load_targets(root: PathBuf) -> Task<Message> {
    Task::perform(
        async move { ske::buck::list_targets(&root) },
        Message::TargetsLoaded,
    )
}

/// Template search path: workspace-local overrides first, then the repo's own
/// templates/, then the user-wide install dir (docs/02-template-system.md).
fn load_templates(root: &Path) -> ske::template::TemplateSet {
    let mut roots = vec![root.join(".skedit/templates"), root.join("templates")];
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".config/skedit/templates"));
    }
    ske::template::TemplateSet::load(&roots)
}

// ---- styles ----

fn canvas_bg(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_CANVAS)),
        ..container::Style::default()
    }
}

fn tabstrip_bg(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_PANEL)),
        ..container::Style::default()
    }
}

/// Invisible edge/corner strips that start an interactive window resize —
/// with OS decorations off, the compositor gives us no handles of its own.
fn resize_grips() -> Vec<Element<'static, Message>> {
    use iced::alignment::{Horizontal, Vertical};
    use mouse::Interaction as I;
    use window::Direction as D;

    let grip = |dir: D, w: Length, h: Length, cursor: I, ax: Horizontal, ay: Vertical| {
        container(
            mouse_area(Space::new().width(w).height(h))
                .on_press(Message::ResizeWindow(dir))
                .interaction(cursor),
        )
        .width(Fill)
        .height(Fill)
        .align_x(ax)
        .align_y(ay)
        .into()
    };
    let edge = Length::Fixed(5.0);
    let corner = Length::Fixed(12.0);
    vec![
        grip(D::North, Fill, edge, I::ResizingVertically, Horizontal::Center, Vertical::Top),
        grip(D::South, Fill, edge, I::ResizingVertically, Horizontal::Center, Vertical::Bottom),
        grip(D::West, edge, Fill, I::ResizingHorizontally, Horizontal::Left, Vertical::Center),
        grip(D::East, edge, Fill, I::ResizingHorizontally, Horizontal::Right, Vertical::Center),
        grip(D::NorthWest, corner, corner, I::ResizingDiagonallyDown, Horizontal::Left, Vertical::Top),
        grip(D::NorthEast, corner, corner, I::ResizingDiagonallyUp, Horizontal::Right, Vertical::Top),
        grip(D::SouthWest, corner, corner, I::ResizingDiagonallyUp, Horizontal::Left, Vertical::Bottom),
        grip(D::SouthEast, corner, corner, I::ResizingDiagonallyDown, Horizontal::Right, Vertical::Bottom),
    ]
}

fn context_panel(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_PANEL)),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: border::radius(6),
        },
        shadow: iced::Shadow {
            color: Color::from_rgba8(0, 0, 0, 0.5),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 16.0,
        },
        ..container::Style::default()
    }
}

fn sidebar_panel(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_PANEL)),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: border::radius(8),
        },
        ..container::Style::default()
    }
}

fn editor_panel(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_EDITOR)),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: border::radius(8),
        },
        ..container::Style::default()
    }
}

fn flat_editor(_theme: &Theme, _status: text_editor::Status) -> text_editor::Style {
    text_editor::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        placeholder: MUTED,
        value: TEXT,
        selection: ACCENT_BG,
    }
}

fn menu_button(_theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then_some(Background::Color(BG_PANEL)),
        text_color: TEXT,
        border: Border {
            radius: border::radius(4),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

fn accent_outline_button(_theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(Background::Color(if hovered { ACCENT_BG } else { BG_PANEL })),
        text_color: ACCENT,
        border: Border {
            color: ACCENT,
            width: 1.0,
            radius: border::radius(6),
        },
        ..button::Style::default()
    }
}

fn rail_button(active: bool, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: (active || hovered).then_some(Background::Color(BG_PANEL)),
        text_color: if active { ACCENT } else { MUTED },
        border: Border {
            radius: border::radius(6),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

fn tab_button(active: bool, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(Background::Color(if active {
            BG_EDITOR
        } else if hovered {
            BG_CANVAS
        } else {
            Color::TRANSPARENT
        })),
        text_color: if active { TEXT } else { MUTED },
        border: Border {
            radius: border::radius(6),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

fn tree_button(selected: bool, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: if selected {
            Some(Background::Color(ACCENT_BG))
        } else if hovered {
            Some(Background::Color(BG_CANVAS))
        } else {
            None
        },
        text_color: TEXT,
        border: Border {
            radius: border::radius(4),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

fn close_button(_theme: &Theme, status: button::Status) -> button::Style {
    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(Background::Color(RED)),
            text_color: Color::WHITE,
            border: Border {
                radius: border::radius(4),
                ..Border::default()
            },
            ..button::Style::default()
        },
        _ => button::Style {
            text_color: MUTED,
            ..button::Style::default()
        },
    }
}
