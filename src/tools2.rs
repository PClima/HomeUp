//! Additional drawing tools: arc by center, 3-point arc, pie, freehand,
//! rotated rectangle, lasso selection and the sample (eyedropper) tool.

use crate::app::{HomeUpApp, In, Tool, ToolState};
use crate::doc::Owner;
use crate::geom::{Plane, Sel};
use crate::pick::{axis_color, Snap, SnapKind, axes};
use crate::units::{format_len, parse_angle, parse_len};
use eframe::egui::{Color32, Pos2};
use glam::{DMat4, DVec2, DVec3};

const DARK: Color32 = Color32::from_rgb(20, 20, 20);

/// Points of an arc around `c` (normal `n`), starting at `a`, sweeping `sweep` radians.
pub fn sweep_points(c: DVec3, n: DVec3, a: DVec3, sweep: f64, sides: usize) -> Vec<DVec3> {
    let n = n.normalize();
    // like SketchUp, an arc uses its segment count whatever its angle
    let steps = sides.max(2);
    (0..=steps)
        .map(|k| {
            let ang = sweep * k as f64 / steps as f64;
            c + DMat4::from_axis_angle(n, ang).transform_vector3(a - c)
        })
        .collect()
}

/// Circle through three points: (center, normal). None when collinear.
pub fn circumcenter(a: DVec3, b: DVec3, c: DVec3) -> Option<(DVec3, DVec3)> {
    let (ab, ac) = (b - a, c - a);
    let n = ab.cross(ac);
    let n2 = n.length_squared();
    if n2 < 1e-18 {
        return None;
    }
    let center = a + (n.cross(ab) * ac.length_squared() + ac.cross(n) * ab.length_squared()) / (2.0 * n2);
    Some((center, n.normalize()))
}

/// Arc from `a` through `m` to `e`: (center, normal, sweep).
pub fn arc3(a: DVec3, m: DVec3, e: DVec3) -> Option<(DVec3, DVec3, f64)> {
    let (c, n) = circumcenter(a, m, e)?;
    let x = (a - c).normalize();
    let y = n.cross(x);
    let ang = |p: DVec3| {
        let v = p - c;
        let t = v.dot(y).atan2(v.dot(x));
        if t < 0.0 { t + std::f64::consts::TAU } else { t }
    };
    let (am, ae) = (ang(m), ang(e));
    let sweep = if am <= ae { ae } else { ae - std::f64::consts::TAU };
    Some((c, n, sweep))
}

/// Signed angle from `from` to `to` around `n`, in (-π, π].
fn signed_angle(from: DVec3, to: DVec3, n: DVec3) -> f64 {
    from.cross(to).dot(n).atan2(from.dot(to))
}

impl HomeUpApp {
    fn add_open_curve(&mut self, pts: &[DVec3], arc: bool) {
        let local: Vec<DVec3> = pts.iter().map(|p| self.to_local(*p)).collect();
        let eye = self.local_eye();
        let g = self.active_mut();
        let c = g.new_curve();
        let segs: Vec<(DVec3, DVec3)> = local.windows(2).map(|w| (w[0], w[1])).collect();
        g.add_segments(&segs, Some(c), arc, eye);
    }

    fn plane_normal_at(&self, s: &Snap) -> (DVec3, DVec3) {
        if let Some(i) = self.plane_lock {
            let n = axes()[i];
            return (n, crate::geom::basis_for(n).0);
        }
        if let Some(f) = s.face {
            let u = axes().iter().copied().find(|a| a.dot(f.n).abs() < 1e-6).unwrap_or(f.u);
            return (f.n, (u - f.n * f.n.dot(u)).normalize());
        }
        (DVec3::Z, DVec3::X)
    }

    fn point_on(&self, plane: &Plane, s: &Snap, px: DVec2) -> DVec3 {
        if matches!(s.kind, SnapKind::Free | SnapKind::OnFace) {
            let (o, d) = self.cam.ray(px);
            plane.intersect_ray(o, d).unwrap_or(plane.project(s.p))
        } else {
            plane.project(s.p)
        }
    }

