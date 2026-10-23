//! Procedures > Copy Along: copies of a group/component along a path
//! (entire path, each segment, or on corners) as a non-destructive procedural
//! component. The path lives inside the procedural component as edges; the
//! copies are regenerated from it and from the options in Entity Info.

use crate::app::{HomeUpApp, In};
use crate::geom::{Geom, Instance, VId};
use eframe::egui::{self, Color32};
use glam::{DMat4, DQuat, DVec3};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlongMode {
    EntirePath,
    EachSegment,
    OnCorners,
}

impl AlongMode {
    pub const ALL: [AlongMode; 3] = [AlongMode::EntirePath, AlongMode::EachSegment, AlongMode::OnCorners];
    pub fn label(self) -> &'static str {
        match self {
            AlongMode::EntirePath => "Caminho inteiro",
            AlongMode::EachSegment => "Cada segmento",
            AlongMode::OnCorners => "Nos cantos",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Distribution {
    ByCount(usize),
    ByDistance(f64),
    ByGap(f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation {
    /// Yaw only (copies stay upright, like steps).
    Stair,
    /// Yaw and pitch (copies follow slopes, like a road).
    Road,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CopyAlong {
    /// Definition being copied.
    pub source: usize,
    /// Rotation/scale of the original instance (no translation).
    pub base: DMat4,
    /// Source size along its own red axis (for By Gap).
    pub source_len: f64,
    pub mode: AlongMode,
    pub dist: Distribution,
    pub orient: Orientation,
    /// Margin at both ends of each span (Each Segment): meters, or a fraction when relative.
    pub margin: f64,
    pub margin_relative: bool,
}

/// Ordered polyline from the edges of `g` (one chain; closed when it loops).
pub fn path_of(g: &Geom) -> Option<(Vec<DVec3>, bool)> {
    if g.edges.is_empty() {
        return None;
    }
    let mut adj: HashMap<VId, Vec<VId>> = HashMap::new();
    for e in &g.edges {
        adj.entry(e.v[0]).or_default().push(e.v[1]);
        adj.entry(e.v[1]).or_default().push(e.v[0]);
    }
    let start = adj.iter().find(|(_, n)| n.len() == 1).map(|(v, _)| *v).unwrap_or(g.edges[0].v[0]);
    let mut out = vec![start];
    let mut prev = None;
    let mut cur = start;
    loop {
        let next = adj[&cur].iter().copied().find(|n| Some(*n) != prev && (out.len() < 2 || *n != out[out.len() - 2]));
        let Some(n) = next else { break };
        if n == start {
            return Some((out.iter().map(|v| g.p(*v)).collect(), true));
        }
        if out.contains(&n) || out.len() > g.edges.len() + 1 {
            break;
        }
        out.push(n);
        prev = Some(cur);
        cur = n;
    }
    Some((out.iter().map(|v| g.p(*v)).collect(), false))
}

fn orient_to(dir: DVec3, o: Orientation) -> DQuat {
    let flat = DVec3::new(dir.x, dir.y, 0.0);
    let yaw = if flat.length() > 1e-9 { flat.y.atan2(flat.x) } else { 0.0 };
    let q = DQuat::from_rotation_z(yaw);
    match o {
        Orientation::Stair => q,
        Orientation::Road => {
            let pitch = dir.z.atan2(flat.length());
            q * DQuat::from_rotation_y(-pitch)
        }
    }
}

/// Positions along a span of length `len` (distances from its start).
fn stations(len: f64, ca: &CopyAlong) -> Vec<f64> {
    let m = if ca.margin_relative { ca.margin.clamp(0.0, 0.49) * len } else { ca.margin.max(0.0).min(len * 0.49) };
    let (a, b) = (m, len - m);
    let span = (b - a).max(0.0);
    match ca.dist {
        Distribution::ByCount(n) => {
            let n = n.clamp(1, 10_000);
            if n == 1 {
                vec![a + span * 0.5]
            } else {
                (0..n).map(|i| a + span * i as f64 / (n - 1) as f64).collect()
            }
        }
        Distribution::ByDistance(d) => {
            let d = d.max(1e-3);
            let n = ((span / d).floor() as usize).min(10_000);
            (0..=n).map(|i| a + d * i as f64).collect()
        }
        Distribution::ByGap(g) => {
            let step = (ca.source_len + g).max(1e-3);
            let n = ((span / step).floor() as usize).min(10_000);
            (0..=n).map(|i| a + step * i as f64).collect()
        }
    }
}

/// Transforms of the copies for a path.
pub fn placements(path: &[DVec3], closed: bool, ca: &CopyAlong) -> Vec<DMat4> {
    let mut pts = path.to_vec();
    if closed && pts.len() > 2 {
        pts.push(pts[0]);
    }
    if pts.len() < 2 {
        return Vec::new();
    }
    let place = |p: DVec3, dir: DVec3| DMat4::from_translation(p) * DMat4::from_quat(orient_to(dir, ca.orient)) * ca.base;
    let mut out = Vec::new();
    match ca.mode {
        AlongMode::OnCorners => {
            let n = pts.len();
            let last = if closed { n - 1 } else { n };
            for i in 0..last {
                let din = if i > 0 { pts[i] - pts[i - 1] } else if closed { pts[i] - pts[n - 2] } else { DVec3::ZERO };
                let dout = if i + 1 < n { pts[i + 1] - pts[i] } else { DVec3::ZERO };
                let dir = (din.normalize_or_zero() + dout.normalize_or_zero()).normalize_or(dout.normalize_or(din.normalize_or(DVec3::X)));
                out.push(place(pts[i], dir));
            }
        }
        AlongMode::EachSegment => {
            for w in pts.windows(2) {
                let (a, b) = (w[0], w[1]);
                let len = a.distance(b);
                if len < 1e-9 {
                    continue;
                }
                let dir = (b - a) / len;
                for s in stations(len, ca) {
                    out.push(place(a + dir * s, dir));
                }
            }
        }
        AlongMode::EntirePath => {
            let lens: Vec<f64> = pts.windows(2).map(|w| w[0].distance(w[1])).collect();
            let total: f64 = lens.iter().sum();
            let full = CopyAlong { margin: 0.0, ..ca.clone() };
            for s in stations(total, &full) {
                let mut acc = 0.0;
                for (k, l) in lens.iter().enumerate() {
                    if s <= acc + l + 1e-9 || k == lens.len() - 1 {
                        let dir = (pts[k + 1] - pts[k]).normalize_or(DVec3::X);
                        out.push(place(pts[k] + dir * (s - acc).clamp(0.0, *l), dir));
                        break;
                    }
                    acc += l;
                }
            }
            if closed {
                // the end coincides with the start on a loop
                if out.len() > 1 && out[0].w_axis.distance(out[out.len() - 1].w_axis) < 1e-6 {
                    out.pop();
                }
            }
        }
    }
    out
}

impl HomeUpApp {
    /// Edges connected to `ei` through vertices used by exactly two edges (the hovered path).
    fn path_edges(&self, ei: usize) -> Vec<usize> {
        let g = self.active();
        let mut deg: HashMap<VId, usize> = HashMap::new();
        for e in &g.edges {
            for v in e.v {
                *deg.entry(v).or_default() += 1;
            }
        }
        let mut set = vec![ei];
        let mut stack = vec![ei];
        while let Some(e) = stack.pop() {
            for v in g.edges[e].v {
                if deg[&v] != 2 {
                    continue;
                }
                for (k, o) in g.edges.iter().enumerate() {
                    if !set.contains(&k) && o.v.contains(&v) {
                        set.push(k);
                        stack.push(k);
                    }
                }
            }
        }
        set
    }

    pub(crate) fn start_copy_along(&mut self, mode: AlongMode) {
        if self.sel.insts.len() != 1 {
            self.message = "Copiar ao longo: selecione um grupo ou componente primeiro.".into();
            return;
        }
        self.copy_along = Some(mode);
        self.message = format!("Copiar ao longo ({}): passe o cursor sobre o caminho e clique.", mode.label());
    }

    pub(crate) fn copy_along_frame(&mut self, i: &In) -> bool {
        let Some(mode) = self.copy_along else { return false };
        let Some(&src) = self.sel.insts.iter().next() else {
            self.copy_along = None;
            return false;
        };
        let hit = self.cache.pick_edge(&self.cam, i.px, 6.0, |e| e.owner == crate::doc::Owner::Active).map(|k| self.cache.edges[k].edge);
        let Some(ei) = hit else { return true };
        let edges = self.path_edges(ei);
        let xf = self.ctx_xf();
        let g = self.active();
        let segs: Vec<[DVec3; 2]> = edges.iter().map(|e| g.edges[*e].v.map(|v| xf.transform_point3(g.p(v)))).collect();
        for [a, b] in segs {
            self.overlay.lines.push((a, b, Color32::from_rgb(230, 120, 0), 3.0));
        }
        if !i.pressed {
            return true;
        }
        // build the procedural component: path edges + copies of the source
        self.checkpoint();
        let g = self.active();
        let inst = g.instances[src].clone();
        let mut path = Geom::default();
        let mut map: HashMap<VId, VId> = HashMap::new();
        for e in &edges {
            let vs = g.edges[*e].v.map(|v| *map.entry(v).or_insert_with(|| {
                path.verts.push(g.p(v));
                (path.verts.len() - 1) as VId
            }));
            path.edges.push(crate::geom::Edge::new(vs[0], vs[1]));
        }
        let base = DMat4::from_cols(inst.xf.x_axis, inst.xf.y_axis, inst.xf.z_axis, glam::DVec4::W);
        let source_len = self.doc.def_bounds(inst.def).map(|(lo, hi)| (base.transform_vector3(DVec3::X * (hi.x - lo.x))).length()).unwrap_or(1.0);
        let ca = CopyAlong {
            source: inst.def,
            base,
            source_len,
            mode,
            dist: match mode {
                AlongMode::EachSegment => Distribution::ByCount(3),
                _ => Distribution::ByCount(5),
            },
            orient: Orientation::Stair,
            margin: 0.0,
            margin_relative: false,
        };
        // remove the path edges and the original from the active context
        let g = self.active_mut();
        let mut es = edges.clone();
        es.sort_unstable();
        for e in es.into_iter().rev() {
            g.edges.remove(e);
        }
        g.instances.remove(src);
        let name = format!("Copiar ao longo: {}", self.doc.defs[inst.def].name);
        self.doc.defs.push(crate::doc::Definition { name, geom: path, group: false, face_me: false, procedure: Some(ca), ifc: None, author: None, light: None });
        let d = self.doc.defs.len() - 1;
        self.regen_procedure(d);
        let g = self.active_mut();
        g.instances.push(Instance { def: d, ..Default::default() });
        let k = g.instances.len() - 1;
        self.sel = crate::geom::Sel { insts: [k].into(), ..Default::default() };
        self.copy_along = None;
        self.cache_dirty = true;
        self.dirty = true;
        self.message = "Componente procedural criado — ajuste as opções em Informações da entidade; duplo clique edita o caminho.".into();
        true
    }

    /// Rebuild the copies of a procedural definition from its path and options.
    pub(crate) fn regen_procedure(&mut self, d: usize) {
        let Some(ca) = self.doc.defs.get(d).and_then(|x| x.procedure.clone()) else { return };
        let Some((path, closed)) = path_of(&self.doc.defs[d].geom) else { return };
        let xs = placements(&path, closed, &ca);
        let geom = &mut self.doc.defs[d].geom;
        geom.instances.retain(|i| i.def != ca.source);
        for xf in xs {
            geom.instances.push(Instance { def: ca.source, xf, ..Default::default() });
        }
        self.cache_dirty = true;
        self.dirty = true;
    }

    pub(crate) fn regen_all_procedures(&mut self) {
        for d in 0..self.doc.defs.len() {
            if self.doc.defs[d].procedure.is_some() {
                self.regen_procedure(d);
            }
        }
    }

    /// Entity Info section of a procedural component.
    pub(crate) fn procedure_ui(&mut self, ui: &mut egui::Ui, d: usize) {
        let Some(mut ca) = self.doc.defs[d].procedure.clone() else { return };
        ui.separator();
        ui.strong("Copiar ao longo (procedural)");
        egui::ComboBox::from_label("Posicionamento").selected_text(ca.mode.label()).show_ui(ui, |ui| {
            for m in AlongMode::ALL {
                ui.selectable_value(&mut ca.mode, m, m.label());
            }
        });
        if ca.mode != AlongMode::OnCorners {
            ui.horizontal(|ui| {
                ui.label("Distribuição:");
                let (mut k, mut v) = match ca.dist {
                    Distribution::ByCount(n) => (0, n as f64),
                    Distribution::ByDistance(x) => (1, x),
                    Distribution::ByGap(x) => (2, x),
                };
                egui::ComboBox::from_id_salt("ca-dist").selected_text(["Por quantidade", "Por distância", "Por espaço"][k]).show_ui(ui, |ui| {
                    for (j, l) in ["Por quantidade", "Por distância", "Por espaço"].iter().enumerate() {
                        if ui.selectable_label(k == j, *l).clicked() && k != j {
                            k = j;
                            v = if j == 0 { 5.0 } else { 1.0 };
                        }
                    }
                });
                if k == 0 {
                    ui.add(egui::DragValue::new(&mut v).range(1.0..=1000.0).speed(0.2).fixed_decimals(0));
                } else {
                    ui.add(egui::DragValue::new(&mut v).range(0.0..=1000.0).speed(0.02).suffix(" m"));
                }
                ca.dist = match k {
                    0 => Distribution::ByCount(v.round().max(1.0) as usize),
                    1 => Distribution::ByDistance(v.max(0.001)),
                    _ => Distribution::ByGap(v),
                };
            });
        }
        ui.horizontal(|ui| {
            ui.label("Orientação:");
            ui.radio_value(&mut ca.orient, Orientation::Stair, "Escada (só giro)");
            ui.radio_value(&mut ca.orient, Orientation::Road, "Estrada (giro e inclinação)");
        });
        if ca.mode == AlongMode::EachSegment {
            ui.horizontal(|ui| {
                ui.label("Margem:");
                ui.radio_value(&mut ca.margin_relative, false, "Absoluta");
                ui.radio_value(&mut ca.margin_relative, true, "Relativa");
                if ca.margin_relative {
                    let mut pct = ca.margin * 100.0;
                    if ui.add(egui::DragValue::new(&mut pct).range(0.0..=49.0).suffix(" %")).changed() {
                        ca.margin = pct / 100.0;
                    }
                } else {
                    ui.add(egui::DragValue::new(&mut ca.margin).range(0.0..=1000.0).speed(0.02).suffix(" m"));
                }
            });
        }
        if Some(&ca) != self.doc.defs[d].procedure.as_ref() {
            self.checkpoint_keep_sel();
            self.doc.defs[d].procedure = Some(ca);
            self.regen_procedure(d);
        }
        let n = self.doc.defs[d].geom.instances.len();
        ui.weak(format!("{n} cópias. Duplo clique no componente edita o caminho; as cópias se atualizam ao sair."));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ca(mode: AlongMode, dist: Distribution) -> CopyAlong {
        CopyAlong { source: 0, base: DMat4::IDENTITY, source_len: 0.5, mode, dist, orient: Orientation::Stair, margin: 0.0, margin_relative: false }
    }
    #[test]
    fn placements_follow_the_path() {
        let path = [DVec3::ZERO, DVec3::new(4.0, 0.0, 0.0), DVec3::new(4.0, 4.0, 0.0)];
        let p = placements(&path, false, &ca(AlongMode::EntirePath, Distribution::ByCount(5)));
        assert_eq!(p.len(), 5);
        assert!(p[2].w_axis.truncate().distance(DVec3::new(4.0, 0.0, 0.0)) < 1e-9);
        assert!(p[4].w_axis.truncate().distance(DVec3::new(4.0, 4.0, 0.0)) < 1e-9);
        // copies on the second leg turn 90° (their red axis points along +y)
        assert!(p[3].x_axis.truncate().distance(DVec3::Y) < 1e-9);
        assert_eq!(placements(&path, false, &ca(AlongMode::EachSegment, Distribution::ByCount(3))).len(), 6);
        assert_eq!(placements(&path, false, &ca(AlongMode::OnCorners, Distribution::ByCount(1))).len(), 3);
        assert_eq!(placements(&path, false, &ca(AlongMode::EntirePath, Distribution::ByDistance(1.0))).len(), 9);
        // by gap: source 0.5 + gap 0.5 = 1 m steps
        assert_eq!(placements(&path, false, &ca(AlongMode::EntirePath, Distribution::ByGap(0.5))).len(), 9);
    }

    #[test]
    fn path_chain_from_edges() {
        let mut g = Geom::default();
        let eye = DVec3::new(0.0, 0.0, 10.0);
        g.add_segments(&[(DVec3::new(2.0, 0.0, 0.0), DVec3::new(3.0, 0.0, 0.0)), (DVec3::ZERO, DVec3::new(2.0, 0.0, 0.0))], None, false, eye);
        let (p, closed) = path_of(&g).unwrap();
        assert!(!closed);
        assert_eq!(p.len(), 3);
        assert!(p[1].distance(DVec3::new(2.0, 0.0, 0.0)) < 1e-9);
    }
}
