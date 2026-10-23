//! Vector icons for the desktop toolbars (Standard, Styles, Views, Section,
//! Shadows) and the status bar.

use crate::icons::Pen;
use eframe::egui::Color32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UiIcon {
    New,
    Open,
    Save,
    Cut,
    Copy,
    Paste,
    Delete,
    Print,
    MakeComponent,
    XRay,
    Edges,
    Ao,
    Wireframe,
    HiddenLine,
    Mono,
    Textured,
    Pbr,
    BackEdges,
    ViewIso,
    ViewTop,
    ViewFront,
    ViewRight,
    ViewLeft,
    ViewBack,
    ViewBottom,
    SectionPlanes,
    SectionCuts,
    SectionFill,
    PrevView,
    Pin,
    Help,
    Photo,
    SbContours,
    SbScratch,
    SbSmoove,
    SbStamp,
    SbDrape,
    SbDetail,
    SbFlip,
}

/// Small terrain mesh used by the Sandbox icons.
fn terrain(k: &Pen, lift: f32) {
    let g = Color32::from_rgb(110, 160, 90);
    let pts = |i: usize, j: usize| {
        let x = 0.12 + i as f32 * 0.25 + j as f32 * 0.06;
        let y = 0.78 - j as f32 * 0.18 - if i == 1 && j == 1 { lift } else { 0.0 };
        (x, y)
    };
    for j in 0..3 {
        k.line(&[pts(0, j), pts(1, j), pts(2, j), pts(3, j)], 1.2, g);
    }
    for i in 0..4 {
        k.line(&[pts(i, 0), pts(i, 1), pts(i, 2)], 1.2, g);
    }
}

const INK: Color32 = Color32::from_rgb(40, 52, 70);
const RED: Color32 = Color32::from_rgb(226, 30, 30);
const BLUE: Color32 = Color32::from_rgb(20, 130, 200);
const FACE: Color32 = Color32::from_rgb(170, 196, 226);
const FACE_D: Color32 = Color32::from_rgb(120, 150, 190);
const PAPER: Color32 = Color32::from_rgb(250, 250, 250);

const TOP: [(f32, f32); 4] = [(0.5, 0.12), (0.86, 0.3), (0.5, 0.48), (0.14, 0.3)];
const LEFT: [(f32, f32); 4] = [(0.14, 0.3), (0.5, 0.48), (0.5, 0.9), (0.14, 0.72)];
const RIGHT: [(f32, f32); 4] = [(0.5, 0.48), (0.86, 0.3), (0.86, 0.72), (0.5, 0.9)];

fn cube_outline(k: &Pen, w: f32, c: Color32) {
    k.closed(&[(0.5, 0.12), (0.86, 0.3), (0.86, 0.72), (0.5, 0.9), (0.14, 0.72), (0.14, 0.3)], w, c);
    k.line(&[(0.14, 0.3), (0.5, 0.48), (0.86, 0.3)], w, c);
    k.line(&[(0.5, 0.48), (0.5, 0.9)], w, c);
}

fn cube_faces(k: &Pen, top: Color32, left: Color32, right: Color32) {
    k.fill(&TOP, top);
    k.fill(&LEFT, left);
    k.fill(&RIGHT, right);
}

fn hidden_edges(k: &Pen, c: Color32) {
    for seg in [[(0.14, 0.72), (0.5, 0.54)], [(0.5, 0.54), (0.86, 0.72)], [(0.5, 0.54), (0.5, 0.12)]] {
        let n = 5;
        for s in 0..n {
            let t0 = s as f32 / n as f32;
            let t1 = t0 + 0.5 / n as f32;
            let p = |t: f32| (seg[0].0 + (seg[1].0 - seg[0].0) * t, seg[0].1 + (seg[1].1 - seg[0].1) * t);
            k.line(&[p(t0), p(t1)], 1.0, c);
        }
    }
}

fn page(k: &Pen) {
    k.fill(&[(0.22, 0.1), (0.62, 0.1), (0.8, 0.28), (0.8, 0.9), (0.22, 0.9)], PAPER);
    k.closed(&[(0.22, 0.1), (0.62, 0.1), (0.8, 0.28), (0.8, 0.9), (0.22, 0.9)], 1.4, INK);
    k.line(&[(0.62, 0.1), (0.62, 0.28), (0.8, 0.28)], 1.2, INK);
}

