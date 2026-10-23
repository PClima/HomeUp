//! Edge and face editing commands from SketchUp's context menus:
//! Divide, Weld Edges, Explode Curve, Area (selection / material) and the
//! Paint Bucket replace modes (all matching, all connected, same object).

use crate::app::{HomeUpApp, In};
use crate::geom::{Geom, VId};
use eframe::egui::Color32;
use glam::DMat4;
use std::collections::{HashMap, HashSet, VecDeque};

impl Geom {
    /// Split a straight edge into `n` equal segments; returns the new vertex ids.
    pub fn divide_edge(&mut self, ei: usize, n: usize) -> Vec<VId> {
        if ei >= self.edges.len() || n < 2 {
            return Vec::new();
        }
        let [a, b] = self.edges[ei].v;
        let (pa, pb) = (self.p(a), self.p(b));
        let mut cur = ei;
        let mut out = Vec::new();
        for k in 1..n {
            let p = pa.lerp(pb, k as f64 / n as f64);
            self.verts.push(p);
            let v = (self.verts.len() - 1) as VId;
            self.split_edge_pub(cur, v);
            // the remaining piece (v..b) was pushed last
            cur = self.edges.len() - 1;
            out.push(v);
        }
        out
    }

    /// Join edges into one curve. They must form a single open or closed chain.
    pub fn weld_edges(&mut self, edges: &HashSet<usize>) -> Result<u32, String> {
        if edges.len() < 2 {
            return Err("Selecione pelo menos duas arestas conectadas.".into());
        }
        let mut deg: HashMap<VId, usize> = HashMap::new();
        for e in edges {
            for v in self.edges[*e].v {
                *deg.entry(v).or_default() += 1;
            }
        }
        if deg.values().any(|d| *d > 2) {
            return Err("As arestas se ramificam — uma curva precisa ser uma sequência simples.".into());
        }
        // connectivity
        let list: Vec<usize> = edges.iter().copied().collect();
        let mut seen = HashSet::from([list[0]]);
        let mut q = VecDeque::from([list[0]]);
        while let Some(e) = q.pop_front() {
            for f in &list {
                if !seen.contains(f) && self.edges[*f].v.iter().any(|v| self.edges[e].v.contains(v)) {
                    seen.insert(*f);
                    q.push_back(*f);
                }
            }
        }
        if seen.len() != list.len() {
            return Err("As arestas selecionadas não estão conectadas.".into());
        }
        let c = self.new_curve();
        for e in edges {
            self.edges[*e].curve = Some(c);
        }
        Ok(c)
    }

    /// Break the curve containing `ei` back into independent segments.
    pub fn explode_curve(&mut self, ei: usize) -> usize {
        let es = self.curve_edges(ei);
        for e in &es {
            self.edges[*e].curve = None;
            self.edges[*e].arc = false;
        }
        es.len()
    }

    /// Faces reachable from `fi` through shared edges that use the same material.
    pub fn connected_same_material(&self, fi: usize) -> Vec<usize> {
        let mat = self.faces[fi].material;
        let key = |a: VId, b: VId| (a.min(b), a.max(b));
        let mut by_edge: HashMap<(VId, VId), Vec<usize>> = HashMap::new();
        for (k, f) in self.faces.iter().enumerate() {
            for l in f.loops() {
                for i in 0..l.len() {
                    by_edge.entry(key(l[i], l[(i + 1) % l.len()])).or_default().push(k);
                }
            }
        }
        let mut seen = HashSet::from([fi]);
        let mut q = VecDeque::from([fi]);
        while let Some(f) = q.pop_front() {
            for l in self.faces[f].loops() {
                for i in 0..l.len() {
                    for &n in by_edge.get(&key(l[i], l[(i + 1) % l.len()])).into_iter().flatten() {
                        if self.faces[n].material == mat && seen.insert(n) {
                            q.push_back(n);
                        }
                    }
                }
            }
        }
        seen.into_iter().collect()
    }
}

