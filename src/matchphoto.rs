//! Match Photo: align the camera to a photo with two vanishing points (red and
//! green bars), an axis origin and a blue-axis scale grip; the photo shows
//! behind the model in its scene and can be projected onto faces as texture.
//!
//! Calibration (principal point at the image center, square pixels): with
//! vanishing points v1, v2 in height-normalized image coordinates,
//! f² = −(v1·v2); the red/green axes point along (v, f) in camera space and
//! blue = green × red (camera space is left-handed).

use crate::app::{HomeUpApp, In};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatchPhoto {
    /// Index into `Document::photos`.
    pub image: usize,
    /// Bars as pairs of image points, normalized: x in 0..aspect, y in 0..1 (top = 0).
    pub red: [[[f64; 2]; 2]; 2],
    pub green: [[[f64; 2]; 2]; 2],
    pub origin: [f64; 2],
    /// Blue-axis grip (a point straight above the origin in the photo).
    pub blue: [f64; 2],
    /// Real height of the blue grip above the origin (m).
    pub height: f64,
    pub fov: f64,
    /// Camera roll recovered from the vanishing points.
    #[serde(default)]
    pub roll: f64,
}

pub struct Solved {
    pub eye: DVec3,
    pub forward: DVec3,
    /// Camera up direction (world).
    pub up: DVec3,
    pub fov_y: f64,
}

/// Roll of a camera looking along `forward` whose up vector is `up`
/// (matches `Camera::view_up`).
pub fn roll_of(forward: DVec3, up: DVec3) -> f64 {
    let r0 = forward.cross(DVec3::Z).normalize_or(DVec3::X);
    let u0 = r0.cross(forward);
    (-(up.dot(r0))).atan2(up.dot(u0))
}

fn line_intersection(a: [[f64; 2]; 2], b: [[f64; 2]; 2]) -> Option<DVec2> {
    let (p, r) = (DVec2::from(a[0]), DVec2::from(a[1]) - DVec2::from(a[0]));
    let (q, s) = (DVec2::from(b[0]), DVec2::from(b[1]) - DVec2::from(b[0]));
    let den = r.perp_dot(s);
    if den.abs() < 1e-12 {
        return None;
    }
    Some(p + r * ((q - p).perp_dot(s) / den))
}

impl MatchPhoto {
    pub fn new(image: usize, aspect: f64) -> MatchPhoto {
        // a plausible two-point start: bars converging left and right of the frame
        let a = aspect;
        MatchPhoto {
            image,
            red: [[[0.12 * a, 0.62], [0.42 * a, 0.70]], [[0.12 * a, 0.35], [0.42 * a, 0.30]]],
            green: [[[0.58 * a, 0.70], [0.88 * a, 0.62]], [[0.58 * a, 0.30], [0.88 * a, 0.35]]],
            origin: [0.5 * a, 0.8],
            blue: [0.5 * a, 0.4],
            height: 3.0,
            fov: 35f64.to_radians(),
            roll: 0.0,
        }
    }