    fn preview_polyline(&mut self, pts: &[DVec3], closed: bool) {
        for w in pts.windows(2) {
            self.overlay.lines.push((w[0], w[1], DARK, 2.0));
        }
        if closed && pts.len() > 2 {
            self.overlay.lines.push((*pts.last().unwrap(), pts[0], DARK, 2.0));
        }
    }

    // ---------------------------------------------------------------- arc by center & pie

    pub(crate) fn commit_center_arc(&mut self, c: DVec3, n: DVec3, a: DVec3, sweep: f64, pie: bool) {
        if sweep.abs() < 1e-6 || a.distance(c) < 1e-6 {
            return;
        }
        let pts = sweep_points(c, n, a, sweep, self.arc_sides);
        self.checkpoint();
        self.add_open_curve(&pts, true);
        if pie {
            let (l0, l1, lc) = (self.to_local(pts[0]), self.to_local(*pts.last().unwrap()), self.to_local(c));
            let eye = self.local_eye();
            let full = sweep.abs() >= std::f64::consts::TAU - 1e-9;
            if full {
                self.active_mut().add_line(lc, l0, eye);
            } else {
                self.active_mut().add_segments(&[(l1, lc), (lc, l0)], None, false, eye);
            }
        }
        self.state = ToolState::Idle;
    }

    pub(crate) fn center_arc_tool(&mut self, i: &In, pie: bool) {
        match self.state.clone() {
            ToolState::ArcCenterR { c, n } => {
                let s = self.snap(i.px, Some(c));
                let plane = Plane::new(n, c);
                let p = self.point_on(&plane, &s, i.px);
                self.overlay.lines.push((c, p, Self::line_color_pub(s.kind), 1.5));
                let r = c.distance(p);
                let pts = crate::tools::circle_points(c, n, crate::geom::basis_for(n).0, r, 48);
                for k in 0..pts.len() {
                    self.overlay.dashed.push((pts[k], pts[(k + 1) % pts.len()], Color32::from_gray(120)));
                }
                self.live_measure = Some(format_len(r));
                if (i.pressed || (i.released && i.dragging_far)) && r > 1e-6 {
                    self.state = ToolState::ArcSweep { c, n, a: p };
                }
            }
            ToolState::ArcSweep { c, n, a } => {
                let s = self.snap(i.px, Some(c));
                let plane = Plane::new(n, c);
                let p = self.point_on(&plane, &s, i.px);
                let mut sweep = signed_angle(a - c, p - c, n);
                if let ToolState::ArcSweep { .. } = self.state {
                    // keep a continuous sweep beyond 180° in the drag direction
                    let prev = self.arc_sweep;
                    while sweep - prev > std::f64::consts::PI {
                        sweep -= std::f64::consts::TAU;
                    }
                    while prev - sweep > std::f64::consts::PI {
                        sweep += std::f64::consts::TAU;
                    }
                    sweep = sweep.clamp(-std::f64::consts::TAU, std::f64::consts::TAU);
                }
                let step = 15f64.to_radians();
                let snapped = (sweep / step).round() * step;
                if (sweep - snapped).abs() < 1.5f64.to_radians() {
                    sweep = snapped;
                }
                self.arc_sweep = sweep;
                let pts = sweep_points(c, n, a, sweep, self.arc_sides);
                self.preview_polyline(&pts, false);
                if pie {
                    self.overlay.lines.push((c, a, DARK, 2.0));
                    self.overlay.lines.push((c, *pts.last().unwrap(), DARK, 2.0));
                }
                self.live_measure = Some(format!("{:.1}°", sweep.to_degrees()).replace('.', ","));
                if i.pressed || (i.released && i.dragging_far) {
                    self.commit_center_arc(c, n, a, sweep, pie);
                }
            }
            _ => {
                let s = self.snap(i.px, None);
                let (n, u) = self.plane_normal_at(&s);
                let r = self.cam.world_per_px(self.cam.depth(s.p)) * 25.0;
                let pts = crate::tools::circle_points(s.p, n, u, r, 24);
                let col = axis_color(axes().iter().position(|a| a.dot(n).abs() > 0.999).unwrap_or(2));
                for k in 0..pts.len() {
                    self.overlay.lines.push((pts[k], pts[(k + 1) % pts.len()], col, 1.0));
                }
                if i.pressed {
                    self.arc_sweep = 0.0;
                    self.state = ToolState::ArcCenterR { c: Plane::new(n, s.p).project(s.p), n };
                }
            }
        }
    }

