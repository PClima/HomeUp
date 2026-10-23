//! Vector icons drawn with the egui painter (no font glyphs or image assets).

use crate::app::Tool;
use eframe::egui::{self, Color32, Painter, Pos2, Rect, Shape, Stroke};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Tool(Tool),
    Ui(crate::icons2::UiIcon),
    ZoomExtents,
    More,
    Undo,
    Redo,
    Search,
    EntityInfo,
    Components,
    Instructor,
    Library,
    Materials,
    Styles,
    Tags,
    Shadows,
    Scenes,
    Display,
    Soften,
    ModelInfo,
    Outliner,
    Environments,
}

const INK: Color32 = Color32::from_rgb(40, 52, 70);
const RED: Color32 = Color32::from_rgb(226, 30, 30);
const BLUE: Color32 = Color32::from_rgb(20, 130, 200);

pub(crate) struct Pen<'a> {
    pub(crate) p: &'a Painter,
    pub(crate) r: Rect,
}

impl Pen<'_> {
    pub(crate) fn at(&self, x: f32, y: f32) -> Pos2 {
        Pos2::new(self.r.min.x + x * self.r.width(), self.r.min.y + y * self.r.height())
    }
    pub(crate) fn s(&self, w: f32) -> f32 {
        w * self.r.width() / 24.0
    }
    pub(crate) fn line(&self, pts: &[(f32, f32)], w: f32, c: Color32) {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| self.at(*x, *y)).collect();
        self.p.add(Shape::line(v, Stroke::new(self.s(w), c)));
    }
    pub(crate) fn closed(&self, pts: &[(f32, f32)], w: f32, c: Color32) {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| self.at(*x, *y)).collect();
        self.p.add(Shape::closed_line(v, Stroke::new(self.s(w), c)));
    }
    pub(crate) fn fill(&self, pts: &[(f32, f32)], c: Color32) {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| self.at(*x, *y)).collect();
        self.p.add(Shape::convex_polygon(v, c, Stroke::NONE));
    }
    pub(crate) fn circle(&self, x: f32, y: f32, r: f32, w: f32, c: Color32) {
        self.p.circle_stroke(self.at(x, y), r * self.r.width(), Stroke::new(self.s(w), c));
    }
    pub(crate) fn dot(&self, x: f32, y: f32, r: f32, c: Color32) {
        self.p.circle_filled(self.at(x, y), r * self.r.width(), c);
    }
    /// Arc from angle a0 to a1 (degrees, 0 = right, counter-clockwise, screen y down).
    pub(crate) fn arc(&self, x: f32, y: f32, r: f32, a0: f32, a1: f32, w: f32, c: Color32) -> (Pos2, Pos2) {
        let n = 24;
        let pts: Vec<(f32, f32)> = (0..=n)
            .map(|k| {
                let a = (a0 + (a1 - a0) * k as f32 / n as f32).to_radians();
                (x + r * a.cos(), y - r * a.sin())
            })
            .collect();
        self.line(&pts, w, c);
        (self.at(pts[0].0, pts[0].1), self.at(pts[n].0, pts[n].1))
    }
    pub(crate) fn arrow_head(&self, tip: (f32, f32), dir: (f32, f32), size: f32, c: Color32) {
        let l = (dir.0 * dir.0 + dir.1 * dir.1).sqrt().max(1e-6);
        let (dx, dy) = (dir.0 / l, dir.1 / l);
        let (px, py) = (-dy, dx);
        self.fill(
            &[
                tip,
                (tip.0 - dx * size + px * size * 0.6, tip.1 - dy * size + py * size * 0.6),
                (tip.0 - dx * size - px * size * 0.6, tip.1 - dy * size - py * size * 0.6),
            ],
            c,
        );
    }
}