/// Area of a face after a transform (handles non-uniform scale).
pub fn face_area_xf(g: &Geom, f: &crate::geom::Face, xf: DMat4) -> f64 {
    let mut total = 0.0;
    for t in g.triangulate_face(f) {
        let [a, b, c] = t.map(|v| xf.transform_point3(g.p(v)));
        total += (b - a).cross(c - a).length() * 0.5;
    }
    total
}

impl HomeUpApp {
    /// Interactive Divide (context menu on an edge): the cursor position sets
    /// the number of segments, typing a number sets it exactly, click applies.
    pub(crate) fn divide_frame(&mut self, i: &In) -> bool {
        let Some((ei, mut n)) = self.divide else { return false };
        let g = self.active();
        if ei >= g.edges.len() {
            self.divide = None;
            return false;
        }
        let xf = self.ctx_xf();
        let [a, b] = g.edges[ei].v.map(|v| xf.transform_point3(g.p(v)));
        if let (Some(sa), Some(sb)) = (self.cam.project(a), self.cam.project(b)) {
            let d = sb - sa;
            let t = ((i.px - sa).dot(d) / d.length_squared().max(1e-9)).clamp(0.0, 1.0);
            if !self.divide_typed {
                // center = few segments, near the ends = many
                n = 2 + ((t - 0.5).abs() * 2.0 * 18.0).round() as usize;
            }
        }
        self.divide = Some((ei, n));
        self.overlay.lines.push((a, b, Color32::from_rgb(200, 30, 200), 3.0));
        for k in 1..n {
            self.overlay.squares.push((a.lerp(b, k as f64 / n as f64), Color32::from_rgb(200, 30, 200)));
        }
        self.live_measure = Some(format!("{n} segmentos"));
        self.message = "Dividir: aproxime das pontas para mais segmentos, do centro para menos (ou digite o número). Clique para aplicar.".into();
        if i.pressed {
            self.checkpoint();
            self.active_mut().divide_edge(ei, n);
            self.divide = None;
            self.divide_typed = false;
            self.live_measure = None;
            self.message = format!("Aresta dividida em {n} segmentos.");
        }
        true
    }

    pub(crate) fn start_divide(&mut self) {
        let Some(&ei) = self.sel.edges.iter().next() else { return };
        let g = self.active();
        if g.edges[ei].curve.is_some() && g.curve_edges(ei).len() > 1 {
            self.message = "Dividir funciona em linhas retas — use Explodir curva antes.".into();
            return;
        }
        self.divide = Some((ei, 4));
        self.divide_typed = false;
    }

    pub(crate) fn weld_selection(&mut self) {
        let edges = self.sel.edges.clone();
        self.checkpoint_keep_sel();
        match self.active_mut().weld_edges(&edges) {
            Ok(_) => self.message = format!("{} arestas soldadas em uma curva.", edges.len()),
            Err(e) => {
                self.undo.pop();
                self.message = e;
            }
        }
    }

    pub(crate) fn explode_curve_selection(&mut self) {
        let Some(&ei) = self.sel.edges.iter().next() else { return };
        self.checkpoint_keep_sel();
        let n = self.active_mut().explode_curve(ei);
        self.message = format!("Curva explodida em {n} segmentos.");
    }

    /// Area of the selected faces (world units).
    pub(crate) fn selection_area(&self) -> f64 {
        let g = self.active();
        let xf = self.ctx_xf();
        self.sel.faces.iter().filter_map(|f| g.faces.get(*f)).map(|f| face_area_xf(g, f, xf)).sum()
    }

    /// Total area painted with a material in the whole model (instances included).
    pub(crate) fn material_area(&self, m: usize) -> f64 {
        let mut total = 0.0;
        self.doc.visit(&[], &mut |it| {
            for f in &it.geom.faces {
                if f.material.or(it.mat) == Some(m) && self.doc.face_visible(f) {
                    total += face_area_xf(it.geom, f, it.xf);
                }
                if f.back_material == Some(m) {
                    total += face_area_xf(it.geom, f, it.xf);
                }
            }
        });
        total
    }

