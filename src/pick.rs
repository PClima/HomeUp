//! World-space pick cache and the inference engine.

use crate::camera::Camera;
use crate::doc::{Document, Guide, Owner};
use crate::geom::Plane;
use eframe::egui::Color32;
use glam::{DVec2, DVec3};

pub struct PTri {
    pub owner: Owner,
    pub face: usize,
    pub p: [DVec3; 3],
    pub n: DVec3,
    /// Direction of the face's first edge (world), for in-plane axes.
    pub u: DVec3,
}

pub struct PEdge {
    pub owner: Owner,
    pub edge: usize,
    pub a: DVec3,
    pub b: DVec3,
}

#[derive(Default)]
pub struct PickCache {
    pub tris: Vec<PTri>,
    pub edges: Vec<PEdge>,
    pub points: Vec<(DVec3, Owner)>,
}

impl PickCache {
    pub fn build(doc: &Document, path: &[usize]) -> PickCache {
        let mut c = PickCache::default();
        doc.visit(path, &mut |it| {
            let g = it.geom;
            let nm = it.xf.inverse().transpose();
            for (fi, f) in g.faces.iter().enumerate() {
                if !doc.face_visible(f) {
                    continue;
                }
                let n = nm.transform_vector3(f.normal).normalize_or_zero();
                let u = it.xf.transform_vector3(g.p(f.outer[1]) - g.p(f.outer[0])).normalize_or_zero();
                for t in g.triangulate_face(f) {
                    c.tris.push(PTri {
                        owner: it.owner,
                        face: fi,
                        p: [it.xf.transform_point3(g.p(t[0])), it.xf.transform_point3(g.p(t[1])), it.xf.transform_point3(g.p(t[2]))],
                        n,
                        u,
                    });
                }
            }
            let mut seen = std::collections::HashSet::new();
            for (ei, e) in g.edges.iter().enumerate() {
                if e.hidden {
                    continue;
                }
                let (a, b) = (it.xf.transform_point3(g.p(e.v[0])), it.xf.transform_point3(g.p(e.v[1])));
                if !e.soft {
                    c.edges.push(PEdge { owner: it.owner, edge: ei, a, b });
                }
                for (v, p) in [(e.v[0], a), (e.v[1], b)] {
                    if seen.insert(v) {
                        c.points.push((p, it.owner));
                    }
                }
            }
        });
        c
    }

    pub fn pick_face(&self, o: DVec3, d: DVec3) -> Option<(f64, usize)> {
        let mut best: Option<(f64, usize)> = None;
        for (i, t) in self.tris.iter().enumerate() {
            if let Some(tt) = ray_tri(o, d, t.p[0], t.p[1], t.p[2]) {
                if best.is_none_or(|(bt, _)| tt < bt) {
                    best = Some((tt, i));
                }
            }
        }
        best
    }