    /// Camera from the bars (None when the vanishing points are inconsistent).
    pub fn solve(&self, aspect: f64) -> Option<Solved> {
        // centered coordinates, y up
        let c = |p: DVec2| DVec2::new(p.x - aspect * 0.5, 0.5 - p.y);
        let v1 = c(line_intersection(self.red[0], self.red[1])?);
        let v2 = c(line_intersection(self.green[0], self.green[1])?);
        let f2 = -v1.dot(v2);
        if f2 <= 1e-6 {
            return None;
        }
        let f = f2.sqrt();
        let r1 = DVec3::new(v1.x, v1.y, f).normalize();
        let mut r2 = DVec3::new(v2.x, v2.y, f).normalize();
        r2 = (r2 - r1 * r1.dot(r2)).normalize();
        // camera space (x right, y up, z forward) is left-handed, so blue = green × red
        let mut r3 = r2.cross(r1);
        if r3.y < 0.0 {
            // keep the blue axis pointing up in the photo (flip green to stay right-handed in the world)
            r2 = -r2;
            r3 = r2.cross(r1);
        }
        let o = c(DVec2::from(self.origin));
        let ro = DVec3::new(o.x, o.y, f).normalize();
        // the blue grip fixes the distance: proj(D·ro + h·r3) = b
        let b = c(DVec2::from(self.blue));
        let sx = (b.x * ro.z - f * ro.x) / (f * r3.x - b.x * r3.z);
        let sy = (b.y * ro.z - f * ro.y) / (f * r3.y - b.y * r3.z);
        let s = if (f * r3.y - b.y * r3.z).abs() > (f * r3.x - b.x * r3.z).abs() { sy } else { sx };
        if !(s.is_finite() && s > 1e-6) {
            return None;
        }
        let d = self.height / s;
        // camera coords of world axes are r1, r2, r3 (columns); world = Rᵀ·camera
        let to_world = |v: DVec3| DVec3::new(r1.dot(v), r2.dot(v), r3.dot(v));
        let eye = -to_world(ro * d);
        let forward = to_world(DVec3::Z).normalize();
        let up = to_world(DVec3::Y).normalize();
        Some(Solved { eye, forward, up, fov_y: 2.0 * (0.5 / f).atan() })
    }
}

/// Grips: 0..8 bar ends (red 0..4, green 4..8), 8 origin, 9 blue.
fn grip_points(mp: &MatchPhoto) -> [[f64; 2]; 10] {
    [mp.red[0][0], mp.red[0][1], mp.red[1][0], mp.red[1][1], mp.green[0][0], mp.green[0][1], mp.green[1][0], mp.green[1][1], mp.origin, mp.blue]
}

fn set_grip(mp: &mut MatchPhoto, k: usize, p: [f64; 2]) {
    match k {
        0..=3 => mp.red[k / 2][k % 2] = p,
        4..=7 => mp.green[(k - 4) / 2][k % 2] = p,
        8 => {
            // the blue grip follows the origin
            let d = [mp.blue[0] - mp.origin[0], mp.blue[1] - mp.origin[1]];
            mp.origin = p;
            mp.blue = [p[0] + d[0], p[1] + d[1]];
        }
        _ => mp.blue = p,
    }
}

impl HomeUpApp {
    pub(crate) fn start_match_photo(&mut self, name: &str, bytes: Vec<u8>) {
        match crate::tex::TextureData::from_bytes(name, bytes) {
            Ok(t) => {
                self.checkpoint_keep_sel();
                let aspect = t.w as f64 / t.h.max(1) as f64;
                self.doc.photos.push(t);
                let mp = MatchPhoto::new(self.doc.photos.len() - 1, aspect);
                self.doc.scenes.push(crate::doc::Scene { name: format!("Foto: {name}"), cam: self.cam.to_array(), ortho: false, photo: Some(mp) });
                let k = self.doc.scenes.len() - 1;
                self.photo_scene = Some(k);
                self.scene_sel = k;
                self.mp_edit = true;
                self.cam.ortho = false;
                self.open_panel = Some(crate::chrome::PanelKind::MatchPhoto);
                self.show_tray = true;
                self.apply_match_photo();
                self.message = "Combinar foto: arraste as barras vermelhas e verdes até linhas paralelas da foto, depois a origem e o eixo azul.".into();
            }
            Err(e) => self.message = e,
        }
    }

    fn photo_aspect(&self, mp: &MatchPhoto) -> f64 {
        self.doc.photos.get(mp.image).map(|t| t.w as f64 / t.h.max(1) as f64).unwrap_or(1.5)
    }

    pub(crate) fn current_photo(&self) -> Option<&MatchPhoto> {
        self.photo_scene.and_then(|k| self.doc.scenes.get(k)).and_then(|s| s.photo.as_ref())
    }

