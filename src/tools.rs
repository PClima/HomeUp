//! Tool behaviour (one handler per tool) and the Measurements box.

use crate::app::{HomeUpApp, In, LastAction, Tool, ToolState, XKind};
use crate::doc::{Guide, Owner};
use crate::geom::{basis_for, Instance, Plane, Sel};
use crate::pick::{axis_color, closest_line_param, infer, seg_dist, Snap, SnapKind, axes};
use crate::units::{format_len, parse_angle, parse_array, parse_len, parse_pair, parse_sides};
use eframe::egui::Color32;
use glam::{DMat4, DVec2, DVec3};
use std::collections::HashSet;

const DARK: Color32 = Color32::from_rgb(20, 20, 20);
const MAGENTA: Color32 = Color32::from_rgb(220, 0, 220);

pub fn circle_points(c: DVec3, n: DVec3, u: DVec3, r: f64, sides: usize) -> Vec<DVec3> {
    let v = n.normalize().cross(u).normalize();
    (0..sides)
        .map(|k| {
            let a = k as f64 / sides as f64 * std::f64::consts::TAU;
            c + (u * a.cos() + v * a.sin()) * r
        })
        .collect()
}

/// Points of a 2-point arc from `a` to `b` bulging `s` along `w` (unit, ⟂ chord).
pub fn arc_points(a: DVec3, b: DVec3, w: DVec3, s: f64, sides: usize) -> Vec<DVec3> {
    let c = a.distance(b);
    if s.abs() < 1e-9 || c < 1e-9 {
        return vec![a, b];
    }
    let mid = (a + b) * 0.5;
    let r = (c * c / 4.0 + s * s) / (2.0 * s);
    let center = mid + w * (s - r);
    let x = (a - center) / r;
    let y = (w - x * x.dot(w)).normalize();
    let beta = ((r - s) / r).clamp(-1.0, 1.0).acos();
    let mut pts: Vec<DVec3> = (0..=sides)
        .map(|k| {
            let t = 2.0 * beta * k as f64 / sides as f64;
            center + (x * t.cos() + y * t.sin()) * r
        })
        .collect();
    pts[0] = a;
    *pts.last_mut().unwrap() = b;
    pts
}

impl HomeUpApp {
    pub(crate) fn snap(&mut self, px: DVec2, anchor: Option<DVec3>) -> Snap {
        let s = infer(&self.cache, &self.doc.guides, &self.cam, px, anchor, &mut self.inf);
        self.snap = Some(s);
        s
    }

    pub(crate) fn line_color_pub(kind: SnapKind) -> Color32 {
        Self::line_color(kind)
    }

    pub(crate) fn finishes_pub(&self, i: &In) -> bool {
        self.finishes(i)
    }

    fn line_color(kind: SnapKind) -> Color32 {
        match kind {
            SnapKind::Axis(i) => axis_color(i),
            SnapKind::Parallel | SnapKind::Perpendicular | SnapKind::PerpFace => MAGENTA,
            _ => DARK,
        }
    }

    fn finishes(&self, i: &In) -> bool {
        i.pressed || (i.released && i.dragging_far)
    }

    /// Entity under the cursor in the active context.
    pub(crate) fn entity_at(&self, px: DVec2) -> Sel {
        let mut s = Sel::default();
        if let Some(ei) = self.cache.pick_edge(&self.cam, px, 6.0, |e| e.owner != Owner::Outside) {
            let e = &self.cache.edges[ei];
            match e.owner {
                Owner::Active => s.edges.extend(self.active().curve_edges(e.edge)),
                Owner::Inst(i) => {
                    s.insts.insert(i);
                }
                Owner::Outside => {}
            }
            return s;
        }
        let (o, d) = self.cam.ray(px);
        if let Some((_, ti)) = self.cache.pick_face(o, d) {
            let t = &self.cache.tris[ti];
            match t.owner {
                Owner::Active => {
                    s.faces.insert(t.face);
                }
                Owner::Inst(i) => {
                    s.insts.insert(i);
                }
                Owner::Outside => {}
            }
        }
        s
    }

    fn hover_entity(&mut self, px: DVec2) {
        let e = self.entity_at(px);
        self.hover_face = e.faces.iter().next().copied();
        self.hover_inst = e.insts.iter().next().copied();
    }

    /// Ensure a selection exists (select the hovered entity). Returns false if none.
    fn ensure_selection(&mut self, px: DVec2) -> bool {
        if self.sel.is_empty() {
            self.sel = self.entity_at(px);
            self.dirty = true;
        }
        if self.sel.is_empty() {
            self.message = "Selecione algo primeiro (ou clique sobre uma entidade).".into();
            return false;
        }
        true
    }

    pub(crate) fn tool_frame(&mut self, i: &In) {
        match self.tool {
            Tool::Select => self.select_tool(i),
            Tool::Line => self.line_tool(i),
            Tool::Rect => self.rect_tool(i),
            Tool::Circle => self.circle_tool(i, false),
            Tool::Polygon => self.circle_tool(i, true),
            Tool::Arc => self.arc_tool(i),
            Tool::PushPull => self.push_tool(i),
            Tool::Offset => self.offset_tool(i),
            Tool::Move => self.move_tool(i),
            Tool::Rotate => self.rotate_tool(i),
            Tool::Scale => self.scale_tool(i),
            Tool::Tape => self.tape_tool(i),
            Tool::Paint => self.paint_tool(i),
            Tool::Eraser => self.eraser_tool(i),
            Tool::Place(d) => self.place_tool(i, d),
            Tool::ArcCenter => self.center_arc_tool(i, false),
            Tool::Pie => self.center_arc_tool(i, true),
            Tool::Arc3 => self.arc3_tool(i),
            Tool::Freehand => self.freehand_tool(i),
            Tool::RotRect => self.rot_rect_tool(i),
            Tool::Lasso => self.lasso_tool(i),
            Tool::Sample => self.sample_tool(i),
            Tool::FollowMe => self.follow_me_tool(i),
            Tool::Flip => self.flip_tool(i),
            Tool::OuterShell | Tool::Union | Tool::Subtract | Tool::SolidIntersect | Tool::Trim | Tool::Split => self.solid_tool(i),
            Tool::Dimension => self.dimension_tool(i),
            Tool::Text => self.text_tool(i),
            Tool::Text3d => {}
            Tool::Protractor => self.protractor_tool(i),
            Tool::Axes => self.axes_tool(i),
            Tool::Section => self.section_tool(i),
            Tool::Tag => self.tag_tool(i),
            Tool::ZoomWindow => self.zoom_window_tool(i),
            Tool::PositionCamera => self.position_camera_tool(i),
            Tool::LookAround => self.look_around_tool(i),
            Tool::Walk => self.walk_tool(i),
            Tool::Orbit | Tool::Pan | Tool::Zoom => self.nav_tool(i),
        }
    }

    // ------------------------------------------------------------ navigation