pub fn paint(p: &Painter, r: Rect, icon: Icon) {
    let k = Pen { p, r };
    match icon {
        Icon::Tool(t) => paint_tool(&k, t),
        Icon::Ui(u) => crate::icons2::paint_ui(&k, u),
        Icon::ZoomExtents => {
            k.circle(0.45, 0.42, 0.18, 1.8, INK);
            k.line(&[(0.58, 0.56), (0.78, 0.76)], 2.6, INK);
            for (x, y, dx, dy) in [(0.1, 0.1, -1.0, -1.0), (0.9, 0.1, 1.0, -1.0), (0.1, 0.9, -1.0, 1.0), (0.9, 0.9, 1.0, 1.0)] {
                k.arrow_head((x, y), (dx, dy), 0.12, RED);
            }
        }
        Icon::More => {
            for x in [0.28, 0.5, 0.72] {
                k.dot(x, 0.5, 0.06, INK);
            }
        }
        Icon::Undo | Icon::Redo => {
            let undo = icon == Icon::Undo;
            let (s, _) = k.arc(0.5, 0.55, 0.26, if undo { 160.0 } else { 20.0 }, if undo { 20.0 } else { 160.0 }, 1.8, INK);
            let _ = s;
            if undo {
                k.arrow_head((0.2, 0.52), (-0.3, 0.6), 0.16, INK);
            } else {
                k.arrow_head((0.8, 0.52), (0.3, 0.6), 0.16, INK);
            }
        }
        Icon::Search => {
            k.circle(0.42, 0.42, 0.22, 2.0, INK);
            k.line(&[(0.58, 0.58), (0.85, 0.85)], 3.0, INK);
        }
        Icon::EntityInfo => {
            k.closed(&[(0.5, 0.15), (0.82, 0.32), (0.82, 0.68), (0.5, 0.85), (0.18, 0.68), (0.18, 0.32)], 1.5, INK);
            k.closed(&[(0.5, 0.32), (0.66, 0.41), (0.66, 0.6), (0.5, 0.69), (0.34, 0.6), (0.34, 0.41)], 1.3, INK);
        }
        Icon::Components => {
            for (x, y) in [(0.33, 0.62), (0.67, 0.62), (0.5, 0.33)] {
                k.closed(&[(x - 0.15, y - 0.08), (x, y - 0.16), (x + 0.15, y - 0.08), (x + 0.15, y + 0.12), (x, y + 0.2), (x - 0.15, y + 0.12)], 1.3, INK);
            }
        }
        Icon::Library => {
            k.circle(0.5, 0.5, 0.36, 1.6, INK);
            k.closed(&[(0.5, 0.26), (0.7, 0.38), (0.7, 0.62), (0.5, 0.74), (0.3, 0.62), (0.3, 0.38)], 1.4, INK);
            k.line(&[(0.3, 0.38), (0.5, 0.5), (0.7, 0.38)], 1.2, INK);
            k.line(&[(0.5, 0.5), (0.5, 0.74)], 1.2, INK);
            k.arrow_head((0.5, 0.97), (0.0, 1.0), 0.14, BLUE);
        }
        Icon::Instructor => {
            k.fill(&[(0.5, 0.2), (0.92, 0.38), (0.5, 0.56), (0.08, 0.38)], INK);
            k.line(&[(0.28, 0.48), (0.28, 0.68), (0.5, 0.78), (0.72, 0.68), (0.72, 0.48)], 1.6, INK);
            k.line(&[(0.86, 0.4), (0.86, 0.66)], 1.4, RED);
        }
        Icon::Outliner => {
            k.line(&[(0.2, 0.22), (0.2, 0.78)], 1.4, INK);
            for (y, x) in [(0.22, 0.3), (0.5, 0.38), (0.78, 0.38)] {
                k.line(&[(0.2, y), (x, y)], 1.4, INK);
                k.closed(&[(x, y - 0.09), (x + 0.42, y - 0.09), (x + 0.42, y + 0.09), (x, y + 0.09)], 1.4, INK);
            }
        }
        Icon::Environments => {
            k.circle(0.5, 0.5, 0.36, 1.5, INK);
            k.arc(0.5, 0.5, 0.36, 200.0, 340.0, 1.2, INK);
            k.line(&[(0.14, 0.5), (0.86, 0.5)], 1.2, INK);
            k.dot(0.62, 0.32, 0.07, Color32::from_rgb(240, 170, 30));
        }
        Icon::Materials => {
            k.closed(&[(0.5, 0.15), (0.84, 0.32), (0.84, 0.7), (0.5, 0.87), (0.16, 0.7), (0.16, 0.32)], 1.5, INK);
            k.fill(&[(0.5, 0.5), (0.84, 0.32), (0.84, 0.7), (0.5, 0.87)], Color32::from_rgb(80, 92, 110));
            k.line(&[(0.16, 0.32), (0.5, 0.5), (0.5, 0.87)], 1.3, INK);
        }
        Icon::Styles => {
            k.closed(&[(0.15, 0.35), (0.45, 0.25), (0.6, 0.32), (0.6, 0.78), (0.3, 0.88), (0.15, 0.8)], 1.4, INK);
            k.circle(0.7, 0.42, 0.12, 1.4, INK);
            k.line(&[(0.58, 0.42), (0.58, 0.8)], 1.4, INK);
            k.line(&[(0.82, 0.42), (0.82, 0.8)], 1.4, INK);
        }
        Icon::Tags => {
            k.closed(&[(0.18, 0.5), (0.45, 0.22), (0.8, 0.22), (0.8, 0.78), (0.45, 0.78)], 1.6, INK);
            k.dot(0.66, 0.5, 0.06, INK);
        }
        Icon::Shadows => {
            k.circle(0.5, 0.42, 0.16, 1.6, INK);
            for i in 0..8 {
                let a = (i as f32 * 45.0f32).to_radians();
                k.line(&[(0.5 + 0.24 * a.cos(), 0.42 - 0.24 * a.sin()), (0.5 + 0.32 * a.cos(), 0.42 - 0.32 * a.sin())], 1.4, INK);
            }
            k.fill(&[(0.25, 0.85), (0.75, 0.85), (0.6, 0.74), (0.4, 0.74)], Color32::from_gray(120));
        }
        Icon::Scenes => {
            k.closed(&[(0.15, 0.4), (0.85, 0.4), (0.85, 0.85), (0.15, 0.85)], 1.6, INK);
            k.closed(&[(0.15, 0.4), (0.8, 0.2), (0.86, 0.34)], 1.4, INK);
            k.fill(&[(0.42, 0.52), (0.62, 0.62), (0.42, 0.72)], INK);
        }
        Icon::Display => {
            k.circle(0.3, 0.55, 0.14, 1.6, INK);
            k.circle(0.7, 0.55, 0.14, 1.6, INK);
            k.line(&[(0.44, 0.52), (0.56, 0.52)], 1.6, INK);
        }
        Icon::Soften => {
            k.circle(0.5, 0.5, 0.33, 1.5, INK);
            k.arc(0.5, 0.5, 0.33, 90.0, 270.0, 1.0, INK);
            k.line(&[(0.5, 0.17), (0.5, 0.83)], 1.0, INK);
            k.line(&[(0.17, 0.5), (0.83, 0.5)], 1.0, INK);
            k.line(&[(0.22, 0.33), (0.78, 0.33)], 0.8, INK);
            k.line(&[(0.22, 0.67), (0.78, 0.67)], 0.8, INK);
        }
        Icon::ModelInfo => {
            k.circle(0.5, 0.5, 0.34, 1.6, INK);
            k.dot(0.5, 0.33, 0.05, INK);
            k.line(&[(0.5, 0.45), (0.5, 0.72)], 2.2, INK);
        }
    }
}

