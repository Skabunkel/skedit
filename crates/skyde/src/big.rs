//! Super-large-file editing (goal.md #7): a piece-table buffer streamed in
//! from disk, rendered on a canvas with plain line numbers — no syntax
//! highlighting, no cosmic-text buffer of the whole file.

use iced::alignment;
use iced::keyboard;
use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Text};
use iced::widget::text::LineHeight;
use iced::{Point, Rectangle, Renderer, Size, Theme};

use crate::gutter::{LINE_H, TEXT_SIZE};
use crate::piece::PieceTable;
use crate::Message;

/// Advance of one cell of the mono font at TEXT_SIZE. Only used to map
/// clicks/columns to x positions, so a small error is cosmetic.
pub const CHAR_W: f32 = 7.83;

/// One streaming read event (`Message::BigLoad`).
#[derive(Debug, Clone)]
pub enum Load {
    Chunk(String),
    Done,
    Failed(String),
}

/// Editing/navigation intents published by the canvas.
#[derive(Debug, Clone)]
pub enum Action {
    Insert(String),
    Enter,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Click { line: usize, col: usize },
    Scroll(f32),
}

pub struct BigBuffer {
    pub table: PieceTable,
    pub line: usize,
    /// Char column within the line (clamped to line length on use).
    pub col: usize,
    pub scroll: f32,
    /// First visible char column (follows the cursor horizontally).
    pub h_first: usize,
    pub loading: bool,
    pub total_bytes: u64,
    pub loaded_bytes: u64,
}

impl BigBuffer {
    pub fn new(total_bytes: u64) -> Self {
        Self {
            table: PieceTable::new(),
            line: 0,
            col: 0,
            scroll: 0.0,
            h_first: 0,
            loading: true,
            total_bytes,
            loaded_bytes: 0,
        }
    }

    /// Byte offset of the cursor, clamping the column to the line's length.
    fn cursor_offset(&self) -> usize {
        let start = self.table.line_start(self.line);
        let line = self.table.line(self.line);
        let within: usize = line
            .chars()
            .take(self.col)
            .map(char::len_utf8)
            .sum();
        start + within
    }

    fn clamp_col(&mut self) {
        let n = self.table.line(self.line).chars().count();
        if self.col > n {
            self.col = n;
        }
    }

