//! Line-number gutter drawn on a canvas so it can follow the editor's pixel
//! scroll offset — a plain text column can't track cosmic-text's fractional
//! clamp at the bottom of the buffer.

use iced::alignment;
use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry};
use iced::widget::text::{Alignment as TextAlignment, LineHeight};
use iced::{Point, Rectangle, Renderer, Theme};

use crate::Message;

/// Shared metrics: the editor uses the same size/line height so rows line up.
pub const TEXT_SIZE: f32 = 13.0;
pub const LINE_H: f32 = 20.0;

pub struct Gutter {
    pub line_count: usize,
    /// Editor scroll offset in pixels (mirrored in `Buffer::scroll`).
    pub scroll: f32,
    /// Cursor line, drawn brighter.
    pub current: usize,
}

impl canvas::Program<Message> for Gutter {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let first = (self.scroll / LINE_H) as usize;
        let visible = (bounds.height / LINE_H).ceil() as usize + 1;
        for i in first..(first + visible).min(self.line_count) {
            frame.fill_text(canvas::Text {
                content: (i + 1).to_string(),
                position: Point::new(bounds.width - 10.0, i as f32 * LINE_H - self.scroll),
                color: if i == self.current {
                    crate::TEXT
                } else {
                    crate::MUTED
                },
                size: TEXT_SIZE.into(),
                line_height: LineHeight::Absolute(LINE_H.into()),
                font: crate::NERD_FONT,
                align_x: TextAlignment::Right,
                align_y: alignment::Vertical::Top,
                ..canvas::Text::default()
            });
        }
        vec![frame.into_geometry()]
    }
}