    /// Put the camera where the photo was taken and remember it in the scene.
    pub(crate) fn apply_match_photo(&mut self) {
        let Some(k) = self.photo_scene else { return };
        let Some(mp) = self.doc.scenes.get(k).and_then(|s| s.photo.clone()) else { return };
        let aspect = self.photo_aspect(&mp);
        let Some(s) = mp.solve(aspect) else {
            self.message = "As barras não definem pontos de fuga válidos (as vermelhas e as verdes devem convergir em lados opostos).".into();
            return;
        };
        let dist = s.eye.length().max(0.5);
        self.cam.target = s.eye + s.forward * dist;
        self.cam.dist = dist;
        self.cam.pitch = (-s.forward.z).clamp(-1.0, 1.0).asin();
        self.cam.yaw = (-s.forward.y).atan2(-s.forward.x);
        self.cam.fov_y = s.fov_y.clamp(0.05, 2.8);
        self.cam.roll = roll_of(s.forward, s.up);
        self.cam.ortho = false;
        self.cam_anim = None;
        let sc = &mut self.doc.scenes[k];
        sc.cam = self.cam.to_array();
        if let Some(p) = sc.photo.as_mut() {
            p.fov = self.cam.fov_y;
            p.roll = self.cam.roll;
        }
        self.photo_cam = Some(self.cam.to_array());
    }

    /// Leave the photo view when the user moves the camera away from it.
    pub(crate) fn track_photo_scene(&mut self) {
        let Some(c) = self.photo_cam else { return };
        if self.mp_edit || self.cam_anim.is_some() {
            return;
        }
        let now = self.cam.to_array();
        if now.iter().zip(c.iter()).any(|(a, b)| (a - b).abs() > 1e-6) {
            self.photo_scene = None;
            self.photo_cam = None;
            self.cam.roll = 0.0;
        }
    }

    /// Entering a scene with a photo (from tabs or the Scenes panel).
    pub(crate) fn enter_photo_scene(&mut self, k: usize) {
        let Some(mp) = self.doc.scenes.get(k).and_then(|s| s.photo.clone()) else {
            self.photo_scene = None;
            self.photo_cam = None;
            return;
        };
        self.photo_scene = Some(k);
        self.cam.set_array(self.doc.scenes[k].cam);
        self.cam.fov_y = mp.fov;
        self.cam.roll = mp.roll;
        self.cam.ortho = false;
        self.cam_anim = None;
        self.photo_cam = Some(self.cam.to_array());
    }

    /// Photo rectangle in the viewport (fitted to its height, centered).
    pub(crate) fn photo_rect(&self, vp: Rect) -> Option<(Rect, f64)> {
        let mp = self.current_photo()?;
        let aspect = self.photo_aspect(mp);
        let h = vp.height();
        let w = h * aspect as f32;
        Some((Rect::from_center_size(vp.center(), Vec2::new(w, h)), aspect))
    }

    /// Grip dragging while editing the match.
    pub(crate) fn match_photo_frame(&mut self, i: &In) -> bool {
        if !self.mp_edit {
            return false;
        }
        let Some(k) = self.photo_scene else {
            self.mp_edit = false;
            return false;
        };
        let Some((r, _)) = self.photo_rect(self.viewport_rect) else { return false };
        let to_img = |p: Pos2| [((p.x - r.min.x) / r.height()) as f64, ((p.y - r.min.y) / r.height()) as f64];
        let to_scr = |q: [f64; 2]| Pos2::new(r.min.x + q[0] as f32 * r.height(), r.min.y + q[1] as f32 * r.height());
        let mp = self.doc.scenes[k].photo.clone().unwrap();
        if i.pressed {
            self.mp_grip = grip_points(&mp).iter().position(|g| to_scr(*g).distance(i.pos) < 9.0);
        }
        if let Some(g) = self.mp_grip {
            if i.primary_down {
                let mut m = mp.clone();
                set_grip(&mut m, g, to_img(i.pos));
                self.doc.scenes[k].photo = Some(m);
                self.apply_match_photo();
                self.modified = true;
            }
            if i.released {
                self.mp_grip = None;
            }
        }
        true
    }