    /// Apply one action; returns true when the buffer was modified.
    pub fn apply(&mut self, action: Action, view_h: f32) -> bool {
        let mut dirty = false;
        match action {
            Action::Insert(s) => {
                self.clamp_col();
                let chars = s.chars().count();
                let has_nl = s.contains('\n');
                self.table.insert(self.cursor_offset(), &s);
                if has_nl {
                    // Multi-line paste: land after the inserted text.
                    self.line += s.matches('\n').count();
                    self.col = s.rsplit('\n').next().unwrap_or("").chars().count();
                } else {
                    self.col += chars;
                }
                dirty = true;
            }
            Action::Enter => {
                self.clamp_col();
                // Auto-indent here too: copy the current line's leading
                // whitespace (goal.md #9).
                let line = self.table.line(self.line);
                let indent: String = line
                    .chars()
                    .take(self.col)
                    .take_while(|c| *c == ' ' || *c == '\t')
                    .collect();
                self.table
                    .insert(self.cursor_offset(), &format!("\n{indent}"));
                self.line += 1;
                self.col = indent.chars().count();
                dirty = true;
            }
            Action::Backspace => {
                self.clamp_col();
                let end = self.cursor_offset();
                if self.col > 0 {
                    let line = self.table.line(self.line);
                    let prev = line
                        .chars()
                        .nth(self.col - 1)
                        .map(char::len_utf8)
                        .unwrap_or(1);
                    self.table.delete(end - prev..end);
                    self.col -= 1;
                    dirty = true;
                } else if self.line > 0 {
                    self.line -= 1;
                    self.col = self.table.line(self.line).chars().count();
                    self.table.delete(end - 1..end);
                    dirty = true;
                }
            }
            Action::Delete => {
                self.clamp_col();
                let start = self.cursor_offset();
                if start < self.table.len_bytes() {
                    let line = self.table.line(self.line);
                    let next = line
                        .chars()
                        .nth(self.col)
                        .map(char::len_utf8)
                        // Cursor at line end: the next byte is the newline.
                        .unwrap_or(1);
                    self.table.delete(start..start + next);
                    dirty = true;
                }
            }
            Action::Left => {
                self.clamp_col();
                if self.col > 0 {
                    self.col -= 1;
                } else if self.line > 0 {
                    self.line -= 1;
                    self.col = self.table.line(self.line).chars().count();
                }
            }
            Action::Right => {
                self.clamp_col();
                if self.col < self.table.line(self.line).chars().count() {
                    self.col += 1;
                } else if self.line + 1 < self.table.line_count() {
                    self.line += 1;
                    self.col = 0;
                }
            }
            Action::Up => self.line = self.line.saturating_sub(1),
            Action::Down => {
                if self.line + 1 < self.table.line_count() {
                    self.line += 1;
                }
            }
            Action::Home => self.col = 0,
            Action::End => self.col = self.table.line(self.line).chars().count(),
            Action::PageUp => {
                let page = (view_h / LINE_H) as usize;
                self.line = self.line.saturating_sub(page.max(1));
            }
            Action::PageDown => {
                let page = (view_h / LINE_H) as usize;
                self.line = (self.line + page.max(1)).min(self.table.line_count() - 1);
            }
            Action::Click { line, col } => {
                self.line = line.min(self.table.line_count() - 1);
                self.col = col;
                self.clamp_col();
            }
            Action::Scroll(px) => {
                self.scroll += px;
                self.clamp_scroll(view_h);
                return false; // wheel scroll must not re-center on the cursor
            }
        }
        self.clamp_scroll(view_h);
        self.ensure_cursor_visible(view_h);
        dirty
    }

    pub fn clamp_scroll(&mut self, view_h: f32) {
        let max = (self.table.line_count() as f32 * LINE_H - view_h).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
    }

    pub fn ensure_cursor_visible(&mut self, view_h: f32) {
        let top = self.line as f32 * LINE_H;
        if top < self.scroll {
            self.scroll = top;
        } else if top + LINE_H > self.scroll + view_h {
            self.scroll = top + LINE_H - view_h;
        }
    }

    /// Keep the cursor's column inside the visible band of `cols` columns.
    pub fn follow_cursor_h(&mut self, cols: usize) {
        self.clamp_col();
        if self.col < self.h_first {
            self.h_first = self.col;
        } else if cols > 8 && self.col > self.h_first + cols - 8 {
            self.h_first = self.col + 8 - cols;
        }
    }
}

/// Canvas program for one frame of the big-file editor. Owns copies of just
/// the visible lines, so building it each view() is cheap.
pub struct View {
    pub lines: Vec<String>,
    pub first: usize,
    pub scroll: f32,
    pub cursor: (usize, usize),
    pub h_first: usize,
    pub gutter_w: f32,
    /// 0.0..1.0 while the file is still streaming in.
    pub loading: Option<f32>,
    pub focused_input: bool,
}

const PAD_X: f32 = 12.0;

