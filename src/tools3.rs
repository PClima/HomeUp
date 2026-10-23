//! Modeling tools: Follow Me, Flip, solid tools and Intersect Faces.

use crate::app::{HomeUpApp, In, Tool, ToolState, XKind};
use crate::doc::{Definition, Document, Owner};
use crate::geom::{Geom, Instance, Sel};
use crate::modops::{self, BoolOp, CPoly};
use crate::pick::{axis_color, axes};
use eframe::egui::Color32;
use glam::{DMat4, DVec2, DVec3};

impl Document {
    /// Closed polygon soup of a definition (nested instances included).
    pub fn flatten_polys(&self, def: usize, xf: DMat4, mat: Option<usize>) -> Vec<CPoly> {
        let mut out = Vec::new();
        self.flatten_rec(&self.defs[def].geom, xf, mat, &mut out, 0);
        out
    }
    fn flatten_rec(&self, g: &Geom, xf: DMat4, mat: Option<usize>, out: &mut Vec<CPoly>, depth: usize) {
        if depth > 32 {
            return;
        }
        out.extend(modops::polys_of(g, xf, mat));
        for inst in &g.instances {
            if inst.def < self.defs.len() {
                self.flatten_rec(&self.defs[inst.def].geom, xf * self.eff_xf(inst), inst.material.or(mat), out, depth + 1);
            }
        }
    }
}

impl HomeUpApp {
    // ---------------------------------------------------------------- Follow Me

    pub(crate) fn follow_me_tool(&mut self, i: &In) {
        self.snap = None;
        let (o, d) = self.cam.ray(i.px);
        let hit = self.cache.pick_face(o, d).filter(|h| self.cache.tris[h.1].owner == Owner::Active);
        self.hover_face = hit.map(|h| self.cache.tris[h.1].face);
        let Some(face) = self.hover_face.filter(|_| i.pressed) else { return };
        let g = self.active();
        let f = &g.faces[face];
        let own: Vec<usize> = f.boundary_edges().iter().filter_map(|(a, b)| g.find_edge(*a, *b)).collect();
        let path: Vec<usize> = self.sel.edges.iter().copied().filter(|e| !own.contains(e)).collect();
        if path.is_empty() {
            self.message = "Selecione primeiro o caminho (arestas) e depois clique na face do perfil.".into();
            return;
        }
        let centroid = g.face_sample_point(f);
        let Some((pts, closed)) = modops::chain_edges(g, &path, centroid) else {
            self.message = "O caminho precisa ser uma sequência contínua de arestas, sem ramificações.".into();
            return;
        };
        let mut trial = self.active().clone();
        match modops::follow_me(&mut trial, face, &pts, closed) {
            Ok(()) => {
                self.checkpoint();
                *self.active_mut() = trial;
                self.message = "Siga-me concluído.".into();
            }
            Err(e) => self.message = format!("Siga-me: {e}"),
        }
    }

    // ---------------------------------------------------------------- Flip