    fn nav_tool(&mut self, i: &In) {
        if i.primary_down {
            match self.tool {
                Tool::Orbit if i.mods.shift => self.cam.pan(i.delta),
                Tool::Orbit => self.cam.orbit(i.delta),
                Tool::Pan => self.cam.pan(i.delta),
                _ => {
                    let t = self.cam.target;
                    if i.mods.shift {
                        self.cam.fov_y = (self.cam.fov_y + i.delta.y * 0.003).clamp(0.1, 2.6);
                    } else {
                        self.cam.zoom_at(t, (i.delta.y * 0.01).exp());
                    }
                }
            }
        }
        if i.dbl && self.tool == Tool::Orbit {
            let (o, d) = self.cam.ray(i.px);
            if let Some((t, _)) = self.cache.pick_face(o, d) {
                self.cam.target = o + d * t;
            }
        }
    }

    // ------------------------------------------------------------ drawing

    pub(crate) fn commit_line(&mut self, a: DVec3, b: DVec3) {
        if a.distance(b) < 1e-6 {
            return;
        }
        let before = self.begin_op();
        let (la, lb, eye) = (self.to_local(a), self.to_local(b), self.local_eye());
        let closed = self.active_mut().add_line(la, lb, eye);
        self.last = Some(LastAction::Line { before, start: a, dir: (b - a).normalize() });
        self.state = if closed { ToolState::Idle } else { ToolState::LineFrom(b) };
        self.inf.lock = None;
    }

    fn line_tool(&mut self, i: &In) {
        let anchor = if let ToolState::LineFrom(a) = self.state { Some(a) } else { None };
        let s = self.snap(i.px, anchor);
        if let Some(a) = anchor {
            self.overlay.lines.push((a, s.p, Self::line_color(s.kind), 2.0));
            self.live_measure = Some(format_len(a.distance(s.p)));
            if self.finishes(i) {
                self.commit_line(a, s.p);
            }
        } else if i.pressed {
            self.state = ToolState::LineFrom(s.p);
        }
    }

    fn rect_corners(p0: DVec3, u: DVec3, v: DVec3, w: f64, h: f64) -> [DVec3; 4] {
        [p0, p0 + u * w, p0 + u * w + v * h, p0 + v * h]
    }

    pub(crate) fn commit_rect(&mut self, p0: DVec3, u: DVec3, v: DVec3, w: f64, h: f64) {
        if w.abs() < 1e-6 || h.abs() < 1e-6 {
            return;
        }
        let before = self.begin_op();
        self.add_local_polygon(&Self::rect_corners(p0, u, v, w, h), false, false);
        self.last = Some(LastAction::Rect { before, p0, u, v, su: w.signum(), sv: h.signum(), w: w.abs(), h: h.abs() });
        self.state = ToolState::Idle;
    }

    pub(crate) fn add_local_polygon(&mut self, pts: &[DVec3], curve: bool, arc: bool) {
        let local: Vec<DVec3> = pts.iter().map(|p| self.to_local(*p)).collect();
        let eye = self.local_eye();
        let g = self.active_mut();
        let c = curve.then(|| g.new_curve());
        g.add_polygon(&local, c, arc, eye);
    }

    /// Drawing plane frame (plane + in-plane axes) for a first click.
    fn plane_frame(&self, s: &Snap) -> (Plane, DVec3, DVec3) {
        if let Some(i) = self.plane_lock {
            let n = axes()[i];
            let (u, v) = match i {
                0 => (DVec3::Y, DVec3::Z),
                1 => (DVec3::X, DVec3::Z),
                _ => (DVec3::X, DVec3::Y),
            };
            return (Plane::new(n, s.p), u, v);
        }
        if let Some(f) = s.face {
            let n = f.n;
            let u = axes().iter().copied().find(|a| a.dot(n).abs() < 1e-6).unwrap_or(f.u);
            let u = (u - n * n.dot(u)).normalize();
            return (Plane::new(n, s.p), u, n.cross(u).normalize());
        }
        (Plane::new(DVec3::Z, s.p), DVec3::X, DVec3::Y)
    }

    fn on_plane(&self, plane: &Plane, s: &Snap, px: DVec2) -> DVec3 {
        if matches!(s.kind, SnapKind::Free | SnapKind::OnFace) {
            let (o, d) = self.cam.ray(px);
            plane.intersect_ray(o, d).unwrap_or(plane.project(s.p))
        } else {
            plane.project(s.p)
        }
    }

    fn rect_tool(&mut self, i: &In) {
        match self.state.clone() {
            ToolState::RectFrom { p0, plane, u, v } => {
                let s = self.snap(i.px, None);
                let p = self.on_plane(&plane, &s, i.px);
                let (mut w, mut h) = ((p - p0).dot(u), (p - p0).dot(v));
                let mut shape = None;
                if matches!(s.kind, SnapKind::Free | SnapKind::OnFace) {
                    let (aw, ah) = (w.abs(), h.abs());
                    let phi = 1.618_033_988_75;
                    if aw > 1e-6 && ah > 1e-6 {
                        let r = aw / ah;
                        if (r - 1.0).abs() < 0.03 {
                            let m = (aw + ah) * 0.5;
                            w = m * w.signum();
                            h = m * h.signum();
                            shape = Some(SnapKind::Square);
                        } else if (r - phi).abs() < 0.04 {
                            h = aw / phi * h.signum();
                            shape = Some(SnapKind::Golden);
                        } else if (1.0 / r - phi).abs() < 0.04 {
                            w = ah / phi * w.signum();
                            shape = Some(SnapKind::Golden);
                        }
                    }
                }
                self.snap = Some(Snap { kind: shape.unwrap_or(s.kind), p: p0 + u * w + v * h, ..s });
                self.last_rect_preview = Some((w, h));
                let c = Self::rect_corners(p0, u, v, w, h);
                let col = if shape.is_some() { SnapKind::Square.color() } else { DARK };
                for k in 0..4 {
                    self.overlay.lines.push((c[k], c[(k + 1) % 4], col, 2.0));
                }
                self.live_measure = Some(format!("{} ; {}", format_len(w.abs()), format_len(h.abs())));
                if self.finishes(i) {
                    self.commit_rect(p0, u, v, w, h);
                }
            }
            _ => {
                let s = self.snap(i.px, None);
                if i.pressed {
                    let (plane, u, v) = self.plane_frame(&s);
                    self.state = ToolState::RectFrom { p0: plane.project(s.p), plane, u, v };
                }
            }
        }
    }

    pub(crate) fn commit_circle(&mut self, c: DVec3, n: DVec3, u: DVec3, r: f64, polygon: bool) {
        if r < 1e-6 {
            return;
        }
        let sides = if polygon { self.polygon_sides } else { self.circle_sides };
        let before = self.begin_op();
        self.add_local_polygon(&circle_points(c, n, u, r, sides), true, !polygon);
        self.last = Some(LastAction::Circle { before, c, n, u, r, polygon });
        self.state = ToolState::Idle;
    }