    // ---------------------------------------------------------------- 3-point arc

    pub(crate) fn arc3_tool(&mut self, i: &In) {
        match self.state.clone() {
            ToolState::Arc3Start(a) => {
                let s = self.snap(i.px, Some(a));
                self.overlay.lines.push((a, s.p, Self::line_color_pub(s.kind), 1.5));
                if self.finishes_pub(i) && s.p.distance(a) > 1e-6 {
                    self.state = ToolState::Arc3Mid { a, m: s.p };
                }
            }
            ToolState::Arc3Mid { a, m } => {
                let s = self.snap(i.px, Some(m));
                if let Some((c, n, sweep)) = arc3(a, m, s.p) {
                    let pts = sweep_points(c, n, a, sweep, self.arc_sides);
                    self.preview_polyline(&pts, false);
                    self.live_measure = Some(format!("{:.1}°", sweep.to_degrees()).replace('.', ","));
                    if self.finishes_pub(i) {
                        self.checkpoint();
                        self.add_open_curve(&pts, true);
                        self.state = ToolState::Idle;
                    }
                } else {
                    self.overlay.lines.push((a, s.p, DARK, 1.5));
                }
            }
            _ => {
                let s = self.snap(i.px, None);
                if i.pressed {
                    self.state = ToolState::Arc3Start(s.p);
                }
            }
        }
    }

    // ---------------------------------------------------------------- freehand

    pub(crate) fn freehand_tool(&mut self, i: &In) {
        match self.state.clone() {
            ToolState::Freehand { mut pts, plane } => {
                let s = self.snap(i.px, None);
                let p = self.point_on(&plane, &s, i.px);
                let min = self.cam.world_per_px(self.cam.depth(p)) * 6.0;
                if pts.last().is_none_or(|l| l.distance(p) > min) {
                    pts.push(p);
                }
                let close = pts.len() > 3 && self.cam.project(pts[0]).zip(self.cam.project(p)).is_some_and(|(a, b)| a.distance(b) < 10.0);
                self.preview_polyline(&pts, false);
                if close {
                    self.overlay.tooltip = Some(("Fechar a curva".into(), SnapKind::Endpoint.color()));
                }
                if i.released || !i.primary_down {
                    if pts.len() >= 2 {
                        self.checkpoint();
                        if close {
                            pts.pop();
                            let local: Vec<DVec3> = pts.iter().map(|q| self.to_local(*q)).collect();
                            let eye = self.local_eye();
                            let g = self.active_mut();
                            let c = g.new_curve();
                            g.add_polygon(&local, Some(c), false, eye);
                        } else {
                            self.add_open_curve(&pts, false);
                        }
                    }
                    self.state = ToolState::Idle;
                } else {
                    self.state = ToolState::Freehand { pts, plane };
                }
            }
            _ => {
                let s = self.snap(i.px, None);
                if i.pressed {
                    let (n, _) = self.plane_normal_at(&s);
                    self.state = ToolState::Freehand { pts: vec![s.p], plane: Plane::new(n, s.p) };
                }
            }
        }
    }

    // ---------------------------------------------------------------- rotated rectangle

