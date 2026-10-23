//! Cut / Copy / Paste / Paste in place (internal clipboard: geometry and
//! groups/components keep their definitions, materials and tags).

use crate::app::{HomeUpApp, Tool, ToolState};
use crate::geom::Geom;
use eframe::egui::Pos2;
use glam::{DMat4, DVec3};

pub struct Clip {
    pub geom: Geom,
    /// World transform of the context the geometry was copied from.
    pub xf: DMat4,
}

impl HomeUpApp {
    pub(crate) fn copy_selection(&mut self) -> bool {
        if self.sel.is_empty() {
            self.message = "Nada selecionado para copiar.".into();
            return false;
        }
        let g = self.doc.geom(&self.path).subset(&self.sel);
        let n = g.faces.len() + g.instances.len() + g.edges.len();
        self.clipboard = Some(Clip { geom: g, xf: self.doc.xf(&self.path) });
        self.message = format!("{n} entidades copiadas.");
        self.clip_text = Some(format!("HomeUp: {n} entidades"));
        true
    }

    pub(crate) fn cut_selection(&mut self) {
        self.drop_locked();
        if self.copy_selection() {
            self.delete_selection();
            self.message = "Recortado — use Colar (Ctrl+V) para posicionar.".into();
        }
    }

    /// Paste at the original position; returns the world bounding box of the pasted items.
    pub(crate) fn paste_in_place(&mut self) -> Option<(DVec3, DVec3)> {
        let clip = self.clipboard.as_ref()?;
        let (g, from) = (clip.geom.clone(), clip.xf);
        self.cancel();
        self.checkpoint();
        let to = self.doc.xf(&self.path);
        let m = to.inverse() * from;
        let sel = self.active_mut().append(&g, m, None);
        self.sel = sel;
        self.dirty = true;
        self.cache_dirty = true;
        // bounds in world space
        let mut lo = DVec3::splat(f64::MAX);
        let mut hi = DVec3::splat(f64::MIN);
        for p in &g.verts {
            let w = from.transform_point3(*p);
            lo = lo.min(w);
            hi = hi.max(w);
        }
        for inst in &g.instances {
            if let Some((a, b)) = self.doc.def_bounds(inst.def) {
                let xf = from * self.doc.eff_xf(inst);
                for c in 0..8 {
                    let p = DVec3::new(if c & 1 == 0 { a.x } else { b.x }, if c & 2 == 0 { a.y } else { b.y }, if c & 4 == 0 { a.z } else { b.z });
                    let w = xf.transform_point3(p);
                    lo = lo.min(w);
                    hi = hi.max(w);
                }
            }
        }
        (lo.x <= hi.x).then_some((lo, hi))
    }

    /// Paste and let the user place the copy with the mouse (Move tool).
    pub(crate) fn paste(&mut self) {
        if self.clipboard.is_none() {
            self.message = "A área de transferência está vazia.".into();
            return;
        }
        if let Some((lo, hi)) = self.paste_in_place() {
            let sel = self.sel.clone();
            self.set_tool(Tool::Move);
            self.sel = sel;
            let base = DVec3::new((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5, lo.z);
            self.state = ToolState::Moving { base, press: Pos2::new(-1e5, -1e5) };
            self.message = "Colado — clique para posicionar (Esc deixa no lugar original).".into();
        }
    }
}