    fn circle_tool(&mut self, i: &In, polygon: bool) {
        let sides = if polygon { self.polygon_sides } else { self.circle_sides };
        match self.state.clone() {
            ToolState::CircleFrom { c, n, u } => {
                let s = self.snap(i.px, Some(c));
                let plane = Plane::new(n, c);
                let p = self.on_plane(&plane, &s, i.px);
                let r = p.distance(c);
                let pts = circle_points(c, n, u, r, sides);
                for k in 0..pts.len() {
                    self.overlay.lines.push((pts[k], pts[(k + 1) % pts.len()], DARK, 2.0));
                }
                self.overlay.lines.push((c, p, Self::line_color(s.kind), 1.0));
                self.live_measure = Some(format!("{} ({} lados)", format_len(r), sides));
                if self.finishes(i) {
                    self.commit_circle(c, n, u, r, polygon);
                }
            }
            _ => {
                let s = self.snap(i.px, None);
                let (plane, u, _) = self.plane_frame(&s);
                // orientation preview
                let r = self.cam.world_per_px(self.cam.depth(s.p)) * 25.0;
                let pts = circle_points(s.p, plane.n, u, r, 24);
                let col = axis_color(axes().iter().position(|a| a.dot(plane.n).abs() > 0.999).unwrap_or(2));
                for k in 0..pts.len() {
                    self.overlay.lines.push((pts[k], pts[(k + 1) % pts.len()], col, 1.0));
                }
                self.live_measure = Some(format!("{sides} lados"));
                if i.pressed {
                    let n = if plane.n.dot(self.cam.eye() - s.p) < 0.0 { -plane.n } else { plane.n };
                    self.state = ToolState::CircleFrom { c: plane.project(s.p), n, u };
                }
            }
        }
    }

    pub(crate) fn commit_arc(&mut self, a: DVec3, b: DVec3, w: DVec3, s: f64) {
        let pts = arc_points(a, b, w, s, self.arc_sides);
        let before = self.begin_op();
        let local: Vec<DVec3> = pts.iter().map(|p| self.to_local(*p)).collect();
        let eye = self.local_eye();
        let g = self.active_mut();
        let c = g.new_curve();
        let segs: Vec<(DVec3, DVec3)> = local.windows(2).map(|p| (p[0], p[1])).collect();
        g.add_segments(&segs, Some(c), true, eye);
        self.last = Some(LastAction::Arc { before, a, b, w, s });
        self.state = ToolState::Idle;
    }

    fn arc_tool(&mut self, i: &In) {
        match self.state.clone() {
            ToolState::ArcStart(a) => {
                let s = self.snap(i.px, Some(a));
                self.overlay.lines.push((a, s.p, Self::line_color(s.kind), 2.0));
                self.live_measure = Some(format_len(a.distance(s.p)));
                if self.finishes(i) && s.p.distance(a) > 1e-6 {
                    self.state = ToolState::ArcChord { a, b: s.p };
                }
            }
            ToolState::ArcChord { a, b } => {
                let s = self.snap(i.px, None);
                let mid = (a + b) * 0.5;
                let u = (b - a).normalize();
                let wv = (s.p - mid) - u * (s.p - mid).dot(u);
                let mut bulge = wv.length();
                let w = if bulge > 1e-9 { wv / bulge } else { basis_for(u).0 };
                let c = a.distance(b);
                let tol = self.cam.world_per_px(self.cam.depth(mid)) * 8.0;
                if (bulge - c / 2.0).abs() < tol {
                    bulge = c / 2.0;
                    self.snap = Some(Snap { kind: SnapKind::HalfCircle, p: mid + w * bulge, ..s });
                }
                let pts = arc_points(a, b, w, bulge, self.arc_sides);
                for p in pts.windows(2) {
                    self.overlay.lines.push((p[0], p[1], DARK, 2.0));
                }
                self.overlay.dashed.push((mid, mid + w * bulge, Color32::from_gray(90)));
                self.live_measure = Some(format!("{} ({} lados)", format_len(bulge), self.arc_sides));
                if self.finishes(i) {
                    self.commit_arc(a, b, w, bulge);
                }
            }
            _ => {
                let s = self.snap(i.px, None);
                if i.pressed {
                    self.state = ToolState::ArcStart(s.p);
                }
            }
        }
    }

    // ------------------------------------------------------------ push/pull & offset

    fn push_distance(&self, origin: DVec3, normal: DVec3, offsets: &[f64], px: DVec2, s: &Snap) -> (f64, bool) {
        let d = if s.kind.is_point() {
            (s.p - origin).dot(normal)
        } else {
            let (o, dir) = self.cam.ray(px);
            closest_line_param(origin, normal, o, dir)
        };
        let tol = self.cam.world_per_px(self.cam.depth(origin)) * 8.0;
        for t in offsets {
            if (d - t).abs() < tol {
                return (*t, true);
            }
        }
        (d, false)
    }

    /// Local distance factor for a face: local_d = world_d * k.
    fn normal_scale(&self, normal_w: DVec3, normal_l: DVec3) -> f64 {
        self.ctx_xf().inverse().transform_vector3(normal_w).dot(normal_l)
    }

    pub(crate) fn commit_push(&mut self, face: usize, d_world: f64, normal_w: DVec3, copy: bool) {
        self.state = ToolState::Idle;
        self.dirty = true;
        if d_world.abs() < 1e-6 {
            return;
        }
        let before = self.begin_op();
        let k = self.normal_scale(normal_w, before.geom(&self.path).faces[face].normal);
        let eye = self.local_eye();
        let r = self.active_mut().push_pull(face, d_world * k, copy, eye);
        if r.through_cut {
            self.message = "Furo passante criado.".into();
        }
        self.last_push = Some(d_world);
        self.last = Some(LastAction::PushPull { before, face, d: d_world, normal: normal_w, copy });
    }

    fn push_tool(&mut self, i: &In) {
        match self.state.clone() {
            ToolState::Pushing { face, origin, normal, press, .. } => {
                let s = self.snap(i.px, None);
                let k = self.normal_scale(normal, self.active().faces[face].normal);
                let offsets: Vec<f64> = self.active().parallel_face_offsets(face).iter().map(|t| t / k).collect();
                let (d, on_face) = self.push_distance(origin, normal, &offsets, i.px, &s);
                let mut pd = self.doc.clone();
                let eye = self.local_eye();
                pd.geom_mut(&self.path).push_pull(face, d * k, self.copy_now(), eye);
                self.preview = Some(pd);
                self.live_measure = Some(format_len(d));
                self.snap = Some(Snap { p: origin + normal * d, kind: if on_face { SnapKind::OnFace } else { s.kind }, ..s });
                if on_face {
                    self.overlay.tooltip = Some(("Desvio até a face paralela".into(), SnapKind::OnFace.color()));
                }
                if let ToolState::Pushing { d: sd, .. } = &mut self.state {
                    *sd = d;
                }
                let far = i.pos.distance(press) > 5.0;
                if i.dbl && !far {
                    if let Some(ld) = self.last_push {
                        self.commit_push(face, ld, normal, self.copy_now());
                        return;
                    }
                }
                if (i.pressed && i.pos.distance(press) > 1.0) || (i.released && far) {
                    self.commit_push(face, d, normal, self.copy_now());
                }
            }
            _ => {
                let (o, d) = self.cam.ray(i.px);
                let hit = self.cache.pick_face(o, d).filter(|h| self.cache.tris[h.1].owner == Owner::Active);
                self.hover_face = hit.map(|h| self.cache.tris[h.1].face);
                if let (true, Some((t, ti))) = (i.pressed, hit) {
                    let tri = &self.cache.tris[ti];
                    self.state = ToolState::Pushing { face: tri.face, origin: o + d * t, normal: tri.n, d: 0.0, press: i.pos };
                    self.last = None;
                }
            }
        }
    }