    pub(crate) fn flip_tool(&mut self, i: &In) {
        self.snap = None;
        if self.sel.is_empty() {
            let e = self.entity_at(i.px);
            self.hover_face = e.faces.iter().next().copied();
            self.hover_inst = e.insts.iter().next().copied();
            if i.pressed {
                self.sel = e;
                self.dirty = true;
            }
            return;
        }
        let Some((lo, hi)) = self.sel_bounds_world() else { return };
        let c = (lo + hi) * 0.5;
        let ext = ((hi - lo) * 0.5 + DVec3::splat(0.05)).max(DVec3::splat(0.2));
        let mut hovered = None;
        for k in 0..3 {
            let (u, v) = match k {
                0 => (DVec3::Y * ext.y, DVec3::Z * ext.z),
                1 => (DVec3::X * ext.x, DVec3::Z * ext.z),
                _ => (DVec3::X * ext.x, DVec3::Y * ext.y),
            };
            let quad = [c - u - v, c + u - v, c + u + v, c - u + v];
            let scr: Option<Vec<DVec2>> = quad.iter().map(|p| self.cam.project(*p)).collect();
            if hovered.is_none() && scr.as_ref().is_some_and(|s| crate::geom::point_in_poly(i.px, s)) {
                hovered = Some(k);
            }
            let col = axis_color(k);
            let w = if hovered == Some(k) { 3.0 } else { 1.0 };
            for j in 0..4 {
                self.overlay.lines.push((quad[j], quad[(j + 1) % 4], col, w));
            }
        }
        if let Some(k) = hovered {
            let name = ["vermelho", "verde", "azul"][k];
            self.overlay.tooltip = Some((format!("Virar ao longo do eixo {name}"), axis_color(k)));
            self.snap = Some(crate::pick::Snap { p: c, kind: crate::pick::SnapKind::Free, face: None, dir: None, in_inst: false, ref_line: None });
            if i.pressed {
                let held = self.ctrl_held;
                self.ctrl_held = false;
                let mode = self.copy_mode;
                self.copy_mode = false;
                self.commit_xform(XKind::Scale { a: c, mask: axes()[k], t: -1.0 });
                self.copy_mode = mode;
                self.ctrl_held = held;
            }
        } else if i.pressed {
            self.sel = self.entity_at(i.px);
            self.dirty = true;
        }
    }

    // ---------------------------------------------------------------- solid tools

    fn solid_polys(&self, k: usize) -> Vec<CPoly> {
        let inst = &self.active().instances[k];
        self.doc.flatten_polys(inst.def, self.doc.eff_xf(inst), inst.material)
    }

    fn add_solid(&mut self, polys: &[CPoly], name: &str) -> Option<usize> {
        let mut g = modops::polys_to_geom(polys);
        if g.faces.is_empty() {
            return None;
        }
        let origin = crate::import::recenter(&mut g);
        let n = self.doc.defs.len() + 1;
        self.doc.defs.push(Definition { name: format!("{name}#{n}"), geom: g, group: true, face_me: false, procedure: None, ifc: None, author: None, light: None });
        let def = self.doc.defs.len() - 1;
        let gm = self.active_mut();
        gm.instances.push(Instance { def, xf: DMat4::from_translation(origin), material: None, name: String::new(), ..Default::default() });
        Some(gm.instances.len() - 1)
    }

    fn remove_instances(&mut self, mut ks: Vec<usize>) {
        ks.sort_unstable();
        ks.dedup();
        for k in ks.into_iter().rev() {
            self.active_mut().instances.remove(k);
        }
    }

    /// Run a solid tool on instances `a` (first clicked) and `b`.
    pub(crate) fn run_solid(&mut self, tool: Tool, a: usize, b: usize) {
        let (pa, pb) = (self.solid_polys(a), self.solid_polys(b));
        let (results, keep_a): (Vec<(Vec<CPoly>, &str)>, bool) = match tool {
            Tool::OuterShell | Tool::Union => (vec![(modops::boolean(pa, pb, BoolOp::Union), "União")], false),
            Tool::Subtract => (vec![(modops::boolean(pb, pa, BoolOp::Subtract), "Subtração")], false),
            Tool::SolidIntersect => (vec![(modops::boolean(pa, pb, BoolOp::Intersect), "Interseção")], false),
            Tool::Trim => (vec![(modops::boolean(pb, pa, BoolOp::Subtract), "Recorte")], true),
            Tool::Split => (
                vec![
                    (modops::boolean(pa.clone(), pb.clone(), BoolOp::Subtract), "Divisão"),
                    (modops::boolean(pb.clone(), pa.clone(), BoolOp::Subtract), "Divisão"),
                    (modops::boolean(pa, pb, BoolOp::Intersect), "Divisão"),
                ],
                false,
            ),
            _ => return,
        };
        if results.iter().all(|(r, _)| r.is_empty()) {
            self.message = "O resultado ficou vazio — os sólidos se sobrepõem?".into();
            return;
        }
        self.checkpoint();
        self.remove_instances(if keep_a { vec![b] } else { vec![a, b] });
        let mut sel = Sel::default();
        for (r, name) in results {
            if let Some(k) = self.add_solid(&r, name) {
                sel.insts.insert(k);
            }
        }
        self.sel = sel;
        self.message = format!("{} concluído.", tool.label());
    }

