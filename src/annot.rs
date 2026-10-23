//! Annotation and construction: Dimensions, Text, 3D Text, Protractor, Axes
//! and Section Planes (cut lines + fill).

use crate::app::{HomeUpApp, In, Tool, ToolState};
use crate::doc::{Definition, Dimension, Document, Guide, Label, ModelAxes, Owner, SectionPlane};
use crate::geom::{basis_for, point_in_poly, signed_area, triangulate_2d, Edge, Face, Geom, Plane, VId};
use crate::pick::{axes, axis_color, seg_dist, SnapKind};
use crate::units::{format_len, parse_angle};
use eframe::egui::{self, Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, Vec2};
use glam::{DMat4, DVec2, DVec3};

const DIM_COLOR: Color32 = Color32::from_rgb(30, 30, 30);
const SECTION_COLOR: Color32 = Color32::from_rgb(230, 120, 0);

#[derive(Clone, Debug)]
pub struct Text3dDialog {
    pub text: String,
    pub height: f64,
    pub depth: f64,
    pub extrude: bool,
}

impl Default for Text3dDialog {
    fn default() -> Self {
        Text3dDialog { text: "HomeUp".into(), height: 0.5, depth: 0.1, extrude: true }
    }
}

// ---------------------------------------------------------------- 3D text geometry

