//! File > Import of 2D images: as an image entity (a textured rectangle placed
//! with two clicks), as a texture (new material, Paint Bucket) or as a matched
//! photo (see matchphoto.rs).

use crate::app::{HomeUpApp, In, Tool};
use crate::geom::Geom;
use eframe::egui::{self, Color32};
use glam::{DMat4, DVec3};

pub struct PendingImage {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Placement of an image entity: definition, height/width ratio, first corner and plane.
#[derive(Clone)]
pub struct ImagePlace {
    pub def: usize,
    pub aspect: f64,
    pub p0: Option<(DVec3, DVec3, DVec3, DVec3)>,
}

pub const IMAGE_EXTS: [&str; 5] = ["png", "jpg", "jpeg", "bmp", "webp"];

pub fn is_image(path: &std::path::Path) -> bool {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).is_some_and(|e| IMAGE_EXTS.contains(&e.as_str()))
}

/// 1 m wide rectangle with UVs covering the whole image once (v = 0 at the top).
pub fn image_geom(aspect: f64) -> Geom {
    let mut g = Geom::default();
    let pts = [DVec3::ZERO, DVec3::X, DVec3::new(1.0, aspect, 0.0), DVec3::new(0.0, aspect, 0.0)];
    g.add_polygon(&pts, None, false, DVec3::new(0.3, 0.2, 10.0));
    for f in &mut g.faces {
        let uvs: Vec<[f32; 2]> = f.loops().flatten().map(|v| {
            let p = g.verts[*v as usize];
            [p.x as f32, (1.0 - p.y / aspect) as f32]
        }).collect();
        f.uvs = Some(uvs);
    }
    g
}

fn plane_basis(n: DVec3) -> (DVec3, DVec3) {
    let u = if n.z.abs() > 0.9 { DVec3::X } else { DVec3::Z.cross(n).normalize() };
    (u, n.cross(u).normalize())
}

impl HomeUpApp {
    pub(crate) fn import_image_file(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Imagem", &IMAGE_EXTS).pick_file() else { return };
        self.start_image_import(&path);
    }