    /// Outer shell / union of every selected group or component.
    pub(crate) fn outer_shell_selection(&mut self) {
        let mut ks: Vec<usize> = self.sel.insts.iter().copied().collect();
        if ks.len() < 2 {
            self.message = "Selecione dois ou mais grupos/componentes sólidos.".into();
            return;
        }
        ks.sort_unstable();
        let mut acc = self.solid_polys(ks[0]);
        for k in &ks[1..] {
            acc = modops::boolean(acc, self.solid_polys(*k), BoolOp::Union);
        }
        if acc.is_empty() {
            self.message = "O resultado ficou vazio.".into();
            return;
        }
        self.checkpoint();
        self.remove_instances(ks);
        if let Some(k) = self.add_solid(&acc, "Revestimento") {
            self.sel.insts.insert(k);
        }
        self.message = "Revestimento externo concluído.".into();
    }

    pub(crate) fn solid_tool(&mut self, i: &In) {
        self.snap = None;
        let (o, d) = self.cam.ray(i.px);
        let hit = self.cache.pick_face(o, d).and_then(|h| match self.cache.tris[h.1].owner {
            Owner::Inst(k) => Some(k),
            _ => None,
        });
        self.hover_inst = hit;
        if self.tool == Tool::OuterShell && i.pressed && self.sel.insts.len() >= 2 && hit.is_some_and(|k| self.sel.insts.contains(&k)) {
            self.outer_shell_selection();
            return;
        }
        let first = if let ToolState::SolidPick { first } = self.state { Some(first) } else { None };
        if let Some(f) = first {
            self.hover_face = None;
            let a = &self.active().instances[f];
            if let Some((lo, hi)) = self.doc.def_bounds(a.def) {
                let xf = self.ctx_xf() * self.doc.eff_xf(a);
                let mut s = crate::render::Scene3D::default();
                crate::scene::box_lines(&mut s, lo, hi, xf, [0.0; 4]);
                for w in s.lines.chunks_exact(2) {
                    let p = |v: &crate::render::Vertex| DVec3::new(v.pos[0] as f64, v.pos[1] as f64, v.pos[2] as f64);
                    self.overlay.lines.push((p(&w[0]), p(&w[1]), Color32::from_rgb(220, 120, 0), 2.0));
                }
            }
        }
        if !i.pressed {
            return;
        }
        let Some(k) = hit else {
            self.message = "Clique num grupo ou componente sólido.".into();
            return;
        };
        match first {
            None => self.state = ToolState::SolidPick { first: k },
            Some(f) if f != k => {
                self.state = ToolState::Idle;
                let tool = self.tool;
                self.run_solid(tool, f, k);
            }
            _ => {}
        }
    }

    // ---------------------------------------------------------------- Intersect Faces

    /// Intersect selected faces with the model (`with_model`) or with each other.
    pub(crate) fn intersect_selection(&mut self, with_model: bool) {
        let a: Vec<usize> = self.sel.faces.iter().copied().collect();
        if a.is_empty() {
            self.message = "Selecione as faces a intersectar.".into();
            return;
        }
        let b: Vec<usize> = if with_model { (0..self.active().faces.len()).collect() } else { a.clone() };
        let eye = self.local_eye();
        let mut trial = self.active().clone();
        let n = modops::intersect_faces(&mut trial, &a, &b, eye);
        if n == 0 {
            self.message = "Nenhuma interseção encontrada.".into();
            return;
        }
        self.checkpoint();
        *self.active_mut() = trial;
        self.message = format!("{n} segmentos de interseção criados.");
    }
}