    /// Closest visible edge within `max_px` of the cursor (index into `edges`).
    pub fn pick_edge(&self, cam: &Camera, px: DVec2, max_px: f64, filter: impl Fn(&PEdge) -> bool) -> Option<usize> {
        let (o, d) = cam.ray(px);
        let occl = self.pick_face(o, d).map(|h| h.0);
        let mut best: Option<(usize, f64)> = None;
        for (i, e) in self.edges.iter().enumerate() {
            if !filter(e) {
                continue;
            }
            let Some((sa, sb)) = cam.project_segment(e.a, e.b) else { continue };
            let (dist, _) = seg_dist(px, sa, sb);
            if dist > max_px {
                continue;
            }
            let q = e.a + (e.b - e.a) * closest_on_segment_to_ray(e.a, e.b, o, d);
            if occluded(q, occl, o, d) {
                continue;
            }
            if best.is_none_or(|(_, bd)| dist < bd) {
                best = Some((i, dist));
            }
        }
        best.map(|b| b.0)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SnapKind {
    Endpoint,
    Midpoint,
    OnEdge,
    OnFace,
    Origin,
    Axis(usize),
    Parallel,
    Perpendicular,
    PerpFace,
    FromPoint(usize),
    OnGuide,
    GuidePoint,
    Square,
    Golden,
    HalfCircle,
    Free,
}

impl SnapKind {
    pub fn label(self) -> &'static str {
        match self {
            SnapKind::Endpoint => "Extremidade",
            SnapKind::Midpoint => "Ponto médio",
            SnapKind::OnEdge => "Na aresta",
            SnapKind::OnFace => "Na face",
            SnapKind::Origin => "Origem",
            SnapKind::Axis(0) => "No eixo vermelho",
            SnapKind::Axis(1) => "No eixo verde",
            SnapKind::Axis(_) => "No eixo azul",
            SnapKind::Parallel => "Paralelo à aresta",
            SnapKind::Perpendicular => "Perpendicular à aresta",
            SnapKind::PerpFace => "Perpendicular à face",
            SnapKind::FromPoint(_) => "Do ponto",
            SnapKind::OnGuide => "Na guia",
            SnapKind::GuidePoint => "Ponto-guia",
            SnapKind::Square => "Quadrado",
            SnapKind::Golden => "Seção áurea",
            SnapKind::HalfCircle => "Meio círculo",
            SnapKind::Free => "",
        }
    }
    pub fn color(self) -> Color32 {
        match self {
            SnapKind::Endpoint => Color32::from_rgb(0, 170, 0),
            SnapKind::Midpoint => Color32::from_rgb(0, 190, 220),
            SnapKind::OnEdge => Color32::from_rgb(220, 0, 0),
            SnapKind::OnFace => Color32::from_rgb(30, 60, 230),
            SnapKind::Origin => Color32::from_rgb(230, 180, 0),
            SnapKind::Axis(i) | SnapKind::FromPoint(i) => axis_color(i),
            SnapKind::Parallel | SnapKind::Perpendicular | SnapKind::PerpFace => Color32::from_rgb(220, 0, 220),
            SnapKind::OnGuide | SnapKind::GuidePoint => Color32::from_rgb(90, 90, 90),
            SnapKind::Square | SnapKind::Golden | SnapKind::HalfCircle => Color32::from_rgb(30, 60, 230),
            SnapKind::Free => Color32::from_gray(30),
        }
    }
    pub fn is_point(self) -> bool {
        matches!(self, SnapKind::Endpoint | SnapKind::Midpoint | SnapKind::OnEdge | SnapKind::Origin | SnapKind::GuidePoint)
    }
}

pub fn axis_color(i: usize) -> Color32 {
    match i {
        0 => Color32::from_rgb(220, 0, 0),
        1 => Color32::from_rgb(0, 160, 0),
        _ => Color32::from_rgb(0, 0, 230),
    }
}

thread_local! {
    static MODEL_AXES: std::cell::Cell<([DVec3; 3], DVec3)> = const { std::cell::Cell::new(([DVec3::X, DVec3::Y, DVec3::Z], DVec3::ZERO)) };
}

/// Current model drawing axes (red, green, blue), set by the Axes tool.
pub fn axes() -> [DVec3; 3] {
    MODEL_AXES.with(|a| a.get().0)
}

pub fn axes_origin() -> DVec3 {
    MODEL_AXES.with(|a| a.get().1)
}

pub fn set_axes(ax: [DVec3; 3], origin: DVec3) {
    MODEL_AXES.with(|a| a.set((ax, origin)));
}

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub struct FaceRef {
    pub owner: Owner,
    pub face: usize,
    pub n: DVec3,
    pub u: DVec3,
}

#[derive(Clone, Copy, Debug)]
pub struct Snap {
    pub p: DVec3,
    pub kind: SnapKind,
    pub face: Option<FaceRef>,
    /// Direction from the anchor, when a linear inference applied.
    pub dir: Option<DVec3>,
    /// Point belongs to a group/component (not the active context).
    pub in_inst: bool,
    /// Dotted reference line (from-point inferences).
    pub ref_line: Option<(DVec3, DVec3)>,
}

impl Snap {
    pub fn label(&self) -> String {
        let base = self.kind.label();
        if base.is_empty() {
            String::new()
        } else if self.in_inst {
            format!("{base} (no grupo)")
        } else {
            base.to_string()
        }
    }
}

#[derive(Default)]
pub struct InferState {
    pub lock: Option<DVec3>,
    pub shift_lock: Option<(DVec3, SnapKind)>,
    pub last_edge_dir: Option<DVec3>,
    pub last_face_n: Option<DVec3>,
    pub refs: Vec<DVec3>,
}

fn axis_kind(d: DVec3) -> SnapKind {
    for (i, a) in axes().iter().enumerate() {
        if a.dot(d).abs() > 0.9999 {
            return SnapKind::Axis(i);
        }
    }
    SnapKind::Parallel
}

fn axis_aligned(d: DVec3) -> bool {
    axes().iter().any(|a| a.dot(d).abs() > 0.9999)
}

/// The inference engine.
pub fn infer(cache: &PickCache, guides: &[Guide], cam: &Camera, px: DVec2, anchor: Option<DVec3>, st: &mut InferState) -> Snap {
    let (o, dir) = cam.ray(px);
    let face_hit = cache.pick_face(o, dir);
    let occl = face_hit.map(|h| h.0);
    let near = |p: DVec3| cam.project(p).map(|s| s.distance(px));
    let mk = |p: DVec3, kind: SnapKind| Snap { p, kind, face: None, dir: None, in_inst: false, ref_line: None };
    let face_ref = face_hit.map(|(_, i)| {
        let t = &cache.tris[i];
        FaceRef { owner: t.owner, face: t.face, n: t.n, u: t.u }
    });
    if let Some(f) = face_ref {
        st.last_face_n = Some(f.n);
    }

    // point inferences
    let mut point: Option<Snap> = None;
    let mut best = 10.0;
    for (p, owner) in &cache.points {
        if let Some(dd) = near(*p) {
            if dd < best && !occluded(*p, occl, o, dir) {
                best = dd;
                point = Some(Snap { in_inst: *owner != Owner::Active, ..mk(*p, SnapKind::Endpoint) });
            }
        }
    }
    for g in guides {
        if let Guide::Point { p } = g {
            if let Some(dd) = near(*p) {
                if dd < best {
                    best = dd;
                    point = Some(mk(*p, SnapKind::GuidePoint));
                }
            }
        }
    }
    if point.is_none() {
        let mut best = 10.0;
        for e in &cache.edges {
            let p = (e.a + e.b) * 0.5;
            if let Some(dd) = near(p) {
                if dd < best && !occluded(p, occl, o, dir) {
                    best = dd;
                    point = Some(Snap { in_inst: e.owner != Owner::Active, ..mk(p, SnapKind::Midpoint) });
                }
            }
        }
    }
    let org = axes_origin();
    if point.is_none() && near(org).is_some_and(|d| d < 10.0) && !occluded(org, occl, o, dir) {
        point = Some(mk(org, SnapKind::Origin));
    }
    if let Some(ei) = cache.pick_edge(cam, px, 6.0, |_| true) {
        let e = &cache.edges[ei];
        st.last_edge_dir = Some((e.b - e.a).normalize());
        if point.is_none() {
            let t = closest_on_segment_to_ray(e.a, e.b, o, dir);
            point = Some(Snap { in_inst: e.owner != Owner::Active, ..mk(e.a + (e.b - e.a) * t, SnapKind::OnEdge) });
        }
    }
    if point.is_none() {
        for g in guides {
            if let Guide::Line { p, dir: gd } = g {
                let t = closest_line_param(*p, *gd, o, dir);
                let q = *p + *gd * t;
                if near(q).is_some_and(|d| d < 6.0) && !occluded(q, occl, o, dir) {
                    point = Some(mk(q, SnapKind::OnGuide));
                    break;
                }
            }
        }
    }
    if let Some(s) = point {
        if matches!(s.kind, SnapKind::Endpoint | SnapKind::Midpoint | SnapKind::GuidePoint)
            && st.refs.first().is_none_or(|r| r.distance(s.p) > 1e-9)
        {
            st.refs.retain(|r| r.distance(s.p) > 1e-9);
            st.refs.insert(0, s.p);
            st.refs.truncate(2);
        }
    }

    // linear inferences from the anchor
    if let Some(a) = anchor {
        let locked = st.lock.or(st.shift_lock.map(|l| l.0));
        if let Some(ld) = locked {
            let kind = st.shift_lock.filter(|_| st.lock.is_none()).map(|l| l.1).unwrap_or_else(|| axis_kind(ld));
            let q = match point {
                Some(s) => a + ld * (s.p - a).dot(ld),
                None => a + ld * closest_line_param(a, ld, o, dir),
            };
            let ref_line = point.filter(|s| s.p.distance(q) > 1e-6).map(|s| (s.p, q));
            return Snap { p: q, kind, face: None, dir: Some(ld), in_inst: false, ref_line };
        }
        if point.is_none() {
            let mut cands: Vec<(DVec3, SnapKind)> = (0..3).map(|i| (axes()[i], SnapKind::Axis(i))).collect();
            if let Some(ed) = st.last_edge_dir {
                if !axis_aligned(ed) {
                    cands.push((ed, SnapKind::Parallel));
                }
                for pd in [ed.cross(DVec3::Z), ed.cross(ed.cross(DVec3::Z))] {
                    if pd.length() > 1e-6 && !axis_aligned(pd.normalize()) {
                        cands.push((pd.normalize(), SnapKind::Perpendicular));
                    }
                }
            }
            if let Some(n) = st.last_face_n {
                if !axis_aligned(n) {
                    cands.push((n, SnapKind::PerpFace));
                }
            }
            let mut best: Option<(f64, Snap)> = None;
            for (ld, kind) in cands {
                let t = closest_line_param(a, ld, o, dir);
                let q = a + ld * t;
                if (q - a).length() < 1e-9 {
                    continue;
                }
                if let Some(dd) = near(q) {
                    if dd < 12.0 && best.is_none_or(|(bd, _)| dd < bd) {
                        best = Some((dd, Snap { dir: Some(ld), ..mk(q, kind) }));
                    }
                }
            }
            if let Some((_, mut s)) = best {
                // from-point refinement along the chosen direction
                let ld = s.dir.unwrap();
                for r in &st.refs {
                    let x = a + ld * (*r - a).dot(ld);
                    if x.distance(a) > 1e-6 && x.distance(*r) > 1e-6 {
                        if let (Some(sx), Some(sq)) = (cam.project(x), cam.project(s.p)) {
                            if sx.distance(sq) < 10.0 {
                                s.p = x;
                                s.ref_line = Some((*r, x));
                                break;
                            }
                        }
                    }
                }
                return s;
            }
        }
    }
    if let Some(s) = point {
        return s;
    }

    // base point on a face or drawing plane
    let base = if let Some((t, _)) = face_hit {
        Snap { face: face_ref, in_inst: face_ref.is_some_and(|f| f.owner != Owner::Active), ..mk(o + dir * t, SnapKind::OnFace) }
    } else {
        let z = anchor.map(|a| a.z).unwrap_or(0.0);
        let ground = Plane::new(DVec3::Z, DVec3::new(0.0, 0.0, z));
        let p = ground.intersect_ray(o, dir).filter(|p| p.distance(o) < cam.dist * 50.0).unwrap_or_else(|| {
            let b = anchor.unwrap_or(cam.target);
            Plane::new(cam.forward(), b).intersect_ray(o, dir).unwrap_or(b)
        });
        mk(p, SnapKind::Free)
    };
    // from-point alignment on the drawing plane (no anchor-linear inference applied)
    let plane = match base.face {
        Some(f) => Plane::new(f.n, base.p),
        None => Plane::new(DVec3::Z, base.p),
    };
    for r in &st.refs {
        for i in 0..3 {
            let q = *r + axes()[i] * (base.p - *r).dot(axes()[i]);
            if q.distance(*r) < 1e-6 || !plane.contains(q) {
                continue;
            }
            if let (Some(sq), Some(sb)) = (cam.project(q), cam.project(base.p)) {
                if sq.distance(sb) < 8.0 {
                    return Snap { p: q, kind: SnapKind::FromPoint(i), ref_line: Some((*r, q)), ..base };
                }
            }
        }
    }
    base
}

// ------------------------------------------------------------------ math helpers

pub fn occluded(p: DVec3, occl: Option<f64>, o: DVec3, dir: DVec3) -> bool {
    match occl {
        Some(t) => (p - o).dot(dir) > t * 1.01 + 2e-3,
        None => false,
    }
}

pub fn ray_tri(o: DVec3, d: DVec3, a: DVec3, b: DVec3, c: DVec3) -> Option<f64> {
    let (e1, e2) = (b - a, c - a);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 1e-9).then_some(t)
}

/// Distance from p to segment ab (2D) and the segment parameter.
pub fn seg_dist(p: DVec2, a: DVec2, b: DVec2) -> (f64, f64) {
    let ab = b - a;
    let l2 = ab.length_squared();
    let t = if l2 < 1e-12 { 0.0 } else { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) };
    ((a + ab * t).distance(p), t)
}

/// Parameter t on line (a + t*dir) closest to ray (o + s*rd).
pub fn closest_line_param(a: DVec3, dir: DVec3, o: DVec3, rd: DVec3) -> f64 {
    let w = a - o;
    let (b, d, e) = (dir.dot(rd), dir.dot(w), rd.dot(w));
    let denom = dir.dot(dir) - b * b;
    if denom.abs() < 1e-9 {
        return 0.0;
    }
    (b * e - d) / denom
}

pub fn closest_on_segment_to_ray(a: DVec3, b: DVec3, o: DVec3, rd: DVec3) -> f64 {
    let len = (b - a).length();
    if len < 1e-12 {
        return 0.0;
    }
    (closest_line_param(a, (b - a) / len, o, rd) / len).clamp(0.0, 1.0)
}