    pub(crate) fn start_image_import(&mut self, path: &std::path::Path) {
        match std::fs::read(path) {
            Ok(bytes) => self.pending_image = Some(PendingImage { name: path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), bytes }),
            Err(e) => self.message = format!("Falha ao ler a imagem: {e}"),
        }
    }

    fn image_material(&mut self, name: &str, bytes: Vec<u8>) -> Option<(usize, f64)> {
        match crate::tex::TextureData::from_bytes(name, bytes) {
            Ok(t) => {
                let aspect = t.h as f64 / t.w.max(1) as f64;
                let mut m = crate::doc::Material::color(name, t.average(), 1.0);
                self.doc.textures.push(t);
                m.texture = Some(self.doc.textures.len() - 1);
                m.tex_size = [1.0, aspect];
                m.source = "imagem importada".into();
                self.doc.materials.push(m);
                Some((self.doc.materials.len() - 1, aspect))
            }
            Err(e) => {
                self.message = e;
                None
            }
        }
    }

    /// The "Use as…" dialog after picking an image.
    pub(crate) fn image_import_window(&mut self, ctx: &egui::Context) {
        let Some(p) = &self.pending_image else { return };
        let name = p.name.clone();
        let mut choice = None;
        let mut open = true;
        egui::Window::new("Importar imagem").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(format!("Como usar \"{name}\"?"));
            ui.add_space(4.0);
            if ui.button("Como imagem — retângulo posicionado com dois cliques").clicked() {
                choice = Some(0);
            }
            if ui.button("Como textura — novo material para pintar faces").clicked() {
                choice = Some(1);
            }
            if ui.button("Como foto combinada — alinhar a câmera à foto").clicked() {
                choice = Some(2);
            }
        });
        if !open {
            self.pending_image = None;
            return;
        }
        let Some(c) = choice else { return };
        let p = self.pending_image.take().unwrap();
        match c {
            0 => {
                self.checkpoint_keep_sel();
                let Some((m, aspect)) = self.image_material(&p.name, p.bytes) else { return };
                let _ = m;
                self.doc.defs.push(crate::doc::Definition { name: format!("Imagem: {}", p.name), geom: image_geom(aspect), group: false, face_me: false, procedure: None, ifc: None, author: None, light: None });
                let def = self.doc.defs.len() - 1;
                self.set_tool(Tool::Select);
                self.image_place = Some(ImagePlace { def, aspect, p0: None });
                self.image_mat = Some(m);
                self.message = "Clique no primeiro canto da imagem e depois no canto oposto.".into();
            }
            1 => {
                self.checkpoint_keep_sel();
                if let Some((m, _)) = self.image_material(&p.name, p.bytes) {
                    self.cur_mat = Some(m);
                    self.edit_mat = Some(m);
                    self.set_tool(Tool::Paint);
                    self.message = "Textura criada — clique nas faces para pintar (ajuste com Textura > Posicionar).".into();
                }
            }
            _ => self.start_match_photo(&p.name, p.bytes),
        }
    }

    /// Two-click placement of an image entity.
    pub(crate) fn image_frame(&mut self, i: &In) -> bool {
        let Some(mut st) = self.image_place.clone() else { return false };
        let s = self.snap(i.px, st.p0.map(|p| p.0));
        match st.p0 {
            None => {
                self.overlay.squares.push((s.p, Color32::from_rgb(30, 110, 220)));
                if i.pressed {
                    let n = s.face.as_ref().map(|f| f.n).unwrap_or(DVec3::Z).normalize();
                    let (u, v) = plane_basis(n);
                    st.p0 = Some((s.p, u, v, n));
                }
            }
            Some((p0, u, v, n)) => {
                // project the cursor ray onto the image plane
                let (o, d) = self.cam.ray(i.px);
                let den = d.dot(n);
                let p = if den.abs() > 1e-9 { o + d * ((p0 - o).dot(n) / den) } else { s.p };
                let du = (p - p0).dot(u);
                let w = du.abs().max(((p - p0).dot(v)).abs() / st.aspect).max(0.01);
                let origin = if du < 0.0 { p0 - u * w } else { p0 };
                let corners = [origin, origin + u * w, origin + u * w + v * w * st.aspect, origin + v * w * st.aspect];
                for k in 0..4 {
                    self.overlay.lines.push((corners[k], corners[(k + 1) % 4], Color32::from_rgb(30, 110, 220), 2.0));
                }
                self.live_measure = Some(format!("{} × {}", crate::units::format_len(w), crate::units::format_len(w * st.aspect)));
                if i.pressed {
                    self.checkpoint_keep_sel();
                    let basis = DMat4::from_cols(u.extend(0.0), v.extend(0.0), n.extend(0.0), origin.extend(1.0));
                    let xf = self.ctx_xf().inverse() * basis * DMat4::from_scale(DVec3::splat(w));
                    let inst = crate::geom::Instance { def: st.def, xf, material: self.image_mat, name: String::new(), ..Default::default() };
                    let g = self.active_mut();
                    g.instances.push(inst);
                    let k = g.instances.len() - 1;
                    self.sel = crate::geom::Sel { insts: [k].into(), ..Default::default() };
                    self.image_place = None;
                    self.live_measure = None;
                    self.cache_dirty = true;
                    self.dirty = true;
                    self.message = "Imagem posicionada (use Mover, Rotacionar e Escala para ajustar).".into();
                    return true;
                }
            }
        }
        self.image_place = Some(st);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_rect_has_full_uvs() {
        let g = image_geom(0.5);
        assert_eq!(g.faces.len(), 1);
        let uvs = g.faces[0].uvs.as_ref().unwrap();
        assert_eq!(uvs.len(), 4);
        assert!(uvs.iter().any(|u| u[0] == 0.0 && u[1] == 1.0));
        assert!(uvs.iter().any(|u| u[0] == 1.0 && u[1] == 0.0));
    }
}