    /// Paint Bucket: Shift = all matching faces in the model, Ctrl = all
    /// connected faces with the same material, Ctrl+Shift = same object.
    pub(crate) fn paint_replace(&mut self, f: usize, mods: eframe::egui::Modifiers) -> bool {
        let cur = self.cur_mat;
        let old = self.active().faces[f].material;
        if mods.ctrl && mods.shift {
            self.checkpoint_keep_sel();
            for face in &mut self.doc.geom_mut(&self.path.clone()).faces {
                if face.material == old {
                    face.material = cur;
                }
            }
            self.message = "Pintado: todas as faces iguais neste objeto.".into();
            return true;
        }
        if mods.ctrl {
            let faces = self.active().connected_same_material(f);
            self.checkpoint_keep_sel();
            let g = self.doc.geom_mut(&self.path.clone());
            for k in &faces {
                g.faces[*k].material = cur;
            }
            self.message = format!("Pintado: {} faces conectadas.", faces.len());
            return true;
        }
        if mods.shift {
            self.checkpoint_keep_sel();
            let mut n = 0;
            for face in self.doc.root.faces.iter_mut().chain(self.doc.defs.iter_mut().flat_map(|d| d.geom.faces.iter_mut())) {
                if face.material == old {
                    face.material = cur;
                    n += 1;
                }
            }
            self.message = format!("Pintado: {n} faces com o mesmo material no modelo.");
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;
    #[test]
    fn divide_weld_explode() {
        let mut g = Geom::default();
        let eye = DVec3::new(0.0, 0.0, 10.0);
        g.add_segments(&[(DVec3::ZERO, DVec3::X * 3.0)], None, false, eye);
        let vs = g.divide_edge(0, 3);
        assert_eq!(vs.len(), 2);
        assert_eq!(g.edges.len(), 3);
        assert!((g.p(vs[0]).x - 1.0).abs() < 1e-9 && (g.p(vs[1]).x - 2.0).abs() < 1e-9);
        let all: HashSet<usize> = (0..3).collect();
        let c = g.weld_edges(&all).unwrap();
        assert!(g.edges.iter().all(|e| e.curve == Some(c)));
        assert_eq!(g.curve_edges(0).len(), 3);
        assert_eq!(g.explode_curve(0), 3);
        assert!(g.edges.iter().all(|e| e.curve.is_none()));
        // branching edges cannot be welded
        g.add_segments(&[(DVec3::X, DVec3::new(1.0, 1.0, 0.0))], None, false, eye);
        let all: HashSet<usize> = (0..g.edges.len()).collect();
        assert!(g.weld_edges(&all).is_err());
    }

    #[test]
    fn connected_paint_and_area() {
        let mut g = Geom::default();
        let eye = DVec3::new(0.3, 0.2, 10.0);
        g.add_polygon(&[DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0), DVec3::Y], None, false, eye);
        g.add_polygon(&[DVec3::X, DVec3::new(2.0, 0.0, 0.0), DVec3::new(2.0, 1.0, 0.0), DVec3::new(1.0, 1.0, 0.0)], None, false, eye);
        g.add_polygon(&[DVec3::new(5.0, 0.0, 0.0), DVec3::new(6.0, 0.0, 0.0), DVec3::new(6.0, 1.0, 0.0), DVec3::new(5.0, 1.0, 0.0)], None, false, eye);
        assert_eq!(g.faces.len(), 3);
        let c = g.connected_same_material(0);
        assert_eq!(c.len(), 2);
        let a = face_area_xf(&g, &g.faces[0], DMat4::from_scale(DVec3::new(2.0, 3.0, 1.0)));
        assert!((a - 6.0).abs() < 1e-9);
    }
}