    /// Bars, axes and grips drawn over the photo while editing.
    pub(crate) fn paint_match_photo(&self, painter: &egui::Painter, vp: Rect) {
        if !self.mp_edit {
            return;
        }
        let Some(mp) = self.current_photo() else { return };
        let Some((r, aspect)) = self.photo_rect(vp) else { return };
        let to_scr = |q: [f64; 2]| Pos2::new(r.min.x + q[0] as f32 * r.height(), r.min.y + q[1] as f32 * r.height());
        let red = Color32::from_rgb(220, 30, 30);
        let green = Color32::from_rgb(20, 160, 40);
        for (bars, c) in [(mp.red, red), (mp.green, green)] {
            for b in bars {
                let (a, z) = (to_scr(b[0]), to_scr(b[1]));
                // extend the bar faintly toward the vanishing point
                let d = (z - a).normalized() * 4000.0;
                crate::app::dashed(painter, a - d, z + d, Stroke::new(1.0, c.gamma_multiply(0.5)), 6.0, 5.0, vp);
                painter.line_segment([a, z], Stroke::new(2.5, c));
                for p in [a, z] {
                    painter.rect_filled(Rect::from_center_size(p, Vec2::splat(9.0)), 1.0, c);
                }
            }
        }
        let (o, b) = (to_scr(mp.origin), to_scr(mp.blue));
        painter.line_segment([o, b], Stroke::new(2.5, Color32::from_rgb(30, 60, 230)));
        painter.rect_filled(Rect::from_center_size(b, Vec2::splat(9.0)), 1.0, Color32::from_rgb(30, 60, 230));
        painter.circle_filled(o, 6.0, Color32::from_rgb(250, 200, 30));
        if mp.solve(aspect).is_none() {
            painter.text(r.left_top() + Vec2::new(10.0, 10.0), egui::Align2::LEFT_TOP, "Pontos de fuga inválidos", egui::FontId::proportional(14.0), red);
        }
        painter.text(vp.left_top() + Vec2::new(10.0, 8.0), egui::Align2::LEFT_TOP, "Combinar foto", egui::FontId::proportional(15.0), Color32::from_rgb(20, 40, 90));
    }

    /// Project the photo onto the visible faces of the active context as a texture.
    pub(crate) fn project_photo_textures(&mut self) {
        let Some(mp) = self.current_photo().cloned() else { return };
        let Some(t) = self.doc.photos.get(mp.image).cloned() else { return };
        let aspect = t.w as f64 / t.h.max(1) as f64;
        self.checkpoint_keep_sel();
        self.doc.textures.push(t.clone());
        let mut m = crate::doc::Material::color(&format!("Foto {}", t.name), t.average(), 1.0);
        m.texture = Some(self.doc.textures.len() - 1);
        m.source = "foto combinada".into();
        self.doc.materials.push(m);
        let mi = self.doc.materials.len() - 1;
        let mut cam = self.cam.clone();
        cam.size = DVec2::new(1000.0 * aspect, 1000.0);
        let vp = cam.view_proj();
        let xf = self.ctx_xf();
        let eye = cam.eye();
        let only_sel = !self.sel.faces.is_empty();
        let sel = self.sel.faces.clone();
        let g = self.active_mut();
        let mut n = 0;
        for fi in 0..g.faces.len() {
            if only_sel && !sel.contains(&fi) {
                continue;
            }
            let f = &g.faces[fi];
            let wn = xf.transform_vector3(f.normal);
            let c = xf.transform_point3(g.p(f.outer[0]));
            if wn.dot(eye - c) <= 0.0 {
                continue; // facing away from the photo
            }
            // exact perspective projection: (x/2 + w/2, w/2 − y/2) / w, stored projectively
            let mut uvs = Vec::new();
            let mut qs = Vec::new();
            for v in f.loops().flatten() {
                let c = vp * xf.transform_point3(g.p(*v)).extend(1.0);
                let w = c.w.max(1e-6);
                uvs.push([(0.5 * c.x + 0.5 * w) as f32, (0.5 * w - 0.5 * c.y) as f32]);
                qs.push(w as f32);
            }
            let f = &mut g.faces[fi];
            f.uvs = Some(uvs);
            f.uvq = Some(qs);
            f.material = Some(mi);
            n += 1;
        }
        self.dirty = true;
        self.message = format!("Foto projetada em {n} faces.");
    }

