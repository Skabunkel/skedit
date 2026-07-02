//! Build-graph canvas (docs/00-plan.md M4): targets as nodes, deps as edges,
//! layered left→right like the project-view mockup.

use iced::alignment;
use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

use crate::Message;

pub const NODE_W: f32 = 170.0;
pub const NODE_H: f32 = 52.0;
const GAP_X: f32 = 70.0;
const GAP_Y: f32 = 24.0;
const MARGIN: f32 = 24.0;

pub struct Node {
    pub name: String,
    pub sub: String,
    pub color: Color,
    pub external: bool,
    /// Index into the app's target list, if this node is a real target.
    pub target_idx: Option<usize>,
    /// Edges: this node depends on these node indices.
    pub deps: Vec<usize>,
}

/// Longest-path layering: nodes nobody depends on sit in column 0, each dep
/// one column to the right of its deepest dependent.
pub fn layout(nodes: &[Node]) -> Vec<Point> {
    let n = nodes.len();
    let mut rank = vec![0usize; n];
    // Relax at most n times; cycles just stop deepening.
    for _ in 0..n {
        let mut changed = false;
        for a in 0..n {
            for &d in &nodes[a].deps {
                if d != a && rank[d] < rank[a] + 1 {
                    rank[d] = rank[a] + 1;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut row = std::collections::BTreeMap::<usize, usize>::new();
    (0..n)
        .map(|i| {
            let r = row.entry(rank[i]).or_insert(0);
            let y = MARGIN + (*r as f32) * (NODE_H + GAP_Y);
            *r += 1;
            Point::new(MARGIN + (rank[i] as f32) * (NODE_W + GAP_X), y)
        })
        .collect()
}

pub struct Program {
    pub nodes: Vec<Node>,
    pub positions: Vec<Point>,
    pub selected: Option<usize>,
}

impl Program {
    fn node_rect(&self, i: usize) -> Rectangle {
        Rectangle::new(self.positions[i], Size::new(NODE_W, NODE_H))
    }
}

impl canvas::Program<Message> for Program {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if let canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            let p = cursor.position_in(bounds)?;
            for i in 0..self.nodes.len() {
                if self.node_rect(i).contains(p) {
                    let ti = self.nodes[i].target_idx?;
                    return Some(canvas::Action::publish(Message::SelectTarget(ti)));
                }
            }
        }
        None
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

        // edges under nodes
        for (a, node) in self.nodes.iter().enumerate() {
            for &d in &node.deps {
                let from = Point::new(
                    self.positions[a].x + NODE_W,
                    self.positions[a].y + NODE_H / 2.0,
                );
                let to = Point::new(self.positions[d].x, self.positions[d].y + NODE_H / 2.0);
                let mid = (from.x + to.x) / 2.0;
                let path = Path::new(|b| {
                    b.move_to(from);
                    b.bezier_curve_to(
                        Point::new(mid, from.y),
                        Point::new(mid, to.y),
                        Point::new(to.x - 6.0, to.y),
                    );
                });
                frame.stroke(
                    &path,
                    Stroke::default().with_color(crate::BORDER).with_width(1.5),
                );
                // arrowhead
                let head = Path::new(|b| {
                    b.move_to(to);
                    b.line_to(Point::new(to.x - 8.0, to.y - 4.0));
                    b.line_to(Point::new(to.x - 8.0, to.y + 4.0));
                    b.close();
                });
                frame.fill(&head, crate::BORDER);
            }
        }

        for (i, node) in self.nodes.iter().enumerate() {
            let rect = self.node_rect(i);
            let body = Path::rounded_rectangle(rect.position(), rect.size(), 8.0.into());
            frame.fill(&body, crate::BG_PANEL);
            let selected = self.selected == Some(i) && node.target_idx.is_some();
            frame.stroke(
                &body,
                Stroke::default()
                    .with_color(if selected { crate::ACCENT } else { crate::BORDER })
                    .with_width(if selected { 1.5 } else { 1.0 }),
            );
            // kind dot
            let dot = Path::circle(
                Point::new(rect.x + NODE_W - 14.0, rect.y + 14.0),
                3.0,
            );
            frame.fill(&dot, node.color);

            frame.fill_text(Text {
                content: node.name.clone(),
                position: Point::new(rect.x + 12.0, rect.y + 10.0),
                color: if node.external {
                    crate::MUTED
                } else {
                    crate::TEXT
                },
                size: 13.0.into(),
                align_y: alignment::Vertical::Top,
                ..Text::default()
            });
            frame.fill_text(Text {
                content: node.sub.clone(),
                position: Point::new(rect.x + 12.0, rect.y + 28.0),
                color: crate::MUTED,
                size: 11.0.into(),
                align_y: alignment::Vertical::Top,
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
        if let Some(p) = cursor.position_in(bounds) {
            for i in 0..self.nodes.len() {
                if self.node_rect(i).contains(p) && self.nodes[i].target_idx.is_some() {
                    return mouse::Interaction::Pointer;
                }
            }
        }
        mouse::Interaction::default()
    }
}