    pub(crate) fn commit_rot_rect(&mut self, p0: DVec3, p1: DVec3, n: DVec3, w: f64) {
        let u = p1 - p0;
        if u.length() < 1e-6 || w.abs() < 1e-6 {
            return;
        }
        let v = n.cross(u).normalize() * w;
        self.checkpoint();
        self.add_local_polygon(&[p0, p1, p1 + v, p0 + v], false, false);
        self.state = ToolState::Idle;
    }

    pub(crate) fn rot_rect_tool(&mut self, i: &In) {
        match self.state.clone() {
            ToolState::RotRect1 { p0, n } => {
                let s = self.snap(i.px, Some(p0));
                let plane = Plane::new(n, p0);
                let p = self.point_on(&plane, &s, i.px);
                self.overlay.lines.push((p0, p, Self::line_color_pub(s.kind), 2.0));
                self.live_measure = Some(format_len(p0.distance(p)));
                if self.finishes_pub(i) && p.distance(p0) > 1e-6 {
                    self.state = ToolState::RotRect2 { p0, p1: p, n };
                }
            }
            ToolState::RotRect2 { p0, p1, n } => {
                let s = self.snap(i.px, None);
                let plane = Plane::new(n, p0);
                let p = self.point_on(&plane, &s, i.px);
                let vdir = n.cross(p1 - p0).normalize();
                let w = (p - p0).dot(vdir);
                let c = [p0, p1, p1 + vdir * w, p0 + vdir * w];
                self.preview_polyline(&c, true);
                self.live_measure = Some(format!("{} ; {}", format_len(p0.distance(p1)), format_len(w.abs())));
                self.rot_rect_w = w;
                if self.finishes_pub(i) {
                    self.commit_rot_rect(p0, p1, n, w);
                }
            }
            _ => {
                let s = self.snap(i.px, None);
                if i.pressed {
                    let (n, _) = self.plane_normal_at(&s);
                    self.state = ToolState::RotRect1 { p0: Plane::new(n, s.p).project(s.p), n };
                }
            }
        }
    }

    // ---------------------------------------------------------------- lasso & sample

    pub(crate) fn lasso_tool(&mut self, i: &In) {
        self.snap = None;
        match self.state.clone() {
            ToolState::Lasso { mut pts } => {
                if pts.last().is_none_or(|l| l.distance(i.pos) > 3.0) {
                    pts.push(i.pos);
                }
                if i.released || !i.primary_down {
                    self.state = ToolState::Idle;
                    if pts.len() >= 3 {
                        self.lasso_select(&pts, i.mods);
                    }
                } else {
                    self.overlay.screen_poly = pts.clone();
                    self.state = ToolState::Lasso { pts };
                }
            }
            _ => {
                if i.pressed {
                    self.state = ToolState::Lasso { pts: vec![i.pos] };
                }
            }
        }
    }

    fn lasso_select(&mut self, screen: &[Pos2], mods: eframe::egui::Modifiers) {
        let origin = self.viewport_min;
        let poly: Vec<DVec2> = screen.iter().map(|p| DVec2::new((p.x - origin.x) as f64, (p.y - origin.y) as f64)).collect();
        // screen y points down: a negative signed area means clockwise on screen
        let clockwise = crate::geom::signed_area(&poly) < 0.0;
        let crossing = clockwise;
        let inside = |p: DVec3| self.cam.project(p).is_some_and(|s| crate::geom::point_in_poly(s, &poly));
        let mut sel = Sel::default();
        let mut inst_any = std::collections::HashSet::new();
        let mut inst_out = std::collections::HashSet::new();
        for e in &self.cache.edges {
            let (a, b) = (inside(e.a), inside(e.b));
            match e.owner {
                Owner::Active => {
                    if (crossing && (a || b)) || (a && b) {
                        sel.edges.insert(e.edge);
                    }
                }
                Owner::Inst(k) => {
                    if a || b {
                        inst_any.insert(k);
                    }
                    if !(a && b) {
                        inst_out.insert(k);
                    }
                }
                Owner::Outside => {}
            }
        }
        for k in inst_any {
            if crossing || !inst_out.contains(&k) {
                sel.insts.insert(k);
            }
        }
        let g = self.active();
        let ctx = self.ctx_xf();
        for (fi, f) in g.faces.iter().enumerate() {
            let ins: Vec<bool> = f.outer.iter().map(|v| inside(ctx.transform_point3(g.p(*v)))).collect();
            if (crossing && ins.iter().any(|x| *x)) || ins.iter().all(|x| *x) {
                sel.faces.insert(fi);
            }
        }
        self.apply_selection(sel, mods);
    }