    pub(crate) fn match_photo_ui(&mut self, ui: &mut egui::Ui) {
        let photos: Vec<usize> = (0..self.doc.scenes.len()).filter(|k| self.doc.scenes[*k].photo.is_some()).collect();
        if ui.button("Combinar nova foto…").clicked() {
            if let Some(path) = rfd::FileDialog::new().add_filter("Imagem", &crate::imageimport::IMAGE_EXTS).pick_file() {
                match std::fs::read(&path) {
                    Ok(b) => self.start_match_photo(&path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), b),
                    Err(e) => self.message = e.to_string(),
                }
            }
        }
        if photos.is_empty() {
            ui.weak("Nenhuma foto combinada. Use uma foto tirada a ~45° de um canto, sem recorte.");
            return;
        }
        for k in &photos {
            let name = self.doc.scenes[*k].name.clone();
            if ui.selectable_label(self.photo_scene == Some(*k), name).clicked() {
                self.enter_photo_scene(*k);
                self.scene_sel = *k;
            }
        }
        let Some(k) = self.photo_scene else {
            ui.weak("Clique em uma foto acima (ou na aba da cena) para voltar à vista combinada.");
            return;
        };
        ui.separator();
        ui.checkbox(&mut self.mp_edit, "Editar combinação (barras e eixos)");
        ui.checkbox(&mut self.mp_hide_model, "Ocultar o modelo enquanto alinha");
        if let Some(mp) = self.doc.scenes[k].photo.as_mut() {
            ui.horizontal(|ui| {
                ui.label("Altura do eixo azul:");
                if ui.add(egui::DragValue::new(&mut mp.height).range(0.01..=1000.0).speed(0.05).suffix(" m")).changed() {
                    self.modified = true;
                }
            });
        }
        if ui.button("Atualizar câmera").clicked() {
            self.apply_match_photo();
        }
        if ui.button("Projetar texturas da foto").on_hover_text("Nas faces selecionadas, ou em todas as voltadas para a câmera").clicked() {
            self.project_photo_textures();
        }
        if ui.button("Concluído").clicked() {
            self.mp_edit = false;
            self.mp_hide_model = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::Camera;

    /// Project world points with a known camera, build bars from them and check
    /// that the solver recovers the camera.
    #[test]
    fn recovers_a_known_camera() {
        let aspect = 1.5;
        let mut cam = Camera { size: DVec2::new(1500.0, 1000.0), fov_y: 50f64.to_radians(), ..Camera::default() };
        cam.target = DVec3::new(1.0, 1.0, 1.0);
        cam.yaw = (-135f64).to_radians();
        cam.pitch = 15f64.to_radians();
        cam.dist = 12.0;
        cam.roll = 4f64.to_radians();
        let proj = |p: DVec3| {
            let s = cam.project(p).unwrap();
            [s.x / 1000.0, s.y / 1000.0]
        };
        let mp = MatchPhoto {
            image: 0,
            red: [[proj(DVec3::new(0.0, 0.0, 0.0)), proj(DVec3::new(4.0, 0.0, 0.0))], [proj(DVec3::new(0.0, 0.0, 3.0)), proj(DVec3::new(4.0, 0.0, 3.0))]],
            green: [[proj(DVec3::new(0.0, 0.0, 0.0)), proj(DVec3::new(0.0, 4.0, 0.0))], [proj(DVec3::new(0.0, 0.0, 3.0)), proj(DVec3::new(0.0, 4.0, 3.0))]],
            origin: proj(DVec3::ZERO),
            blue: proj(DVec3::new(0.0, 0.0, 3.0)),
            height: 3.0,
            fov: 0.6,
            roll: 0.0,
        };
        let s = mp.solve(aspect).expect("solvable");
        assert!((s.fov_y - cam.fov_y).abs() < 1e-3, "fov {} vs {}", s.fov_y, cam.fov_y);
        assert!(s.eye.distance(cam.eye()) < 1e-3, "eye {:?} vs {:?}", s.eye, cam.eye());
        assert!(s.forward.distance(cam.forward()) < 1e-4);
        assert!((roll_of(s.forward, s.up) - cam.roll).abs() < 1e-6, "roll {}", roll_of(s.forward, s.up));
    }
}