    pub(crate) fn commit_offset(&mut self, face: usize, d: f64) {
        self.state = ToolState::Idle;
        if d.abs() < 1e-6 {
            return;
        }
        let before = self.begin_op();
        let pts = self.active().offset_loop(face, d);
        let eye = self.local_eye();
        self.active_mut().add_polygon(&pts, None, false, eye);
        self.last = Some(LastAction::Offset { before, face, d });
    }

    fn offset_tool(&mut self, i: &In) {
        match self.state.clone() {
            ToolState::Offsetting { face, press } => {
                let s = self.snap(i.px, None);
                let g = self.active();
                let f = &g.faces[face];
                let ctx = self.ctx_xf();
                let plane_w = Plane::new(ctx.inverse().transpose().transform_vector3(f.normal), ctx.transform_point3(g.p(f.outer[0])));
                let p = self.on_plane(&plane_w, &s, i.px);
                let pl = self.to_local(p);
                let pts: Vec<DVec3> = f.outer.iter().map(|v| g.p(*v)).collect();
                let mut dist = f64::MAX;
                for k in 0..pts.len() {
                    let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
                    let ab = b - a;
                    let t = ((pl - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
                    dist = dist.min((a + ab * t).distance(pl));
                }
                let d = if g.face_contains(f, pl) { dist } else { -dist };
                let poly = g.offset_loop(face, d);
                let world: Vec<DVec3> = poly.iter().map(|q| ctx.transform_point3(*q)).collect();
                for k in 0..world.len() {
                    self.overlay.lines.push((world[k], world[(k + 1) % world.len()], DARK, 2.0));
                }
                self.live_measure = Some(format_len(d));
                if let ToolState::Offsetting { .. } = self.state {
                    self.offset_d = d;
                }
                if (i.pressed && i.pos.distance(press) > 1.0) || (i.released && i.pos.distance(press) > 5.0) {
                    self.commit_offset(face, d);
                }
            }
            _ => {
                let (o, d) = self.cam.ray(i.px);
                let hit = self.cache.pick_face(o, d).filter(|h| self.cache.tris[h.1].owner == Owner::Active);
                self.hover_face = hit.map(|h| self.cache.tris[h.1].face);
                if let (true, Some((_, ti))) = (i.pressed, hit) {
                    self.state = ToolState::Offsetting { face: self.cache.tris[ti].face, press: i.pos };
                    self.offset_d = 0.0;
                }
            }
        }
    }

    // ------------------------------------------------------------ move / rotate / scale

    /// Copy/new-extrusion mode: the Ctrl toggle, inverted while Ctrl is held.
    pub(crate) fn copy_now(&self) -> bool {
        self.copy_mode ^ self.ctrl_held
    }

    pub(crate) fn preview_xform(&mut self, kind: &XKind) {
        self.drop_locked();
        let m = self.local_matrix(kind.matrix(1.0));
        let copy = self.copy_now();
        let mut d = self.doc.clone();
        let g = d.geom_mut(&self.path);
        let s = if copy { g.copy_sel(&self.sel, m, false) } else { g.transform_sel(&self.sel, m, false) };
        self.preview = Some(d);
        self.preview_sel = Some(s);
    }

    pub(crate) fn commit_xform(&mut self, kind: XKind) {
        self.drop_locked();
        self.state = ToolState::Idle;
        self.dirty = true;
        let sel = self.sel.clone();
        let copy = self.copy_now();
        let before = self.begin_op();
        let m = self.local_matrix(kind.matrix(1.0));
        let g = self.active_mut();
        self.sel = if copy { g.copy_sel(&sel, m, true) } else { g.transform_sel(&sel, m, true) };
        self.last = Some(LastAction::Xform { before, sel, kind, copy });
    }

    /// Re-run the last copy as an array of `n` copies (`divide`: spread between).
    pub(crate) fn apply_array(&mut self, n: usize, divide: bool) {
        let Some(LastAction::Xform { before, sel, kind, copy: true }) = self.last.clone() else {
            self.message = "Matrizes (x3, /3) valem logo após uma cópia.".into();
            return;
        };
        self.doc = before;
        let mut last = Sel::default();
        for k in 1..=n {
            let frac = if divide { k as f64 / n as f64 } else { k as f64 };
            let m = self.local_matrix(kind.matrix(frac));
            last = self.active_mut().copy_sel(&sel, m, true);
        }
        self.sel = last;
        self.dirty = true;
        self.cache_dirty = true;
    }

    fn move_tool(&mut self, i: &In) {
        match self.state.clone() {
            ToolState::Moving { base, press } => {
                let s = self.snap(i.px, Some(base));
                let v = s.p - base;
                self.preview_xform(&XKind::Move(v));
                self.overlay.lines.push((base, s.p, Self::line_color(s.kind), 1.5));
                self.live_measure = Some(format_len(v.length()));
                if (i.pressed && i.pos.distance(press) > 1.0) || (i.released && i.pos.distance(press) > 5.0) {
                    self.commit_xform(XKind::Move(v));
                }
            }
            _ => {
                let s = self.snap(i.px, None);
                if self.sel.is_empty() {
                    self.hover_entity(i.px);
                }
                if i.pressed && self.ensure_selection(i.px) {
                    self.state = ToolState::Moving { base: s.p, press: i.pos };
                    self.last = None;
                }
            }
        }
    }

    pub(crate) fn protractor_pub(&mut self, c: DVec3, n: DVec3) {
        self.protractor(c, n);
    }

    pub(crate) fn rot_normal_pub(&self, s: &Snap) -> DVec3 {
        self.rot_normal(s)
    }

    fn protractor(&mut self, c: DVec3, n: DVec3) {
        let r = self.cam.world_per_px(self.cam.depth(c)) * 45.0;
        let (u, _) = basis_for(n);
        let pts = circle_points(c, n, u, r, 36);
        let col = axis_color(axes().iter().position(|a| a.dot(n).abs() > 0.999).unwrap_or(2));
        let col = if axes().iter().any(|a| a.dot(n).abs() > 0.999) { col } else { MAGENTA };
        for k in 0..36 {
            let w = if k % 3 == 0 { 2.0 } else { 1.0 };
            self.overlay.lines.push((pts[k], pts[(k + 1) % 36], col, w));
        }
    }

    fn rot_normal(&self, s: &Snap) -> DVec3 {
        if let Some(i) = self.plane_lock {
            return axes()[i];
        }
        s.face.map(|f| f.n).unwrap_or(DVec3::Z)
    }

    fn rotate_tool(&mut self, i: &In) {
        match self.state.clone() {
            ToolState::RotCenter { c, n } => {
                let s = self.snap(i.px, Some(c));
                let q = s.p - n * (s.p - c).dot(n);
                self.protractor(c, n);
                self.overlay.lines.push((c, q, DARK, 1.5));
                if self.finishes(i) && q.distance(c) > 1e-6 {
                    self.state = ToolState::RotAngle { c, n, r0: (q - c).normalize(), press: i.pos };
                }
            }
            ToolState::RotAngle { c, n, r0, press } => {
                let s = self.snap(i.px, Some(c));
                let q = s.p - n * (s.p - c).dot(n);
                let r = (q - c).normalize_or_zero();
                let mut ang = r0.cross(r).dot(n).atan2(r0.dot(r));
                let step = 15f64.to_radians();
                let snapped = (ang / step).round() * step;
                if (ang - snapped).abs() < 1.5f64.to_radians() {
                    ang = snapped;
                }
                self.protractor(c, n);
                let len = self.cam.world_per_px(self.cam.depth(c)) * 60.0;
                self.overlay.lines.push((c, c + r0 * len, DARK, 1.5));
                self.overlay.lines.push((c, c + DMat4::from_axis_angle(n, ang).transform_vector3(r0) * len, DARK, 1.5));
                self.preview_xform(&XKind::Rotate { c, n, ang });
                self.live_measure = Some(format!("{:.1}°", ang.to_degrees()).replace('.', ","));
                if (i.pressed && i.pos.distance(press) > 1.0) || (i.released && i.pos.distance(press) > 5.0) {
                    self.commit_xform(XKind::Rotate { c, n, ang });
                }
            }
            _ => {
                let s = self.snap(i.px, None);
                let n = self.rot_normal(&s);
                self.protractor(s.p, n);
                if self.sel.is_empty() {
                    self.hover_entity(i.px);
                }
                if i.pressed && self.ensure_selection(i.px) {
                    self.state = ToolState::RotCenter { c: s.p, n };
                    self.last = None;
                }
            }
        }
    }

    pub(crate) fn sel_bounds_world(&self) -> Option<(DVec3, DVec3)> {
        let g = self.active();
        let ctx = self.ctx_xf();
        let mut acc: Option<(DVec3, DVec3)> = None;
        let mut add = |p: DVec3| {
            acc = Some(match acc {
                Some((lo, hi)) => (lo.min(p), hi.max(p)),
                None => (p, p),
            })
        };
        for v in g.sel_vertices(&self.sel) {
            add(ctx.transform_point3(g.p(v)));
        }
        for i in &self.sel.insts {
            let inst = &g.instances[*i];
            if let Some((lo, hi)) = self.doc.geom_bounds(&self.doc.defs[inst.def].geom, ctx * inst.xf) {
                add(lo);
                add(hi);
            }
        }
        acc
    }

    /// Scale grips: (position, index triple in {0,1,2}^3).
    fn grips(&self) -> Vec<(DVec3, [usize; 3])> {
        let Some((lo, hi)) = self.sel_bounds_world() else { return vec![] };
        let mut out = Vec::new();
        for a in 0..3 {
            for b in 0..3 {
                for c in 0..3 {
                    if a == 1 && b == 1 && c == 1 {
                        continue;
                    }
                    let f = DVec3::new(a as f64, b as f64, c as f64) * 0.5;
                    out.push((lo + (hi - lo) * f, [a, b, c]));
                }
            }
        }
        out
    }

    fn scale_tool(&mut self, i: &In) {
        match self.state.clone() {
            ToolState::Scaling { anchor, grip, mask, press } => {
                let len = grip.distance(anchor);
                let (o, d) = self.cam.ray(i.px);
                let t = if len > 1e-9 { closest_line_param(anchor, (grip - anchor) / len, o, d) / len } else { 1.0 };
                let t = if (t - t.round()).abs() < 0.02 && t.round() != 0.0 { t.round() } else { t };
                let kind = XKind::Scale { a: anchor, mask, t };
                self.preview_xform(&kind);
                self.overlay.dashed.push((anchor, anchor + (grip - anchor) * t, Color32::from_rgb(200, 0, 0)));
                self.live_measure = Some(format!("{t:.3}").replace('.', ","));
                if (i.pressed && i.pos.distance(press) > 1.0) || (i.released && i.pos.distance(press) > 5.0) {
                    self.commit_xform(kind);
                }
            }
            _ => {
                self.snap = None;
                if self.sel.is_empty() {
                    self.hover_entity(i.px);
                    if i.pressed {
                        self.ensure_selection(i.px);
                    }
                    return;
                }
                let grips = self.grips();
                let Some((lo, hi)) = self.sel_bounds_world() else { return };
                let mut hovered = None;
                let mut best = 9.0;
                for (k, (p, _)) in grips.iter().enumerate() {
                    if let Some(sp) = self.cam.project(*p) {
                        let dd = sp.distance(i.px);
                        if dd < best {
                            best = dd;
                            hovered = Some(k);
                        }
                    }
                }
                for (k, (p, _)) in grips.iter().enumerate() {
                    let c = if Some(k) == hovered { Color32::from_rgb(220, 0, 0) } else { Color32::from_rgb(0, 160, 0) };
                    self.overlay.squares.push((*p, c));
                }
                if let Some(k) = hovered {
                    let (p, idx) = grips[k];
                    let label = match idx.iter().filter(|x| **x != 1).count() {
                        3 => "Escala uniforme (canto)",
                        2 => "Escala em dois eixos",
                        _ => "Escala em um eixo",
                    };
                    self.overlay.tooltip = Some((label.into(), Color32::from_rgb(220, 0, 0)));
                    self.snap = Some(Snap { p, kind: SnapKind::Free, face: None, dir: None, in_inst: false, ref_line: None });
                    if i.pressed {
                        let mask = DVec3::new(
                            (idx[0] != 1) as u8 as f64,
                            (idx[1] != 1) as u8 as f64,
                            (idx[2] != 1) as u8 as f64,
                        );
                        let anchor = if self.copy_now() {
                            (lo + hi) * 0.5
                        } else {
                            let f = DVec3::new(2.0 - idx[0] as f64, 2.0 - idx[1] as f64, 2.0 - idx[2] as f64) * 0.5;
                            lo + (hi - lo) * f
                        };
                        self.state = ToolState::Scaling { anchor, grip: p, mask, press: i.pos };
                        self.last = None;
                    }
                } else if i.pressed {
                    let e = self.entity_at(i.px);
                    self.sel = e;
                    self.dirty = true;
                }
            }
        }
    }

    // ------------------------------------------------------------ tape, paint, eraser, select, place

    fn tape_tool(&mut self, i: &In) {
        match self.state.clone() {
            ToolState::TapeFrom { p, edge_dir: Some(e) } => {
                let s = self.snap(i.px, None);
                let w = (s.p - p) - e * (s.p - p).dot(e);
                let q = p + w;
                let l = self.cam.dist * 300.0;
                self.overlay.dashed.push((q - e * l, q + e * l, Color32::from_gray(60)));
                self.overlay.lines.push((p, q, DARK, 1.0));
                self.live_measure = Some(format_len(w.length()));
                if self.finishes(i) && w.length() > 1e-6 {
                    self.checkpoint();
                    self.doc.guides.push(Guide::Line { p: q, dir: e });
                    self.state = ToolState::Idle;
                    self.message = format!("Guia criada a {}", format_len(w.length()));
                }
            }
            ToolState::TapeFrom { p, edge_dir: None } => {
                let s = self.snap(i.px, Some(p));
                self.overlay.lines.push((p, s.p, Self::line_color(s.kind), 1.5));
                let dist = p.distance(s.p);
                self.live_measure = Some(format_len(dist));
                if self.finishes(i) && dist > 1e-6 {
                    self.checkpoint();
                    self.doc.guides.push(Guide::Point { p: s.p });
                    self.state = ToolState::Idle;
                    self.tape_last = Some(dist);
                    self.message = format!("Distância medida: {} — digite a medida real e Enter para redimensionar.", format_len(dist));
                }
            }
            _ => {
                let s = self.snap(i.px, None);
                if i.pressed {
                    let edge_dir = (s.kind == SnapKind::OnEdge).then_some(self.inf.last_edge_dir).flatten();
                    self.state = ToolState::TapeFrom { p: s.p, edge_dir };
                }
            }
        }
    }

    fn paint_tool(&mut self, i: &In) {
        self.snap = None;
        let (o, d) = self.cam.ray(i.px);
        let hit = self.cache.pick_face(o, d).map(|h| (self.cache.tris[h.1].owner, self.cache.tris[h.1].face));
        self.hover_face = None;
        self.hover_inst = None;
        match hit {
            Some((Owner::Active, f)) => self.hover_face = Some(f),
            Some((Owner::Inst(k), _)) => self.hover_inst = Some(k),
            _ => {}
        }
        if !i.pressed {
            return;
        }
        let cur = self.cur_mat;
        match hit {
            Some((Owner::Active, f)) => {
                if i.mods.alt {
                    self.cur_mat = self.active().faces[f].material;
                    return;
                }
                if !self.sel.faces.contains(&f) && self.paint_replace(f, i.mods) {
                    return;
                }
                self.checkpoint_keep_sel();
                let g = self.doc.geom_mut(&self.path);
                if self.sel.faces.contains(&f) {
                    for fi in self.sel.faces.iter() {
                        g.faces[*fi].material = cur;
                    }
                    for k in self.sel.insts.iter() {
                        g.instances[*k].material = cur;
                    }
                } else {
                    g.faces[f].material = cur;
                }
            }
            Some((Owner::Inst(k), _)) => {
                if i.mods.alt {
                    self.cur_mat = self.active().instances[k].material;
                    return;
                }
                self.checkpoint_keep_sel();
                self.doc.geom_mut(&self.path).instances[k].material = cur;
            }
            _ => {}
        }
    }

    fn guide_at(&self, px: DVec2) -> Option<usize> {
        for (k, g) in self.doc.guides.iter().enumerate() {
            let hit = match g {
                Guide::Point { p } => self.cam.project(*p).is_some_and(|s| s.distance(px) < 7.0),
                Guide::Line { p, dir } => {
                    let l = self.cam.dist * 300.0;
                    self.cam.project_segment(*p - *dir * l, *p + *dir * l).is_some_and(|(a, b)| seg_dist(px, a, b).0 < 5.0)
                }
            };
            if hit {
                return Some(k);
            }
        }
        None
    }

    fn eraser_tool(&mut self, i: &In) {
        self.snap = None;
        let hit = self.cache.pick_edge(&self.cam, i.px, 7.0, |e| e.owner != Owner::Outside);
        let guide = if hit.is_none() { self.guide_at(i.px) } else { None };
        if hit.is_none() && guide.is_none() {
            let dim = self.dim_at(i.px);
            let label = if dim.is_none() { self.label_at(i.px) } else { None };
            if dim.is_some() || label.is_some() {
                self.overlay.tooltip = Some((if dim.is_some() { "Cota" } else { "Texto" }.into(), Color32::from_gray(80)));
                if i.pressed {
                    self.checkpoint_keep_sel();
                    if let Some(k) = dim {
                        self.doc.dims.remove(k);
                    } else if let Some(k) = label {
                        self.doc.labels.remove(k);
                    }
                    return;
                }
            }
        }
        if i.pressed {
            self.state = ToolState::Erasing { edges: HashSet::new(), guides: HashSet::new() };
        }
        if let ToolState::Erasing { edges, guides } = &mut self.state {
            if let Some(e) = hit {
                edges.insert(e);
            }
            if let Some(g) = guide {
                guides.insert(g);
            }
        }
        let marked: Vec<usize> = match &self.state {
            ToolState::Erasing { edges, .. } => edges.iter().copied().collect(),
            _ => hit.into_iter().collect(),
        };
        for e in marked {
            let pe = &self.cache.edges[e];
            self.overlay.lines.push((pe.a, pe.b, Color32::from_rgb(40, 110, 255), 3.0));
        }
        if let Some(g) = guide {
            self.overlay.tooltip = Some(("Guia".into(), Color32::from_gray(80)));
            let _ = g;
        }
        if !i.released {
            return;
        }
        let ToolState::Erasing { edges, guides } = std::mem::replace(&mut self.state, ToolState::Idle) else { return };
        if edges.is_empty() && guides.is_empty() {
            return;
        }
        let mut pairs = Vec::new();
        let mut insts = HashSet::new();
        for e in edges {
            let pe = &self.cache.edges[e];
            match pe.owner {
                Owner::Active => pairs.push(self.active().edges[pe.edge].v),
                Owner::Inst(k) => {
                    insts.insert(k);
                }
                Owner::Outside => {}
            }
        }
        self.checkpoint();
        let eye = self.local_eye();
        let (soften, hide) = (i.mods.ctrl, i.mods.shift);
        let g = self.doc.geom_mut(&self.path);
        for [a, b] in pairs {
            let Some(ei) = g.find_edge(a, b) else { continue };
            if soften {
                g.edges[ei].soft = !hide;
            } else if hide {
                g.edges[ei].hidden = true;
            } else {
                g.erase_edge(ei, eye);
            }
        }
        if !soften && !hide && !insts.is_empty() {
            let mut k = 0;
            g.instances.retain(|_| {
                k += 1;
                !insts.contains(&(k - 1))
            });
        }
        let mut gs: Vec<usize> = guides.into_iter().collect();
        gs.sort_unstable();
        for k in gs.into_iter().rev() {
            self.doc.guides.remove(k);
        }
    }

    fn select_tool(&mut self, i: &In) {
        self.snap = None;
        if i.pressed {
            self.state = ToolState::Box { start: i.pos };
        }
        if let ToolState::Box { start } = self.state {
            if i.dragging_far {
                self.overlay.box_rect = Some((start, i.pos));
            }
            if i.released {
                self.state = ToolState::Idle;
                if i.dragging_far {
                    self.box_select(start, i.pos, i.mods);
                } else {
                    let e = self.entity_at(i.px);
                    self.apply_selection(e, i.mods);
                }
            }
        }
        if i.triple {
            let e = self.entity_at(i.px);
            let g = self.active();
            let seed: HashSet<u32> = g.sel_vertices(&e);
            if !seed.is_empty() {
                let c = g.connected(&seed);
                self.apply_selection(c, i.mods);
            }
        } else if i.dbl {
            let e = self.entity_at(i.px);
            if let Some(k) = e.insts.iter().next().copied() {
                self.enter_context(k);
            } else if let Some(f) = e.faces.iter().next().copied() {
                let g = self.active();
                let mut s = e.clone();
                for (a, b) in g.faces[f].boundary_edges() {
                    if let Some(ei) = g.find_edge(a, b) {
                        s.edges.insert(ei);
                    }
                }
                self.apply_selection(s, i.mods);
            } else if let Some(ei) = e.edges.iter().next().copied() {
                let g = self.active();
                let [a, b] = g.edges[ei].v;
                let mut s = e.clone();
                s.faces.extend((0..g.faces.len()).filter(|f| g.faces[*f].has_edge(a, b)));
                self.apply_selection(s, i.mods);
            } else if !self.path.is_empty() {
                self.exit_context();
            }
        }
    }

    fn box_select(&mut self, a: eframe::egui::Pos2, b: eframe::egui::Pos2, mods: eframe::egui::Modifiers) {
        let crossing = b.x < a.x;
        let origin = self.viewport_min;
        let r = eframe::egui::Rect::from_two_pos(a, b);
        let to_pos = |v: DVec2| origin + eframe::egui::vec2(v.x as f32, v.y as f32);
        let cam = &self.cam;
        let seg_in = |p: DVec3, q: DVec3| -> (bool, bool) {
            // (touches, fully inside)
            match cam.project_segment(p, q) {
                Some((sp, sq)) => {
                    let (x, y) = (to_pos(sp), to_pos(sq));
                    let inside = r.contains(x) && r.contains(y);
                    (inside || crate::app::segment_hits_rect(x, y, r), inside)
                }
                None => (false, false),
            }
        };
        let mut sel = Sel::default();
        let mut inst_touch: HashSet<usize> = HashSet::new();
        let mut inst_out: HashSet<usize> = HashSet::new();
        for e in &self.cache.edges {
            let (touch, inside) = seg_in(e.a, e.b);
            match e.owner {
                Owner::Active => {
                    if (crossing && touch) || (!crossing && inside) {
                        sel.edges.insert(e.edge);
                    }
                }
                Owner::Inst(k) => {
                    if touch {
                        inst_touch.insert(k);
                    }
                    if !inside {
                        inst_out.insert(k);
                    }
                }
                Owner::Outside => {}
            }
        }
        for k in inst_touch.iter() {
            if crossing || !inst_out.contains(k) {
                sel.insts.insert(*k);
            }
        }
        let g = self.active();
        let ctx = self.ctx_xf();
        for (fi, f) in g.faces.iter().enumerate() {
            let pts: Vec<DVec3> = f.outer.iter().map(|v| ctx.transform_point3(g.p(*v))).collect();
            let n = pts.len();
            let res: Vec<(bool, bool)> = (0..n).map(|k| seg_in(pts[k], pts[(k + 1) % n])).collect();
            let hit = if crossing { res.iter().any(|x| x.0) } else { res.iter().all(|x| x.1) };
            if hit {
                sel.faces.insert(fi);
            }
        }
        self.apply_selection(sel, mods);
    }

    fn place_tool(&mut self, i: &In, def: usize) {
        let s = self.snap(i.px, None);
        let local = self.to_local(s.p);
        let mut d = self.doc.clone();
        d.geom_mut(&self.path).instances.push(Instance {
            def,
            xf: DMat4::from_translation(local),
            material: None,
            name: String::new(),
            ..Default::default()
        });
        self.preview = Some(d);
        self.preview_sel = Some(Sel::default());
        if i.pressed {
            self.checkpoint();
            let g = self.active_mut();
            g.instances.push(Instance { def, xf: DMat4::from_translation(local), material: None, name: String::new(), ..Default::default() });
            let k = g.instances.len() - 1;
            self.set_tool(Tool::Select);
            self.sel.insts.insert(k);
            self.message = format!("{} inserido.", self.doc.defs[def].name);
        }
    }

    // ------------------------------------------------------------ Measurements box

    pub(crate) fn apply_vcb(&mut self) {
        let text = std::mem::take(&mut self.vcb);
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        let bad = |s: &mut Self| s.message = format!("Valor inválido: \"{text}\"");
        if self.sandbox.is_some() && self.sandbox_vcb(&text) {
            return;
        }
        if self.tool == Tool::Tape && matches!(self.state, ToolState::Idle) {
            if let (Some(m), Some(v)) = (self.tape_last, crate::units::parse_len(&text)) {
                if v > 0.0 {
                    self.tape_resize = Some((m, v));
                    return;
                }
            }
        }
        if let Some((ei, _)) = self.divide {
            match text.parse::<usize>() {
                Ok(n) if (2..=500).contains(&n) => {
                    self.divide = Some((ei, n));
                    self.divide_typed = true;
                }
                _ => bad(self),
            }
            return;
        }
        // segment counts apply to the circle/polygon/arc tools at any time
        if let Some(n) = parse_sides(&text) {
            match self.tool {
                Tool::Circle => self.circle_sides = n,
                Tool::Polygon => self.polygon_sides = n,
                Tool::Arc => self.arc_sides = n,
                _ => return bad(self),
            }
            self.message = format!("{n} lados.");
            return;
        }
        if let Some((n, divide)) = parse_array(&text) {
            return self.apply_array(n, divide);
        }
        if self.vcb_tools2(&text) || self.vcb_annot(&text) || self.vcb_camera(&text) {
            return;
        }
        let snap_p = self.snap.map(|s| s.p);
        match self.state.clone() {
            ToolState::LineFrom(a) => {
                let Some(len) = parse_len(&text) else { return bad(self) };
                let dir = snap_p.map(|p| p - a).filter(|d| d.length() > 1e-9).map(|d| d.normalize()).unwrap_or(DVec3::X);
                self.commit_line(a, a + dir * len);
            }
            ToolState::RectFrom { p0, u, v, .. } => {
                let Some((pw, ph)) = parse_pair(&text) else { return bad(self) };
                let (cw, ch) = self.last_rect_preview.unwrap_or((1.0, 1.0));
                let w = pw.map(|x| x * cw.signum()).unwrap_or(cw);
                let h = ph.map(|x| x * ch.signum()).unwrap_or(ch);
                self.commit_rect(p0, u, v, w, h);
            }
            ToolState::CircleFrom { c, n, u } => {
                let Some(r) = parse_len(&text) else { return bad(self) };
                self.commit_circle(c, n, u, r.abs(), self.tool == Tool::Polygon);
            }
            ToolState::ArcStart(a) => {
                let Some(len) = parse_len(&text) else { return bad(self) };
                let dir = snap_p.map(|p| p - a).filter(|d| d.length() > 1e-9).map(|d| d.normalize()).unwrap_or(DVec3::X);
                self.state = ToolState::ArcChord { a, b: a + dir * len };
            }
            ToolState::ArcChord { a, b } => {
                let Some(s) = parse_len(&text) else { return bad(self) };
                let mid = (a + b) * 0.5;
                let u = (b - a).normalize();
                let w = snap_p
                    .map(|p| (p - mid) - u * (p - mid).dot(u))
                    .filter(|w| w.length() > 1e-9)
                    .map(|w| w.normalize())
                    .unwrap_or(basis_for(u).0);
                self.commit_arc(a, b, w, s);
            }
            ToolState::Pushing { face, normal, d, .. } => {
                let Some(len) = parse_len(&text) else { return bad(self) };
                let sign = if d < 0.0 { -1.0 } else { 1.0 };
                self.commit_push(face, len * sign, normal, self.copy_now());
            }
            ToolState::Offsetting { face, .. } => {
                let Some(len) = parse_len(&text) else { return bad(self) };
                let sign = if self.offset_d < 0.0 { -1.0 } else { 1.0 };
                self.commit_offset(face, len * sign);
            }
            ToolState::Moving { base, .. } => {
                let Some(len) = parse_len(&text) else { return bad(self) };
                let dir = snap_p.map(|p| p - base).filter(|d| d.length() > 1e-9).map(|d| d.normalize()).unwrap_or(DVec3::X);
                self.commit_xform(XKind::Move(dir * len));
            }
            ToolState::RotAngle { c, n, .. } => {
                let Some(a) = parse_angle(&text) else { return bad(self) };
                self.commit_xform(XKind::Rotate { c, n, ang: a.to_radians() });
            }
            ToolState::Scaling { anchor, mask, .. } => {
                let Some(t) = parse_angle(&text) else { return bad(self) };
                self.commit_xform(XKind::Scale { a: anchor, mask, t });
            }
            ToolState::TapeFrom { p, edge_dir } => {
                let Some(len) = parse_len(&text) else { return bad(self) };
                let sp = snap_p.unwrap_or(p + DVec3::X);
                self.checkpoint();
                match edge_dir {
                    Some(e) => {
                        let w = (sp - p) - e * (sp - p).dot(e);
                        let w = if w.length() > 1e-9 { w.normalize() } else { basis_for(e).0 };
                        self.doc.guides.push(Guide::Line { p: p + w * len, dir: e });
                    }
                    None => {
                        let dir = (sp - p).normalize_or(DVec3::X);
                        self.doc.guides.push(Guide::Point { p: p + dir * len });
                    }
                }
                self.state = ToolState::Idle;
            }
            _ if self.tool == Tool::Zoom => {
                let Some(a) = parse_angle(&text) else { return bad(self) };
                self.cam.fov_y = a.clamp(5.0, 150.0).to_radians();
                self.message = format!("Campo de visão: {a:.0}°");
            }
            _ => self.reapply_last(&text),
        }
    }

    /// Typing right after an operation re-applies it with the new value.
    fn reapply_last(&mut self, text: &str) {
        let bad = |s: &mut Self| s.message = format!("Valor inválido: \"{text}\"");
        let Some(last) = self.last.clone() else {
            self.message = "Nada para medir: comece uma operação primeiro.".into();
            return;
        };
        self.dirty = true;
        self.cache_dirty = true;
        match last {
            LastAction::Line { before, start, dir } => {
                let Some(len) = parse_len(text) else { return bad(self) };
                self.doc = before;
                let end = start + dir * len;
                let (a, b, eye) = (self.to_local(start), self.to_local(end), self.local_eye());
                let closed = self.active_mut().add_line(a, b, eye);
                if self.tool == Tool::Line && !closed {
                    self.state = ToolState::LineFrom(end);
                }
            }
            LastAction::Rect { before, p0, u, v, su, sv, w, h } => {
                let Some((pw, ph)) = parse_pair(text) else { return bad(self) };
                let (w, h) = (pw.unwrap_or(w), ph.unwrap_or(h));
                self.doc = before.clone();
                self.add_local_polygon(&Self::rect_corners(p0, u, v, w * su, h * sv), false, false);
                self.last = Some(LastAction::Rect { before, p0, u, v, su, sv, w, h });
            }
            LastAction::Circle { before, c, n, u, polygon, .. } => {
                let Some(r) = parse_len(text) else { return bad(self) };
                self.doc = before.clone();
                let sides = if polygon { self.polygon_sides } else { self.circle_sides };
                self.add_local_polygon(&circle_points(c, n, u, r.abs(), sides), true, !polygon);
                self.last = Some(LastAction::Circle { before, c, n, u, r, polygon });
            }
            LastAction::Arc { before, a, b, w, .. } => {
                let Some(s) = parse_len(text) else { return bad(self) };
                self.doc = before;
                self.commit_arc(a, b, w, s);
                self.undo.pop();
            }
            LastAction::PushPull { before, face, d, normal, copy } => {
                let Some(len) = parse_len(text) else { return bad(self) };
                let nd = len * if d < 0.0 { -1.0 } else { 1.0 };
                self.doc = before.clone();
                let k = self.normal_scale(normal, before.geom(&self.path).faces[face].normal);
                let eye = self.local_eye();
                self.active_mut().push_pull(face, nd * k, copy, eye);
                self.last = Some(LastAction::PushPull { before, face, d: nd, normal, copy });
                self.last_push = Some(nd);
            }
            LastAction::Offset { before, face, d } => {
                let Some(len) = parse_len(text) else { return bad(self) };
                let nd = len * if d < 0.0 { -1.0 } else { 1.0 };
                self.doc = before.clone();
                let pts = self.active().offset_loop(face, nd);
                let eye = self.local_eye();
                self.active_mut().add_polygon(&pts, None, false, eye);
                self.last = Some(LastAction::Offset { before, face, d: nd });
            }
            LastAction::Xform { before, sel, kind, copy } => {
                let kind = match kind {
                    XKind::Move(v) => {
                        let Some(len) = parse_len(text) else { return bad(self) };
                        XKind::Move(v.normalize_or(DVec3::X) * len)
                    }
                    XKind::Rotate { c, n, .. } => {
                        let Some(a) = parse_angle(text) else { return bad(self) };
                        XKind::Rotate { c, n, ang: a.to_radians() }
                    }
                    XKind::Scale { a, mask, .. } => {
                        let Some(t) = parse_angle(text) else { return bad(self) };
                        XKind::Scale { a, mask, t }
                    }
                };
                self.doc = before.clone();
                let m = self.local_matrix(kind.matrix(1.0));
                let g = self.active_mut();
                self.sel = if copy { g.copy_sel(&sel, m, true) } else { g.transform_sel(&sel, m, true) };
                self.last = Some(LastAction::Xform { before, sel, kind, copy });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arc_geometry() {
        let a = DVec3::ZERO;
        let b = DVec3::new(2.0, 0.0, 0.0);
        let pts = arc_points(a, b, DVec3::Y, 1.0, 12);
        assert_eq!(pts.len(), 13);
        // half circle of radius 1 centered at (1,0,0)
        for p in &pts {
            assert!((p.distance(DVec3::new(1.0, 0.0, 0.0)) - 1.0).abs() < 1e-9);
        }
        assert!((pts[6] - DVec3::new(1.0, 1.0, 0.0)).length() < 1e-9);
        let c = circle_points(DVec3::ZERO, DVec3::Z, DVec3::X, 2.0, 24);
        assert_eq!(c.len(), 24);
        assert!((c[6] - DVec3::new(0.0, 2.0, 0.0)).length() < 1e-9);
    }
}