    pub(crate) fn sample_tool(&mut self, i: &In) {
        self.snap = None;
        let (o, d) = self.cam.ray(i.px);
        let hit = self.cache.pick_face(o, d).map(|h| (self.cache.tris[h.1].owner, self.cache.tris[h.1].face));
        if !i.pressed {
            return;
        }
        let m = match hit {
            Some((Owner::Active, f)) => self.active().faces[f].material,
            Some((Owner::Inst(k), _)) => self.active().instances[k].material,
            _ => return,
        };
        self.set_tool(Tool::Paint);
        self.cur_mat = m;
        self.message = format!("Material de amostra: {}", m.map(|k| self.doc.materials[k].name.clone()).unwrap_or("Padrão".into()));
    }

    /// Measurements box for the new tools. Returns false when not handled.
    pub(crate) fn vcb_tools2(&mut self, text: &str) -> bool {
        match self.state.clone() {
            ToolState::ArcCenterR { c, n } => {
                let Some(r) = parse_len(text) else { return false };
                let dir = self.snap.map(|s| Plane::new(n, c).project(s.p) - c).filter(|d| d.length() > 1e-9).map(|d| d.normalize()).unwrap_or(crate::geom::basis_for(n).0);
                self.arc_sweep = 0.0;
                self.state = ToolState::ArcSweep { c, n, a: c + dir * r.abs() };
                true
            }
            ToolState::ArcSweep { c, n, a } => {
                let Some(deg) = parse_angle(text) else { return false };
                let sign = if self.arc_sweep < 0.0 { -1.0 } else { 1.0 };
                self.commit_center_arc(c, n, a, deg.to_radians() * sign, self.tool == Tool::Pie);
                true
            }
            ToolState::RotRect1 { p0, n } => {
                let Some(l) = parse_len(text) else { return false };
                let dir = self.snap.map(|s| Plane::new(n, p0).project(s.p) - p0).filter(|d| d.length() > 1e-9).map(|d| d.normalize()).unwrap_or(crate::geom::basis_for(n).0);
                self.state = ToolState::RotRect2 { p0, p1: p0 + dir * l, n };
                true
            }
            ToolState::RotRect2 { p0, p1, n } => {
                let Some(w) = parse_len(text) else { return false };
                let sign = if self.rot_rect_w < 0.0 { -1.0 } else { 1.0 };
                self.commit_rot_rect(p0, p1, n, w * sign);
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn three_point_arc_and_sweep() {
        let (a, m, e) = (DVec3::new(1.0, 0.0, 0.0), DVec3::new(0.0, 1.0, 0.0), DVec3::new(-1.0, 0.0, 0.0));
        let (c, n, sweep) = arc3(a, m, e).unwrap();
        assert!(c.length() < 1e-9);
        assert!((sweep.abs() - std::f64::consts::PI).abs() < 1e-9);
        let pts = sweep_points(c, n, a, sweep, 12);
        assert!((pts.last().unwrap().distance(e)) < 1e-9);
        // the middle point lies on the arc
        assert!(pts.iter().any(|p| p.distance(m) < 1e-9));
        assert!(circumcenter(a, a * 2.0, a * 3.0).is_none());
    }
}