fn paint_tool(k: &Pen, t: Tool) {
    match t {
        Tool::Select => k.fill(&[(0.32, 0.12), (0.32, 0.8), (0.47, 0.66), (0.72, 0.62)], INK),
        Tool::Eraser => {
            k.fill(&[(0.18, 0.62), (0.5, 0.3), (0.66, 0.46), (0.34, 0.78)], Color32::from_rgb(245, 160, 175));
            k.fill(&[(0.5, 0.3), (0.65, 0.15), (0.82, 0.31), (0.66, 0.46)], Color32::from_rgb(70, 110, 190));
            k.closed(&[(0.18, 0.62), (0.65, 0.15), (0.82, 0.31), (0.34, 0.78)], 1.3, INK);
            k.line(&[(0.3, 0.86), (0.85, 0.86)], 1.2, INK);
        }
        Tool::Line => {
            k.line(&[(0.25, 0.78), (0.78, 0.25)], 4.0, RED);
            k.fill(&[(0.18, 0.85), (0.22, 0.7), (0.33, 0.81)], INK);
            k.line(&[(0.7, 0.22), (0.82, 0.34)], 4.0, INK);
        }
        Tool::Rect => {
            k.closed(&[(0.18, 0.28), (0.72, 0.28), (0.72, 0.82), (0.18, 0.82)], 1.8, INK);
            k.line(&[(0.18, 0.82), (0.72, 0.28)], 1.0, BLUE);
            k.dot(0.76, 0.22, 0.08, RED);
        }
        Tool::Circle => {
            k.circle(0.48, 0.55, 0.3, 1.8, INK);
            k.dot(0.48, 0.55, 0.05, INK);
            k.line(&[(0.48, 0.55), (0.72, 0.36)], 1.0, BLUE);
            k.dot(0.74, 0.34, 0.08, RED);
        }
        Tool::Polygon => {
            let pts: Vec<(f32, f32)> = (0..6)
                .map(|i| {
                    let a = (30.0 + i as f32 * 60.0f32).to_radians();
                    (0.5 + 0.32 * a.cos(), 0.55 - 0.32 * a.sin())
                })
                .collect();
            k.closed(&pts, 1.8, INK);
            k.dot(0.78, 0.39, 0.08, RED);
        }
        Tool::Arc => {
            k.arc(0.5, 0.7, 0.34, 0.0, 180.0, 1.8, INK);
            k.dot(0.16, 0.7, 0.07, INK);
            k.dot(0.84, 0.7, 0.07, INK);
            k.dot(0.5, 0.36, 0.08, RED);
        }
        Tool::ArcCenter => {
            k.arc(0.42, 0.62, 0.34, 0.0, 120.0, 1.8, INK);
            k.line(&[(0.42, 0.62), (0.76, 0.62)], 1.0, BLUE);
            k.dot(0.42, 0.62, 0.06, RED);
            k.dot(0.76, 0.62, 0.06, INK);
        }
        Tool::Arc3 => {
            k.arc(0.5, 0.72, 0.36, 10.0, 170.0, 1.8, INK);
            k.dot(0.15, 0.66, 0.06, INK);
            k.dot(0.5, 0.36, 0.07, RED);
            k.dot(0.85, 0.66, 0.06, INK);
        }
        Tool::Pie => {
            k.fill(&[(0.35, 0.7), (0.85, 0.7), (0.8, 0.45), (0.65, 0.27), (0.5, 0.2)], Color32::from_rgb(80, 150, 210));
            k.arc(0.35, 0.7, 0.5, 0.0, 75.0, 1.6, INK);
            k.line(&[(0.48, 0.22), (0.35, 0.7), (0.85, 0.7)], 1.6, INK);
            k.dot(0.35, 0.7, 0.06, RED);
        }
        Tool::Freehand => {
            k.line(&[(0.12, 0.7), (0.25, 0.45), (0.4, 0.62), (0.55, 0.3), (0.7, 0.55), (0.88, 0.25)], 2.2, BLUE);
            k.dot(0.88, 0.25, 0.07, RED);
        }
        Tool::RotRect => {
            k.closed(&[(0.2, 0.5), (0.5, 0.18), (0.82, 0.5), (0.5, 0.82)], 1.8, INK);
            k.dot(0.5, 0.18, 0.07, RED);
            k.arc(0.2, 0.5, 0.22, 0.0, 45.0, 1.0, BLUE);
        }
        Tool::Lasso => {
            k.line(&[(0.5, 0.18), (0.78, 0.25), (0.86, 0.45), (0.7, 0.62), (0.42, 0.64), (0.2, 0.5), (0.24, 0.3), (0.5, 0.18)], 1.6, INK);
            k.line(&[(0.42, 0.64), (0.36, 0.78), (0.44, 0.9)], 1.6, INK);
            k.fill(&[(0.58, 0.48), (0.58, 0.86), (0.66, 0.76), (0.76, 0.74)], INK);
        }
        Tool::Sample => {
            k.line(&[(0.25, 0.75), (0.6, 0.4)], 3.0, INK);
            k.fill(&[(0.55, 0.3), (0.7, 0.15), (0.85, 0.3), (0.7, 0.45)], INK);
            k.dot(0.22, 0.8, 0.07, RED);
        }
        Tool::Dimension => {
            k.line(&[(0.12, 0.62), (0.88, 0.62)], 1.6, INK);
            k.line(&[(0.12, 0.45), (0.12, 0.8)], 1.2, INK);
            k.line(&[(0.88, 0.45), (0.88, 0.8)], 1.2, INK);
            k.arrow_head((0.14, 0.62), (-1.0, 0.0), 0.12, INK);
            k.arrow_head((0.86, 0.62), (1.0, 0.0), 0.12, INK);
            k.line(&[(0.38, 0.3), (0.62, 0.3)], 2.0, BLUE);
        }
        Tool::Text => {
            k.closed(&[(0.4, 0.15), (0.9, 0.15), (0.9, 0.5), (0.4, 0.5)], 1.4, INK);
            k.line(&[(0.5, 0.26), (0.8, 0.26)], 1.2, INK);
            k.line(&[(0.5, 0.38), (0.72, 0.38)], 1.2, INK);
            k.line(&[(0.4, 0.5), (0.15, 0.85)], 1.4, INK);
            k.dot(0.15, 0.85, 0.06, RED);
        }
        Tool::Text3d => {
            k.fill(&[(0.2, 0.2), (0.8, 0.2), (0.8, 0.36), (0.2, 0.36)], Color32::from_rgb(80, 150, 210));
            k.fill(&[(0.42, 0.36), (0.58, 0.36), (0.58, 0.86), (0.42, 0.86)], Color32::from_rgb(80, 150, 210));
            k.line(&[(0.8, 0.2), (0.9, 0.12)], 1.2, INK);
            k.line(&[(0.58, 0.86), (0.68, 0.78), (0.68, 0.3)], 1.2, INK);
        }
        Tool::Protractor => {
            k.arc(0.5, 0.72, 0.36, 0.0, 180.0, 1.6, INK);
            k.line(&[(0.14, 0.72), (0.86, 0.72)], 1.6, INK);
            k.line(&[(0.5, 0.72), (0.78, 0.4)], 1.4, RED);
            for a in [30.0f32, 60.0, 90.0, 120.0, 150.0] {
                let r = a.to_radians();
                k.line(&[(0.5 + 0.3 * r.cos(), 0.72 - 0.3 * r.sin()), (0.5 + 0.36 * r.cos(), 0.72 - 0.36 * r.sin())], 1.0, INK);
            }
        }
        Tool::Axes => {
            k.line(&[(0.4, 0.6), (0.9, 0.6)], 2.0, RED);
            k.line(&[(0.4, 0.6), (0.4, 0.1)], 2.0, Color32::from_rgb(0, 0, 230));
            k.line(&[(0.4, 0.6), (0.12, 0.88)], 2.0, Color32::from_rgb(0, 160, 0));
            k.dot(0.4, 0.6, 0.06, Color32::from_rgb(230, 180, 0));
        }
        Tool::ZoomWindow => {
            k.circle(0.45, 0.45, 0.22, 1.8, INK);
            k.line(&[(0.61, 0.61), (0.86, 0.86)], 2.8, INK);
            k.closed(&[(0.32, 0.32), (0.58, 0.32), (0.58, 0.58), (0.32, 0.58)], 1.0, RED);
        }
        Tool::PositionCamera => {
            k.dot(0.5, 0.22, 0.09, INK);
            k.line(&[(0.5, 0.32), (0.5, 0.62)], 2.2, INK);
            k.line(&[(0.5, 0.62), (0.36, 0.88)], 2.0, INK);
            k.line(&[(0.5, 0.62), (0.64, 0.88)], 2.0, INK);
            k.line(&[(0.32, 0.42), (0.68, 0.42)], 2.0, INK);
            k.dot(0.8, 0.2, 0.07, RED);
        }
        Tool::LookAround => {
            k.arc(0.5, 0.62, 0.34, 20.0, 160.0, 1.6, INK);
            k.arc(0.5, 0.38, 0.34, 200.0, 340.0, 1.6, INK);
            k.dot(0.5, 0.5, 0.11, BLUE);
            k.dot(0.5, 0.5, 0.04, INK);
        }
        Tool::Walk => {
            k.dot(0.55, 0.14, 0.08, INK);
            k.line(&[(0.52, 0.24), (0.46, 0.55), (0.62, 0.86)], 2.2, INK);
            k.line(&[(0.46, 0.55), (0.3, 0.84)], 2.0, INK);
            k.line(&[(0.5, 0.32), (0.7, 0.48)], 1.8, INK);
            k.line(&[(0.5, 0.32), (0.32, 0.46)], 1.8, INK);
        }
        Tool::Tag => {
            k.closed(&[(0.18, 0.5), (0.45, 0.22), (0.8, 0.22), (0.8, 0.78), (0.45, 0.78)], 1.6, INK);
            k.dot(0.66, 0.5, 0.06, RED);
        }
        Tool::Section => {
            k.closed(&[(0.15, 0.3), (0.65, 0.15), (0.85, 0.7), (0.35, 0.85)], 1.6, Color32::from_rgb(230, 120, 0));
            k.line(&[(0.5, 0.5), (0.85, 0.35)], 1.6, Color32::from_rgb(230, 120, 0));
            k.arrow_head((0.9, 0.33), (1.0, -0.4), 0.14, Color32::from_rgb(230, 120, 0));
        }
        Tool::FollowMe => {
            k.line(&[(0.15, 0.85), (0.15, 0.45), (0.45, 0.2), (0.88, 0.2)], 1.4, RED);
            k.fill(&[(0.08, 0.78), (0.3, 0.78), (0.3, 0.95), (0.08, 0.95)], Color32::from_rgb(80, 150, 210));
            k.arrow_head((0.92, 0.2), (1.0, 0.0), 0.14, RED);
        }
        Tool::Flip => {
            k.line(&[(0.5, 0.1), (0.5, 0.9)], 1.2, BLUE);
            k.fill(&[(0.42, 0.3), (0.42, 0.7), (0.12, 0.7)], Color32::from_rgb(80, 150, 210));
            k.closed(&[(0.58, 0.3), (0.58, 0.7), (0.88, 0.7)], 1.4, INK);
            k.arrow_head((0.82, 0.22), (1.0, 0.0), 0.12, RED);
        }
        Tool::OuterShell | Tool::Union | Tool::Subtract | Tool::SolidIntersect | Tool::Trim | Tool::Split => {
            let fill = Color32::from_rgb(80, 150, 210);
            let (a, b) = ([(0.12, 0.3), (0.6, 0.3), (0.6, 0.78), (0.12, 0.78)], [(0.4, 0.15), (0.88, 0.15), (0.88, 0.62), (0.4, 0.62)]);
            match t {
                Tool::OuterShell | Tool::Union => {
                    k.fill(&a, fill);
                    k.fill(&b, fill);
                }
                Tool::Subtract => k.fill(&b, fill),
                Tool::SolidIntersect => k.fill(&[(0.4, 0.3), (0.6, 0.3), (0.6, 0.62), (0.4, 0.62)], fill),
                Tool::Trim => k.fill(&a, fill),
                _ => {
                    k.fill(&[(0.4, 0.3), (0.6, 0.3), (0.6, 0.62), (0.4, 0.62)], RED);
                }
            }
            if t != Tool::OuterShell {
                k.closed(&a, 1.3, INK);
                k.closed(&b, 1.3, INK);
            } else {
                k.line(&[(0.12, 0.3), (0.4, 0.3), (0.4, 0.15), (0.88, 0.15), (0.88, 0.62), (0.6, 0.62), (0.6, 0.78), (0.12, 0.78), (0.12, 0.3)], 1.6, INK);
            }
        }
        Tool::PushPull => {
            k.fill(&[(0.12, 0.72), (0.5, 0.58), (0.88, 0.72), (0.5, 0.88)], Color32::from_rgb(80, 150, 210));
            k.closed(&[(0.12, 0.72), (0.5, 0.58), (0.88, 0.72), (0.5, 0.88)], 1.3, INK);
            k.line(&[(0.5, 0.66), (0.5, 0.25)], 3.0, RED);
            k.arrow_head((0.5, 0.08), (0.0, -1.0), 0.2, RED);
        }
        Tool::Offset => {
            k.closed(&[(0.14, 0.18), (0.82, 0.18), (0.82, 0.86), (0.14, 0.86)], 1.6, INK);
            k.closed(&[(0.32, 0.36), (0.64, 0.36), (0.64, 0.68), (0.32, 0.68)], 1.6, RED);
        }
        Tool::Move => {
            k.line(&[(0.5, 0.22), (0.5, 0.78)], 2.4, RED);
            k.line(&[(0.22, 0.5), (0.78, 0.5)], 2.4, RED);
            k.arrow_head((0.5, 0.06), (0.0, -1.0), 0.17, RED);
            k.arrow_head((0.5, 0.94), (0.0, 1.0), 0.17, RED);
            k.arrow_head((0.06, 0.5), (-1.0, 0.0), 0.17, RED);
            k.arrow_head((0.94, 0.5), (1.0, 0.0), 0.17, RED);
        }
        Tool::Rotate => {
            k.arc(0.5, 0.52, 0.3, 120.0, 400.0, 2.4, RED);
            k.arrow_head((0.24, 0.3), (-0.6, 0.5), 0.18, RED);
        }
        Tool::Scale => {
            k.closed(&[(0.14, 0.3), (0.7, 0.3), (0.7, 0.86), (0.14, 0.86)], 1.6, INK);
            k.fill(&[(0.14, 0.58), (0.42, 0.58), (0.42, 0.86), (0.14, 0.86)], Color32::from_rgb(80, 150, 210));
            k.line(&[(0.42, 0.58), (0.78, 0.22)], 2.0, RED);
            k.arrow_head((0.88, 0.12), (1.0, -1.0), 0.18, RED);
        }
        Tool::Tape => {
            k.circle(0.36, 0.6, 0.22, 2.0, INK);
            k.dot(0.36, 0.6, 0.06, INK);
            k.line(&[(0.5, 0.78), (0.92, 0.78)], 2.4, Color32::from_rgb(230, 170, 30));
            for x in [0.62, 0.72, 0.82] {
                k.line(&[(x, 0.72), (x, 0.78)], 1.0, INK);
            }
        }
        Tool::Paint => {
            k.fill(&[(0.2, 0.42), (0.5, 0.15), (0.8, 0.45), (0.5, 0.75)], Color32::from_rgb(90, 150, 220));
            k.closed(&[(0.2, 0.42), (0.5, 0.15), (0.8, 0.45), (0.5, 0.75)], 1.4, INK);
            k.dot(0.82, 0.72, 0.09, Color32::from_rgb(90, 150, 220));
        }
        Tool::Orbit => {
            k.arc(0.5, 0.5, 0.34, 200.0, 340.0, 2.4, RED);
            k.arc(0.5, 0.5, 0.34, 20.0, 160.0, 2.4, RED);
            k.arrow_head((0.84, 0.62), (0.3, 1.0), 0.17, RED);
            k.arrow_head((0.16, 0.38), (-0.3, -1.0), 0.17, RED);
        }
        Tool::Pan => {
            k.closed(&[(0.3, 0.5), (0.3, 0.24), (0.4, 0.24), (0.4, 0.46), (0.4, 0.16), (0.5, 0.16), (0.5, 0.44), (0.5, 0.2), (0.6, 0.2), (0.6, 0.46), (0.6, 0.3), (0.7, 0.3), (0.7, 0.66), (0.58, 0.86), (0.38, 0.86), (0.2, 0.62), (0.26, 0.56)], 1.4, INK);
        }
        Tool::Zoom => {
            k.circle(0.42, 0.42, 0.24, 2.0, INK);
            k.line(&[(0.6, 0.6), (0.86, 0.86)], 3.2, INK);
        }
        Tool::Place(_) => {
            k.closed(&[(0.5, 0.15), (0.84, 0.32), (0.84, 0.7), (0.5, 0.87), (0.16, 0.7), (0.16, 0.32)], 1.5, INK);
            k.line(&[(0.16, 0.32), (0.5, 0.5), (0.84, 0.32)], 1.3, INK);
            k.line(&[(0.5, 0.5), (0.5, 0.87)], 1.3, INK);
        }
    }
}

/// An icon button; returns the response. `active` paints the selected state.
pub fn button(ui: &mut egui::Ui, icon: Icon, active: bool, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let bg = if active {
        Color32::from_rgb(205, 228, 248)
    } else if resp.hovered() {
        Color32::from_rgb(228, 234, 242)
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 4.0, bg);
    if active {
        ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(3.0, size)), 1.0, Color32::from_rgb(20, 130, 200));
    }
    paint(ui.painter(), rect.shrink(size * 0.17), icon);
    resp
}