struct Outline {
    loops: Vec<Vec<DVec2>>,
    cur: Vec<DVec2>,
    scale: f64,
    dx: f64,
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.flush();
        self.cur.push(self.pt(x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.cur.push(self.pt(x, y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let p0 = *self.cur.last().unwrap_or(&DVec2::ZERO);
        let (p1, p2) = (self.pt(x1, y1), self.pt(x, y));
        for k in 1..=6 {
            let t = k as f64 / 6.0;
            self.cur.push(p0 * (1.0 - t) * (1.0 - t) + p1 * 2.0 * t * (1.0 - t) + p2 * t * t);
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let p0 = *self.cur.last().unwrap_or(&DVec2::ZERO);
        let (p1, p2, p3) = (self.pt(x1, y1), self.pt(x2, y2), self.pt(x, y));
        for k in 1..=8 {
            let t = k as f64 / 8.0;
            let u = 1.0 - t;
            self.cur.push(p0 * u * u * u + p1 * 3.0 * u * u * t + p2 * 3.0 * u * t * t + p3 * t * t * t);
        }
    }
    fn close(&mut self) {
        self.flush();
    }
}

impl Outline {
    fn pt(&self, x: f32, y: f32) -> DVec2 {
        DVec2::new(x as f64 * self.scale + self.dx, y as f64 * self.scale)
    }
    fn flush(&mut self) {
        let mut l = std::mem::take(&mut self.cur);
        l.dedup_by(|a, b| a.distance(*b) < 1e-9);
        if l.len() > 1 && l[0].distance(*l.last().unwrap()) < 1e-9 {
            l.pop();
        }
        if l.len() >= 3 {
            self.loops.push(l);
        }
    }
}

/// Flat (or extruded) 3D text lying on the XY plane, starting at the origin.
pub fn text_geometry(o: &Text3dDialog) -> Result<Geom, String> {
    let face = ttf_parser::Face::parse(epaint_default_fonts::UBUNTU_LIGHT, 0).map_err(|e| e.to_string())?;
    let cap = face.capital_height().map(|c| c as f64).unwrap_or(face.units_per_em() as f64 * 0.7);
    let scale = o.height / cap;
    let mut g = Geom::default();
    let mut x = 0.0;
    for ch in o.text.chars() {
        let Some(id) = face.glyph_index(ch) else { continue };
        let mut ol = Outline { loops: Vec::new(), cur: Vec::new(), scale, dx: x };
        face.outline_glyph(id, &mut ol);
        ol.flush();
        x += face.glyph_hor_advance(id).unwrap_or(0) as f64 * scale;
        // classify loops: a loop inside an odd number of others is a hole
        let loops = ol.loops;
        let depth: Vec<usize> = (0..loops.len())
            .map(|i| (0..loops.len()).filter(|j| *j != i && point_in_poly(loops[i][0], &loops[*j])).count())
            .collect();
        for i in (0..loops.len()).filter(|i| depth[*i] % 2 == 0) {
            let holes: Vec<usize> = (0..loops.len())
                .filter(|j| depth[*j] == depth[i] + 1 && point_in_poly(loops[*j][0], &loops[i]))
                .collect();
            let add_loop = |pts: &[DVec2], ccw: bool, g: &mut Geom| -> Vec<VId> {
                let mut p: Vec<DVec2> = pts.to_vec();
                if (signed_area(&p) > 0.0) != ccw {
                    p.reverse();
                }
                let ids: Vec<VId> = p
                    .iter()
                    .map(|q| {
                        g.verts.push(DVec3::new(q.x, q.y, 0.0));
                        (g.verts.len() - 1) as VId
                    })
                    .collect();
                let c = g.new_curve();
                for k in 0..ids.len() {
                    let mut e = Edge::new(ids[k], ids[(k + 1) % ids.len()]);
                    e.curve = Some(c);
                    e.arc = true;
                    g.edges.push(e);
                }
                ids
            };
            let outer = add_loop(&loops[i], true, &mut g);
            let hole_ids: Vec<Vec<VId>> = holes.iter().map(|h| add_loop(&loops[*h], false, &mut g)).collect();
            g.faces.push(Face { outer, holes: hole_ids, normal: DVec3::Z, material: None, ..Default::default() });
        }
    }
    if g.faces.is_empty() {
        return Err("Nenhum caractere suportado.".into());
    }
    if o.extrude && o.depth.abs() > 1e-6 {
        let n = g.faces.len();
        let eye = DVec3::new(0.0, -10.0, 10.0);
        for _ in 0..n {
            g.push_pull(0, o.depth, false, eye);
        }
    }
    Ok(g)
}

// ---------------------------------------------------------------- section cut

/// Cut segments of every face with the plane (n·x = d), world coordinates.
/// Section segments per object (each group/component instance and the loose
/// geometry), with the object's material for its hatch.
pub fn section_segments_grouped(doc: &Document, path: &[usize], plane: &Plane) -> Vec<(Option<usize>, Vec<(DVec3, DVec3)>)> {
    let mut groups = Vec::new();
    doc.visit(path, &mut |it| {
        let mut out = Vec::new();
        let g = it.geom;
        let mat = it.mat.or_else(|| g.faces.iter().find_map(|f| f.material));
        let nm = it.xf.inverse().transpose();
        for f in &g.faces {
            let pts: Vec<Vec<DVec3>> = f.loops().map(|l| l.iter().map(|v| it.xf.transform_point3(g.p(*v))).collect()).collect();
            let fnrm = nm.transform_vector3(f.normal).normalize_or_zero();
            let dir = fnrm.cross(plane.n);
            if dir.length() < 1e-9 {
                continue;
            }
            // crossings of the face loops with the plane, sorted along `dir`
            let mut ts: Vec<(f64, DVec3)> = Vec::new();
            for l in &pts {
                for k in 0..l.len() {
                    let (a, b) = (l[k], l[(k + 1) % l.len()]);
                    let (da, db) = (plane.dist(a), plane.dist(b));
                    if (da > 0.0) != (db > 0.0) {
                        let x = a + (b - a) * (da / (da - db));
                        ts.push((x.dot(dir), x));
                    }
                }
            }
            ts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            for c in ts.chunks_exact(2) {
                if c[0].1.distance(c[1].1) > 1e-9 {
                    out.push((c[0].1, c[1].1));
                }
            }
        }
        if !out.is_empty() {
            groups.push((mat, out));
        }
    });
    groups
}

/// Chain segments into closed loops and triangulate them (section fill).
pub fn section_fill(segs: &[(DVec3, DVec3)], plane: &Plane) -> Vec<[DVec3; 3]> {
    let (u, v) = plane.basis();
    let key = |p: DVec3| ((p.x * 1e5).round() as i64, (p.y * 1e5).round() as i64, (p.z * 1e5).round() as i64);
    let mut adj: std::collections::HashMap<(i64, i64, i64), Vec<usize>> = std::collections::HashMap::new();
    for (i, (a, b)) in segs.iter().enumerate() {
        adj.entry(key(*a)).or_default().push(i);
        adj.entry(key(*b)).or_default().push(i);
    }
    let mut used = vec![false; segs.len()];
    let mut loops: Vec<Vec<DVec2>> = Vec::new();
    for s in 0..segs.len() {
        if used[s] {
            continue;
        }
        used[s] = true;
        let start = segs[s].0;
        let mut cur = segs[s].1;
        let mut l = vec![start];
        for _ in 0..segs.len() {
            if key(cur) == key(start) {
                break;
            }
            l.push(cur);
            let Some(&n) = adj.get(&key(cur)).and_then(|c| c.iter().find(|i| !used[**i])) else { break };
            used[n] = true;
            cur = if key(segs[n].0) == key(cur) { segs[n].1 } else { segs[n].0 };
        }
        if l.len() >= 3 && key(cur) == key(start) {
            loops.push(l.iter().map(|p| DVec2::new(p.dot(u), p.dot(v))).collect());
        }
    }
    let depth: Vec<usize> =
        (0..loops.len()).map(|i| (0..loops.len()).filter(|j| *j != i && point_in_poly(loops[i][0], &loops[*j])).count()).collect();
    let to3 = |p: DVec2| u * p.x + v * p.y + plane.n * plane.d;
    let mut tris = Vec::new();
    for i in (0..loops.len()).filter(|i| depth[*i] % 2 == 0) {
        let holes: Vec<Vec<DVec2>> = (0..loops.len())
            .filter(|j| depth[*j] == depth[i] + 1 && point_in_poly(loops[*j][0], &loops[i]))
            .map(|j| loops[j].clone())
            .collect();
        let all: Vec<DVec2> = loops[i].iter().chain(holes.iter().flatten()).copied().collect();
        for t in triangulate_2d(&loops[i], &holes) {
            tris.push([to3(all[t[0]]), to3(all[t[1]]), to3(all[t[2]])]);
        }
    }
    tris
}

/// Active section plane (if any) as a world plane.
pub fn active_section(doc: &Document) -> Option<Plane> {
    doc.sections.iter().find(|s| s.active).map(|s| {
        let n = s.n.normalize();
        Plane { n, d: n.dot(s.p) }
    })
}

// ---------------------------------------------------------------- tools

impl HomeUpApp {
    fn default_label(&self, s: &crate::pick::Snap) -> String {
        if let Some(f) = s.face.filter(|f| f.owner == Owner::Active) {
            let g = self.active();
            return format!("{} m²", format!("{:.2}", g.face_area(&g.faces[f.face])).replace('.', ","));
        }
        if s.kind == SnapKind::OnEdge {
            if let Some(ei) = self.cache.pick_edge(&self.cam, self.last_px, 7.0, |_| true) {
                let e = &self.cache.edges[ei];
                return format_len(e.a.distance(e.b));
            }
        }
        let p = s.p;
        format!("({}; {}; {})", format_len(p.x), format_len(p.y), format_len(p.z))
    }

    pub(crate) fn dimension_tool(&mut self, i: &In) {
        match self.state.clone() {
            ToolState::DimFrom(a) => {
                let s = self.snap(i.px, Some(a));
                self.overlay.lines.push((a, s.p, Self::line_color_pub(s.kind), 1.5));
                self.live_measure = Some(format_len(a.distance(s.p)));
                if self.finishes_pub(i) && s.p.distance(a) > 1e-6 {
                    self.state = ToolState::DimOffset { a, b: s.p };
                }
            }
            ToolState::DimOffset { a, b } => {
                let s = self.snap(i.px, None);
                let d = (b - a).normalize();
                let w = (s.p - a) - d * (s.p - a).dot(d);
                self.preview_dim = Some(Dimension { a, b, off: w });
                self.live_measure = Some(format_len(a.distance(b)));
                if self.finishes_pub(i) {
                    self.checkpoint_keep_sel();
                    self.doc.dims.push(Dimension { a, b, off: w });
                    self.state = ToolState::Idle;
                }
            }
            _ => {
                let s = self.snap(i.px, None);
                if i.pressed {
                    if s.kind == SnapKind::OnEdge {
                        if let Some(ei) = self.cache.pick_edge(&self.cam, i.px, 7.0, |_| true) {
                            let e = &self.cache.edges[ei];
                            self.state = ToolState::DimOffset { a: e.a, b: e.b };
                            return;
                        }
                    }
                    self.state = ToolState::DimFrom(s.p);
                }
            }
        }
    }

    pub(crate) fn text_tool(&mut self, i: &In) {
        self.last_px = i.px;
        match self.state.clone() {
            ToolState::TextLeader { anchor, text } => {
                let s = self.snap(i.px, None);
                self.overlay.lines.push((anchor, s.p, DIM_COLOR, 1.0));
                if self.finishes_pub(i) {
                    self.checkpoint_keep_sel();
                    self.doc.labels.push(Label { anchor, pos: s.p, text });
                    self.edit_label = Some(self.doc.labels.len() - 1);
                    self.state = ToolState::Idle;
                }
            }
            _ => {
                let s = self.snap(i.px, None);
                if i.dbl {
                    if let Some(k) = self.label_at(i.px) {
                        self.edit_label = Some(k);
                        return;
                    }
                }
                if i.pressed && self.label_at(i.px).is_none() {
                    let text = self.default_label(&s);
                    self.state = ToolState::TextLeader { anchor: s.p, text };
                }
            }
        }
    }

    pub(crate) fn protractor_tool(&mut self, i: &In) {
        match self.state.clone() {
            ToolState::ProtRef { c, n } => {
                let s = self.snap(i.px, Some(c));
                let q = s.p - n * (s.p - c).dot(n);
                self.protractor_pub(c, n);
                self.overlay.lines.push((c, q, DIM_COLOR, 1.5));
                if self.finishes_pub(i) && q.distance(c) > 1e-6 {
                    self.state = ToolState::ProtAngle { c, n, r0: (q - c).normalize() };
                }
            }
            ToolState::ProtAngle { c, n, r0 } => {
                let s = self.snap(i.px, Some(c));
                let q = s.p - n * (s.p - c).dot(n);
                let r = (q - c).normalize_or_zero();
                let mut ang = r0.cross(r).dot(n).atan2(r0.dot(r));
                let step = 15f64.to_radians();
                let snapped = (ang / step).round() * step;
                if (ang - snapped).abs() < 1.5f64.to_radians() {
                    ang = snapped;
                }
                self.protractor_pub(c, n);
                let dir = DMat4::from_axis_angle(n, ang).transform_vector3(r0);
                let l = self.cam.dist * 300.0;
                self.overlay.dashed.push((c - dir * l, c + dir * l, Color32::from_gray(60)));
                self.arc_sweep = ang;
                self.live_measure = Some(format!("{:.1}°", ang.to_degrees()).replace('.', ","));
                if self.finishes_pub(i) {
                    self.checkpoint_keep_sel();
                    self.doc.guides.push(Guide::Line { p: c, dir });
                    self.state = ToolState::Idle;
                }
            }
            _ => {
                let s = self.snap(i.px, None);
                let n = self.rot_normal_pub(&s);
                self.protractor_pub(s.p, n);
                if i.pressed {
                    self.state = ToolState::ProtRef { c: s.p, n };
                }
            }
        }
    }

    pub(crate) fn axes_tool(&mut self, i: &In) {
        match self.state.clone() {
            ToolState::AxesX { o } => {
                let s = self.snap(i.px, Some(o));
                self.overlay.lines.push((o, s.p, axis_color(0), 2.5));
                if self.finishes_pub(i) && s.p.distance(o) > 1e-6 {
                    self.state = ToolState::AxesY { o, x: (s.p - o).normalize() };
                }
            }
            ToolState::AxesY { o, x } => {
                let s = self.snap(i.px, Some(o));
                let y = ((s.p - o) - x * (s.p - o).dot(x)).normalize_or_zero();
                let l = self.cam.world_per_px(self.cam.depth(o)) * 80.0;
                self.overlay.lines.push((o, o + x * l, axis_color(0), 2.5));
                if y.length() > 0.5 {
                    self.overlay.lines.push((o, o + y * l, axis_color(1), 2.5));
                    self.overlay.lines.push((o, o + x.cross(y) * l, axis_color(2), 2.5));
                }
                if self.finishes_pub(i) && y.length() > 0.5 {
                    self.checkpoint_keep_sel();
                    self.doc.axes = ModelAxes { origin: o, x, y, z: x.cross(y) };
                    self.state = ToolState::Idle;
                    self.dirty = true;
                    self.message = "Eixos de desenho redefinidos.".into();
                }
            }
            _ => {
                let s = self.snap(i.px, None);
                if i.pressed {
                    self.state = ToolState::AxesX { o: s.p };
                }
            }
        }
    }

    pub(crate) fn section_tool(&mut self, i: &In) {
        let s = self.snap(i.px, None);
        let n = match s.face {
            Some(f) => f.n,
            None => match self.plane_lock {
                Some(k) => axes()[k],
                None => -self.cam.forward(),
            },
        };
        // preview: a square on the candidate plane
        let r = self.cam.world_per_px(self.cam.depth(s.p)) * 60.0;
        let (u, v) = basis_for(n);
        let q = [s.p - u * r - v * r, s.p + u * r - v * r, s.p + u * r + v * r, s.p - u * r + v * r];
        for k in 0..4 {
            self.overlay.lines.push((q[k], q[(k + 1) % 4], SECTION_COLOR, 2.0));
        }
        self.overlay.lines.push((s.p, s.p + n * r * 0.8, SECTION_COLOR, 2.0));
        if i.pressed {
            self.checkpoint_keep_sel();
            for sp in &mut self.doc.sections {
                sp.active = false;
            }
            self.doc.sections.push(SectionPlane { p: s.p, n, active: true, ..Default::default() });
            self.dirty = true;
            self.message = "Plano de seção criado (ajuste em Exibição).".into();
            self.set_tool(Tool::Select);
        }
    }

    pub(crate) fn label_at(&self, px: DVec2) -> Option<usize> {
        let mut best = None;
        for (k, l) in self.doc.labels.iter().enumerate() {
            if let Some(p) = self.cam.project(l.pos) {
                if p.distance(px) < 30.0 {
                    best = Some(k);
                }
            }
        }
        best
    }

    pub(crate) fn dim_at(&self, px: DVec2) -> Option<usize> {
        for (k, d) in self.doc.dims.iter().enumerate() {
            if let Some((a, b)) = self.cam.project_segment(d.a + d.off, d.b + d.off) {
                if seg_dist(px, a, b).0 < 6.0 {
                    return Some(k);
                }
            }
        }
        None
    }

    pub(crate) fn vcb_annot(&mut self, text: &str) -> bool {
        match self.state.clone() {
            ToolState::ProtAngle { c, n, r0 } => {
                let Some(a) = parse_angle(text) else { return false };
                let dir = DMat4::from_axis_angle(n, a.to_radians()).transform_vector3(r0);
                self.checkpoint_keep_sel();
                self.doc.guides.push(Guide::Line { p: c, dir });
                self.state = ToolState::Idle;
                true
            }
            ToolState::DimOffset { a, b } => {
                let Some(l) = crate::units::parse_len(text) else { return false };
                let w = self.preview_dim.as_ref().map(|d| d.off).filter(|w| w.length() > 1e-9).unwrap_or(basis_for((b - a).normalize()).0);
                self.checkpoint_keep_sel();
                self.doc.dims.push(Dimension { a, b, off: w.normalize() * l });
                self.state = ToolState::Idle;
                true
            }
            _ => false,
        }
    }

    /// Insert 3D text as a component and start placing it.
    pub(crate) fn insert_text3d(&mut self, o: &Text3dDialog) {
        match text_geometry(o) {
            Ok(g) => {
                self.checkpoint_keep_sel();
                let n = self.doc.defs.len() + 1;
                self.doc.defs.push(Definition { name: format!("Texto 3D#{n}"), geom: g, group: false, face_me: false, procedure: None, ifc: None, author: None, light: None });
                let d = self.doc.defs.len() - 1;
                self.set_tool(Tool::Place(d));
                self.message = "Clique para posicionar o texto 3D.".into();
            }
            Err(e) => self.message = format!("Texto 3D: {e}"),
        }
    }

    pub(crate) fn annot_windows(&mut self, ctx: &egui::Context) {
        if let Some(k) = self.edit_label.filter(|k| *k < self.doc.labels.len()) {
            let mut open = true;
            let mut done = false;
            let mut delete = false;
            egui::Window::new("Texto").collapsible(false).resizable(false).open(&mut open).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
                let r = ui.add(egui::TextEdit::multiline(&mut self.doc.labels[k].text).desired_rows(3).desired_width(260.0));
                if r.changed() {
                    self.modified = true;
                }
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        done = true;
                    }
                    if ui.button("Apagar").clicked() {
                        delete = true;
                    }
                });
            });
            if delete {
                self.doc.labels.remove(k);
                self.edit_label = None;
            } else if done || !open {
                self.edit_label = None;
            }
        }
        if let Some(mut o) = self.text3d.take() {
            let mut open = true;
            let mut go = false;
            egui::Window::new("Texto 3D").collapsible(false).resizable(false).open(&mut open).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
                ui.add(egui::TextEdit::multiline(&mut o.text).desired_rows(2).desired_width(260.0));
                egui::Grid::new("t3d").show(ui, |ui| {
                    ui.label("Altura (m):");
                    ui.add(egui::DragValue::new(&mut o.height).speed(0.01).range(0.01..=100.0));
                    ui.end_row();
                    ui.checkbox(&mut o.extrude, "Extrudado");
                    ui.add_enabled(o.extrude, egui::DragValue::new(&mut o.depth).speed(0.01).range(0.001..=100.0));
                    ui.end_row();
                });
                if ui.button("Inserir").clicked() {
                    go = true;
                }
            });
            if go {
                self.insert_text3d(&o);
            } else if open {
                self.text3d = Some(o);
            } else {
                self.set_tool(Tool::Select);
            }
        }
    }

    /// Dimensions, labels and section planes in the 2D overlay.
    pub(crate) fn paint_annotations(&self, painter: &Painter, rect: Rect) {
        let to_pos = |v: DVec2| rect.min + Vec2::new(v.x as f32, v.y as f32);
        let cam = &self.cam;
        let font = FontId::proportional(13.0);
        let tag = |at: Pos2, text: &str, painter: &Painter| {
            let galley = painter.layout_no_wrap(text.to_string(), font.clone(), DIM_COLOR);
            let r = Rect::from_center_size(at, galley.size()).expand(2.0);
            painter.rect_filled(r, 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 230));
            painter.galley(r.min + Vec2::splat(2.0), galley, DIM_COLOR);
        };
        let dims = self.doc.dims.iter().chain(self.preview_dim.iter());
        for d in dims {
            let (a2, b2) = (d.a + d.off, d.b + d.off);
            let st = Stroke::new(1.0, DIM_COLOR);
            for (p, q) in [(d.a, a2 + d.off.normalize_or_zero() * d.off.length().min(0.05)), (d.b, b2 + d.off.normalize_or_zero() * d.off.length().min(0.05))] {
                if let Some((s, e)) = cam.project_segment(p, q) {
                    painter.line_segment([to_pos(s), to_pos(e)], Stroke::new(0.8, Color32::from_gray(90)));
                }
            }
            if let Some((s, e)) = cam.project_segment(a2, b2) {
                let (s, e) = (to_pos(s), to_pos(e));
                painter.line_segment([s, e], st);
                let dir = (e - s).normalized();
                let perp = Vec2::new(-dir.y, dir.x);
                for p in [s, e] {
                    painter.line_segment([p - (dir + perp) * 5.0, p + (dir + perp) * 5.0], Stroke::new(1.5, DIM_COLOR));
                }
                tag(s + (e - s) * 0.5, &format_len(d.a.distance(d.b)), painter);
            }
        }
        for l in &self.doc.labels {
            if let (Some((a, p)), Some(_)) = (cam.project_segment(l.anchor, l.pos), cam.project(l.pos)) {
                painter.line_segment([to_pos(a), to_pos(p)], Stroke::new(1.0, DIM_COLOR));
                painter.circle_filled(to_pos(a), 2.5, DIM_COLOR);
                let galley = painter.layout(l.text.clone(), font.clone(), DIM_COLOR, 240.0);
                let r = Rect::from_min_size(to_pos(p) + Vec2::new(4.0, -galley.size().y * 0.5), galley.size()).expand(3.0);
                painter.rect_filled(r, 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 235));
                painter.rect_stroke(r, 2.0, Stroke::new(1.0, Color32::from_gray(150)), egui::StrokeKind::Inside);
                painter.galley(r.min + Vec2::splat(3.0), galley, DIM_COLOR);
            }
        }
        if self.display.section_planes {
            let size = self.doc.bounds().map(|(lo, hi)| (hi - lo).length() * 0.3).unwrap_or(2.0).max(1.0);
            for (si, s) in self.doc.sections.iter().enumerate() {
                if s.hidden {
                    continue;
                }
                let (u, v) = basis_for(s.n);
                let q = [s.p - u * size - v * size, s.p + u * size - v * size, s.p + u * size + v * size, s.p - u * size + v * size];
                let picked = self.outliner_section == Some(si);
                let c = if picked { Color32::from_rgb(30, 80, 255) } else if s.active { SECTION_COLOR } else { Color32::from_gray(150) };
                for k in 0..4 {
                    if let Some((a, b)) = cam.project_segment(q[k], q[(k + 1) % 4]) {
                        painter.line_segment([to_pos(a), to_pos(b)], Stroke::new(if picked { 3.0 } else { 1.5 }, c));
                    }
                }
                if let Some((a, b)) = cam.project_segment(s.p, s.p + s.n * size * 0.2) {
                    painter.arrow(to_pos(a), to_pos(b) - to_pos(a), Stroke::new(2.0, c));
                }
            }
        }
        if self.display.true_north {
            let a = self.doc.location.north_deg.to_radians();
            let nv = DVec3::new(a.sin(), a.cos(), 0.0);
            let o = crate::pick::axes_origin();
            if let Some((p, q)) = cam.project_segment(o, o + nv * (cam.dist * 0.6)) {
                let c = Color32::from_rgb(230, 140, 0);
                crate::app::dashed(painter, to_pos(p), to_pos(q), Stroke::new(2.0, c), 8.0, 4.0, rect);
                painter.text(to_pos(q), Align2::CENTER_BOTTOM, "N", FontId::proportional(16.0), c);
            }
        }
        if self.display.section_cuts {
            for (a, b) in &self.section_cut {
                if let Some((p, q)) = cam.project_segment(*a, *b) {
                    painter.line_segment([to_pos(p), to_pos(q)], Stroke::new(self.display.cut_width, Color32::BLACK));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_3d_letters_with_holes() {
        let g = text_geometry(&Text3dDialog { text: "oA".into(), height: 1.0, depth: 0.0, extrude: false }).unwrap();
        // 'o' has a hole, 'A' too
        assert_eq!(g.faces.len(), 2);
        assert!(g.faces.iter().all(|f| f.holes.len() == 1));
        let e = text_geometry(&Text3dDialog { text: "I".into(), height: 1.0, depth: 0.2, extrude: true }).unwrap();
        assert_eq!(e.faces.len(), 6);
        let hi = e.verts.iter().fold(DVec3::splat(-1.0), |a, b| a.max(*b));
        assert!((hi.z - 0.2).abs() < 1e-9 && (hi.y - 1.0).abs() < 0.02);
    }

    #[test]
    fn section_through_a_box() {
        let mut doc = Document::new();
        let eye = DVec3::new(10.0, -10.0, 10.0);
        doc.root.add_polygon(&[DVec3::ZERO, DVec3::new(2.0, 0.0, 0.0), DVec3::new(2.0, 2.0, 0.0), DVec3::new(0.0, 2.0, 0.0)], None, false, eye);
        doc.root.push_pull(0, 2.0, false, eye);
        let pl = Plane { n: DVec3::X, d: 1.0 };
        let segs: Vec<_> = section_segments_grouped(&doc, &[], &pl).into_iter().flat_map(|g| g.1).collect();
        assert_eq!(segs.len(), 4);
        let tris = section_fill(&segs, &pl);
        let area: f64 = tris.iter().map(|t| (t[1] - t[0]).cross(t[2] - t[0]).length() * 0.5).sum();
        assert!((area - 4.0).abs() < 1e-9);
    }
}