pub fn paint_ui(k: &Pen, i: UiIcon) {
    match i {
        UiIcon::New => {
            page(k);
            k.line(&[(0.38, 0.6), (0.64, 0.6)], 1.6, BLUE);
            k.line(&[(0.51, 0.47), (0.51, 0.73)], 1.6, BLUE);
        }
        UiIcon::Open => {
            k.fill(&[(0.1, 0.3), (0.36, 0.3), (0.42, 0.38), (0.84, 0.38), (0.84, 0.82), (0.1, 0.82)], Color32::from_rgb(242, 200, 110));
            k.closed(&[(0.1, 0.3), (0.36, 0.3), (0.42, 0.38), (0.84, 0.38), (0.84, 0.82), (0.1, 0.82)], 1.3, INK);
            k.fill(&[(0.2, 0.5), (0.94, 0.5), (0.84, 0.82), (0.1, 0.82)], Color32::from_rgb(250, 220, 140));
            k.closed(&[(0.2, 0.5), (0.94, 0.5), (0.84, 0.82), (0.1, 0.82)], 1.3, INK);
        }
        UiIcon::Save => {
            k.fill(&[(0.14, 0.14), (0.74, 0.14), (0.86, 0.26), (0.86, 0.86), (0.14, 0.86)], Color32::from_rgb(70, 110, 170));
            k.fill(&[(0.28, 0.14), (0.66, 0.14), (0.66, 0.38), (0.28, 0.38)], PAPER);
            k.fill(&[(0.26, 0.56), (0.74, 0.56), (0.74, 0.86), (0.26, 0.86)], PAPER);
            k.closed(&[(0.14, 0.14), (0.74, 0.14), (0.86, 0.26), (0.86, 0.86), (0.14, 0.86)], 1.2, INK);
        }
        UiIcon::Cut => {
            k.circle(0.3, 0.76, 0.12, 1.6, INK);
            k.circle(0.7, 0.76, 0.12, 1.6, INK);
            k.line(&[(0.37, 0.66), (0.66, 0.14)], 1.6, INK);
            k.line(&[(0.63, 0.66), (0.34, 0.14)], 1.6, INK);
        }
        UiIcon::Copy => {
            k.fill(&[(0.14, 0.12), (0.56, 0.12), (0.56, 0.66), (0.14, 0.66)], PAPER);
            k.closed(&[(0.14, 0.12), (0.56, 0.12), (0.56, 0.66), (0.14, 0.66)], 1.3, INK);
            k.fill(&[(0.4, 0.32), (0.84, 0.32), (0.84, 0.88), (0.4, 0.88)], PAPER);
            k.closed(&[(0.4, 0.32), (0.84, 0.32), (0.84, 0.88), (0.4, 0.88)], 1.3, INK);
        }
        UiIcon::Paste => {
            k.fill(&[(0.18, 0.18), (0.78, 0.18), (0.78, 0.88), (0.18, 0.88)], Color32::from_rgb(200, 160, 100));
            k.closed(&[(0.18, 0.18), (0.78, 0.18), (0.78, 0.88), (0.18, 0.88)], 1.3, INK);
            k.fill(&[(0.36, 0.1), (0.6, 0.1), (0.6, 0.26), (0.36, 0.26)], Color32::from_gray(120));
            k.fill(&[(0.42, 0.4), (0.88, 0.4), (0.88, 0.92), (0.42, 0.92)], PAPER);
            k.closed(&[(0.42, 0.4), (0.88, 0.4), (0.88, 0.92), (0.42, 0.92)], 1.2, INK);
        }
        UiIcon::Delete => {
            k.line(&[(0.22, 0.22), (0.78, 0.78)], 2.6, RED);
            k.line(&[(0.78, 0.22), (0.22, 0.78)], 2.6, RED);
        }
        UiIcon::Print => {
            k.fill(&[(0.28, 0.1), (0.72, 0.1), (0.72, 0.4), (0.28, 0.4)], PAPER);
            k.closed(&[(0.28, 0.1), (0.72, 0.1), (0.72, 0.4), (0.28, 0.4)], 1.2, INK);
            k.fill(&[(0.1, 0.4), (0.9, 0.4), (0.9, 0.72), (0.1, 0.72)], Color32::from_gray(150));
            k.closed(&[(0.1, 0.4), (0.9, 0.4), (0.9, 0.72), (0.1, 0.72)], 1.2, INK);
            k.fill(&[(0.28, 0.6), (0.72, 0.6), (0.72, 0.9), (0.28, 0.9)], PAPER);
            k.closed(&[(0.28, 0.6), (0.72, 0.6), (0.72, 0.9), (0.28, 0.9)], 1.2, INK);
        }
        UiIcon::MakeComponent => {
            cube_faces(k, FACE, FACE_D, FACE);
            cube_outline(k, 1.3, INK);
            k.dot(0.8, 0.2, 0.1, BLUE);
        }
        UiIcon::XRay => {
            cube_faces(k, Color32::from_rgba_unmultiplied(170, 196, 226, 110), Color32::from_rgba_unmultiplied(120, 150, 190, 110), Color32::from_rgba_unmultiplied(170, 196, 226, 110));
            hidden_edges(k, INK);
            cube_outline(k, 1.2, INK);
        }
        UiIcon::BackEdges => {
            cube_faces(k, PAPER, PAPER, PAPER);
            hidden_edges(k, INK);
            cube_outline(k, 1.2, INK);
        }
        UiIcon::Edges => {
            cube_faces(k, PAPER, PAPER, PAPER);
            cube_outline(k, 2.0, INK);
        }
        UiIcon::Ao => {
            cube_faces(k, Color32::from_gray(235), Color32::from_gray(190), Color32::from_gray(215));
            k.fill(&[(0.5, 0.48), (0.5, 0.9), (0.42, 0.86), (0.42, 0.52)], Color32::from_gray(140));
            k.fill(&[(0.5, 0.48), (0.86, 0.3), (0.78, 0.3), (0.5, 0.44)], Color32::from_gray(160));
            cube_outline(k, 1.0, INK);
        }
        UiIcon::Wireframe => {
            hidden_edges(k, INK);
            cube_outline(k, 1.2, INK);
            for seg in [[(0.14, 0.72), (0.5, 0.54)], [(0.5, 0.54), (0.86, 0.72)], [(0.5, 0.54), (0.5, 0.12)]] {
                k.line(&seg, 1.1, INK);
            }
        }
        UiIcon::HiddenLine => {
            cube_faces(k, PAPER, PAPER, PAPER);
            cube_outline(k, 1.3, INK);
        }
        UiIcon::Mono => {
            cube_faces(k, Color32::from_gray(245), Color32::from_gray(200), Color32::from_gray(225));
            cube_outline(k, 1.2, INK);
        }

        UiIcon::Textured => {
            cube_faces(k, Color32::from_rgb(230, 170, 130), Color32::from_rgb(180, 100, 70), Color32::from_rgb(205, 130, 90));
            for t in [0.35f32, 0.6] {
                k.line(&[(0.14, 0.3 + 0.42 * t), (0.5, 0.48 + 0.42 * t)], 1.0, PAPER);
                k.line(&[(0.5, 0.48 + 0.42 * t), (0.86, 0.3 + 0.42 * t)], 1.0, PAPER);
            }
            cube_outline(k, 1.2, INK);
        }
        UiIcon::Pbr => {
            k.p.circle_filled(k.at(0.5, 0.52), 0.36 * k.r.width(), Color32::from_rgb(150, 170, 190));
            k.p.circle_filled(k.at(0.44, 0.46), 0.22 * k.r.width(), Color32::from_rgb(190, 205, 220));
            k.p.circle_filled(k.at(0.38, 0.38), 0.08 * k.r.width(), Color32::WHITE);
            k.circle(0.5, 0.52, 0.36, 1.2, INK);
        }
        UiIcon::ViewIso => {
            cube_faces(k, FACE, FACE_D, FACE);
            cube_outline(k, 1.2, INK);
        }
        UiIcon::ViewTop | UiIcon::ViewFront | UiIcon::ViewRight | UiIcon::ViewLeft | UiIcon::ViewBack | UiIcon::ViewBottom => {
            // a small house seen in 3/4 with the viewed face highlighted
            cube_faces(k, PAPER, PAPER, PAPER);
            let hl = Color32::from_rgb(110, 160, 220);
            match i {
                UiIcon::ViewTop => k.fill(&TOP, hl),
                UiIcon::ViewFront => k.fill(&LEFT, hl),
                UiIcon::ViewRight => k.fill(&RIGHT, hl),
                _ => {}
            }
            cube_outline(k, 1.2, INK);
            match i {
                UiIcon::ViewLeft | UiIcon::ViewBack | UiIcon::ViewBottom => {
                    hidden_edges(k, hl);
                    let (x, y) = match i {
                        UiIcon::ViewLeft => (0.08, 0.5),
                        UiIcon::ViewBack => (0.92, 0.18),
                        _ => (0.5, 0.98),
                    };
                    k.dot(x, y, 0.08, hl);
                }
                _ => {}
            }
        }
        UiIcon::SectionPlanes => {
            k.fill(&[(0.2, 0.2), (0.62, 0.08), (0.8, 0.8), (0.38, 0.92)], Color32::from_rgba_unmultiplied(240, 150, 40, 120));
            k.closed(&[(0.2, 0.2), (0.62, 0.08), (0.8, 0.8), (0.38, 0.92)], 1.4, Color32::from_rgb(220, 120, 20));
            k.arrow_head((0.86, 0.42), (1.0, -0.2), 0.14, Color32::from_rgb(220, 120, 20));
        }
        UiIcon::SectionCuts => {
            cube_faces(k, FACE, FACE_D, FACE);
            k.fill(&[(0.5, 0.48), (0.86, 0.3), (0.86, 0.72), (0.5, 0.9)], PAPER);
            k.line(&[(0.5, 0.48), (0.86, 0.3), (0.86, 0.72), (0.5, 0.9), (0.5, 0.48)], 2.0, INK);
            k.line(&[(0.14, 0.3), (0.5, 0.12), (0.86, 0.3)], 1.0, INK);
            k.line(&[(0.14, 0.3), (0.14, 0.72), (0.5, 0.9)], 1.0, INK);
        }
        UiIcon::SectionFill => {
            cube_faces(k, FACE, FACE_D, FACE);
            k.fill(&RIGHT, Color32::from_gray(50));
            cube_outline(k, 1.1, INK);
        }
        UiIcon::PrevView => {
            k.arc(0.52, 0.56, 0.28, 0.0, 180.0, 2.0, INK);
            k.arrow_head((0.24, 0.62), (0.0, 1.0), 0.16, INK);
            k.dot(0.52, 0.56, 0.06, RED);
        }
        UiIcon::Pin => {
            k.p.circle_filled(k.at(0.5, 0.36), 0.22 * k.r.width(), RED);
            k.fill(&[(0.32, 0.46), (0.68, 0.46), (0.5, 0.9)], RED);
            k.p.circle_filled(k.at(0.5, 0.36), 0.08 * k.r.width(), Color32::WHITE);
        }
        UiIcon::Photo => {
            k.closed(&[(0.1, 0.2), (0.9, 0.2), (0.9, 0.8), (0.1, 0.8)], 1.5, INK);
            k.fill(&[(0.16, 0.74), (0.4, 0.42), (0.56, 0.6), (0.66, 0.5), (0.84, 0.74)], Color32::from_rgb(90, 140, 90));
            k.p.circle_filled(k.at(0.7, 0.34), 0.07 * k.r.width(), Color32::from_rgb(240, 170, 30));
        }
        UiIcon::SbContours => {
            for (k2, c) in [(0.0f32, Color32::from_rgb(110, 160, 90)), (0.12, Color32::from_rgb(90, 130, 70)), (0.24, Color32::from_rgb(70, 100, 50))] {
                k.arc(0.5, 0.62 - k2, 0.36 - k2, 10.0, 170.0, 1.4, c);
            }
        }
        UiIcon::SbScratch => terrain(k, 0.0),
        UiIcon::SbSmoove => {
            terrain(k, 0.22);
            k.arrow_head((0.4, 0.2), (0.0, -1.0), 0.12, RED);
        }
        UiIcon::SbStamp => {
            terrain(k, 0.0);
            k.fill(&[(0.32, 0.2), (0.62, 0.2), (0.62, 0.42), (0.32, 0.42)], FACE_D);
        }
        UiIcon::SbDrape => {
            terrain(k, 0.1);
            k.line(&[(0.2, 0.18), (0.8, 0.18)], 1.6, RED);
            k.arrow_head((0.5, 0.42), (0.0, 1.0), 0.1, RED);
        }
        UiIcon::SbDetail => {
            terrain(k, 0.0);
            k.line(&[(0.37, 0.78), (0.68, 0.6)], 1.0, Color32::from_rgb(110, 160, 90));
            k.dot(0.5, 0.66, 0.05, RED);
        }
        UiIcon::SbFlip => {
            k.closed(&[(0.15, 0.8), (0.85, 0.8), (0.85, 0.2), (0.15, 0.2)], 1.3, Color32::from_rgb(110, 160, 90));
            k.line(&[(0.15, 0.8), (0.85, 0.2)], 1.0, Color32::from_gray(150));
            k.line(&[(0.15, 0.2), (0.85, 0.8)], 1.8, RED);
        }
        UiIcon::Help => {
            k.circle(0.5, 0.5, 0.4, 1.6, INK);
            k.arc(0.5, 0.38, 0.13, -60.0, 180.0, 1.8, INK);
            k.line(&[(0.56, 0.49), (0.5, 0.56), (0.5, 0.62)], 1.8, INK);
            k.dot(0.5, 0.74, 0.05, INK);
        }

    }
}