impl canvas::Program<Message> for View {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let publish = |a: Action| Some(canvas::Action::publish(Message::Big(a)).and_capture());
        match event {
            canvas::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => {
                // Let app-level shortcuts (ctrl+s, ctrl+g, …) pass through,
                // and stay out of the way while a text_input has focus.
                if modifiers.command() || modifiers.alt() || self.focused_input {
                    return None;
                }
                use keyboard::key::Named;
                match key.as_ref() {
                    keyboard::Key::Named(Named::Enter) => publish(Action::Enter),
                    keyboard::Key::Named(Named::Backspace) => publish(Action::Backspace),
                    keyboard::Key::Named(Named::Delete) => publish(Action::Delete),
                    keyboard::Key::Named(Named::ArrowLeft) => publish(Action::Left),
                    keyboard::Key::Named(Named::ArrowRight) => publish(Action::Right),
                    keyboard::Key::Named(Named::ArrowUp) => publish(Action::Up),
                    keyboard::Key::Named(Named::ArrowDown) => publish(Action::Down),
                    keyboard::Key::Named(Named::Home) => publish(Action::Home),
                    keyboard::Key::Named(Named::End) => publish(Action::End),
                    keyboard::Key::Named(Named::PageUp) => publish(Action::PageUp),
                    keyboard::Key::Named(Named::PageDown) => publish(Action::PageDown),
                    keyboard::Key::Named(Named::Tab) => {
                        publish(Action::Insert("    ".into()))
                    }
                    _ => match text {
                        Some(t) if t.chars().all(|c| !c.is_control()) && !t.is_empty() => {
                            publish(Action::Insert(t.to_string()))
                        }
                        _ => None,
                    },
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = cursor.position_in(bounds)?;
                let line = ((p.y + self.scroll) / LINE_H).max(0.0) as usize;
                let col = self.h_first
                    + (((p.x - self.gutter_w - PAD_X) / CHAR_W).round().max(0.0) as usize);
                publish(Action::Click { line, col })
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                cursor.position_in(bounds)?;
                let px = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => -y * 3.0 * LINE_H,
                    mouse::ScrollDelta::Pixels { y, .. } => -y,
                };
                publish(Action::Scroll(px))
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let y_off = -(self.scroll - self.first as f32 * LINE_H);
        let max_cols = ((bounds.width - self.gutter_w - PAD_X) / CHAR_W) as usize + 2;

        for (i, line) in self.lines.iter().enumerate() {
            let ln = self.first + i;
            let y = i as f32 * LINE_H + y_off;
            // line number, right-aligned in the gutter
            frame.fill_text(Text {
                content: (ln + 1).to_string(),
                position: Point::new(self.gutter_w - 10.0, y),
                color: if ln == self.cursor.0 {
                    crate::TEXT
                } else {
                    crate::FAINT
                },
                size: TEXT_SIZE.into(),
                line_height: LineHeight::Absolute(LINE_H.into()),
                font: crate::NERD_FONT,
                align_x: iced::widget::text::Alignment::Right,
                align_y: alignment::Vertical::Top,
                ..Text::default()
            });
            // the line itself, horizontally windowed to the visible columns
            let visible: String = line
                .chars()
                .skip(self.h_first)
                .take(max_cols)
                .collect();
            frame.fill_text(Text {
                content: visible,
                position: Point::new(self.gutter_w + PAD_X, y),
                color: crate::TEXT,
                size: TEXT_SIZE.into(),
                line_height: LineHeight::Absolute(LINE_H.into()),
                font: crate::NERD_FONT,
                align_y: alignment::Vertical::Top,
                ..Text::default()
            });
            // caret
            if ln == self.cursor.0 && self.cursor.1 >= self.h_first {
                let col = line.chars().count().min(self.cursor.1);
                let x = self.gutter_w + PAD_X + (col - self.h_first) as f32 * CHAR_W;
                frame.fill(
                    &Path::rectangle(Point::new(x, y + 2.0), Size::new(1.5, LINE_H - 4.0)),
                    crate::ACCENT,
                );
            }
        }

        if let Some(pct) = self.loading {
            frame.fill_text(Text {
                content: format!("streaming… {:.0}%", pct * 100.0),
                position: Point::new(bounds.width - 12.0, bounds.height - 8.0),
                color: crate::YELLOW,
                size: 12.0.into(),
                font: crate::NERD_FONT,
                align_x: iced::widget::text::Alignment::Right,
                align_y: alignment::Vertical::Bottom,
                ..Text::default()
            });
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor
            .position_in(bounds)
            .is_some_and(|p| p.x > self.gutter_w)
        {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }
}
