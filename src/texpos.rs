//! Texture > Position: four pins on the face. Fixed-pin mode: red moves,
//! green scales/rotates, blue scales/shears, yellow distorts (perspective).
//! Free-pin mode: each pin is a tile corner moved on its own. The mapping is a
//! plane→uv homography stored as projective per-vertex UVs (uvs + uvq).

use crate::app::{HomeUpApp, In};
use eframe::egui::{self, Color32};
use glam::{DMat3, DVec2, DVec3};

#[derive(Clone, Debug)]
pub struct TexPos {
    pub face: usize,
    /// Face plane basis (local coordinates of the active context).
    pub fu: DVec3,
    pub fv: DVec3,
    pub origin: DVec3,
    /// Tile corners in plane coordinates: uv (0,0), (1,0), (0,1), (1,1).
    pub red: DVec2,
    pub green: DVec2,
    pub blue: DVec2,
    pub yellow: DVec2,
    pub free: bool,
    pub drag: Option<usize>,
}

/// Homography from the unit square to the quad p0 (0,0), p1 (1,0), p2 (1,1), p3 (0,1).
pub fn square_to_quad(p: [DVec2; 4]) -> DMat3 {
    let (x0, y0, x1, y1, x2, y2, x3, y3) = (p[0].x, p[0].y, p[1].x, p[1].y, p[2].x, p[2].y, p[3].x, p[3].y);
    let (dx1, dx2, dx3) = (x1 - x2, x3 - x2, x0 - x1 + x2 - x3);
    let (dy1, dy2, dy3) = (y1 - y2, y3 - y2, y0 - y1 + y2 - y3);
    let (g, h) = if dx3.abs() < 1e-12 && dy3.abs() < 1e-12 {
        (0.0, 0.0)
    } else {
        let det = dx1 * dy2 - dx2 * dy1;
        if det.abs() < 1e-18 {
            (0.0, 0.0)
        } else {
            ((dx3 * dy2 - dx2 * dy3) / det, (dx1 * dy3 - dx3 * dy1) / det)
        }
    };
    let (a, b, c) = (x1 - x0 + g * x1, x3 - x0 + h * x3, x0);
    let (d, e, f) = (y1 - y0 + g * y1, y3 - y0 + h * y3, y0);
    // column-major: columns are (a,d,g), (b,e,h), (c,f,1)
    DMat3::from_cols(DVec3::new(a, d, g), DVec3::new(b, e, h), DVec3::new(c, f, 1.0))
}

fn dehom(v: DVec3) -> DVec2 {
    DVec2::new(v.x, v.y) / if v.z.abs() < 1e-12 { 1e-12 } else { v.z }
}

impl TexPos {
    fn corners(&self) -> [DVec2; 4] {
        [self.red, self.green, self.yellow, self.blue]
    }
    /// uv → plane point.
    pub fn to_plane(&self, uv: DVec2) -> DVec2 {
        dehom(square_to_quad(self.corners()) * uv.extend(1.0))
    }
    /// Plane point → homogeneous uv (u·q, v·q, q) with q > 0 over the tile.
    pub fn uvq(&self, q: DVec2) -> [f64; 3] {
        let h = square_to_quad(self.corners()).inverse();
        let s = (h * self.red.extend(1.0)).z.signum();
        let r = h * q.extend(1.0) * if s == 0.0 { 1.0 } else { s };
        [r.x, r.y, r.z]
    }
}

/// Homography mapping 4 plane points to 4 uv points.
fn fit(plane: [DVec2; 4], uv: [DVec2; 4]) -> DMat3 {
    square_to_quad(uv) * square_to_quad(plane).inverse()
}

impl HomeUpApp {
    /// Current (projective) UVs of a face: (point, uv, q).
    fn face_uvs(&self, fi: usize) -> Option<Vec<(DVec3, [f32; 2], f32)>> {
        let g = self.active();
        let f = g.faces.get(fi)?;
        let m = f.material.and_then(|m| self.doc.materials.get(m))?;
        m.texture?;
        let xf = self.ctx_xf();
        let wn = xf.inverse().transpose().transform_vector3(f.normal).normalize_or_zero();
        let (pu, pv) = if wn.z.abs() > 0.9 {
            (DVec3::X, DVec3::Y)
        } else {
            let u = DVec3::Z.cross(wn).normalize_or_zero();
            (u, wn.cross(u))
        };
        let tile = m.tex_size;
        Some(
            f.loops()
                .flatten()
                .enumerate()
                .map(|(k, v)| {
                    let p = g.p(*v);
                    match f.uv_at(k) {
                        Some(uv) => (p, uv, f.uvq_at(k)),
                        None => {
                            let wp = xf.transform_point3(p);
                            (p, [(wp.dot(pu) / tile[0]) as f32, (-wp.dot(pv) / tile[1]) as f32], 1.0)
                        }
                    }
                })
                .collect(),
        )
    }

    pub(crate) fn start_texture_position(&mut self) {
        let Some(&fi) = self.sel.faces.iter().next() else { return };
        let Some(pts) = self.face_uvs(fi) else {
            self.message = "A face precisa de um material com textura.".into();
            return;
        };
        let g = self.active();
        let n = g.faces[fi].normal;
        let (fu, fv) = crate::geom::basis_for(n);
        let origin = pts[0].0;
        let q = |p: DVec3| DVec2::new((p - origin).dot(fu), (p - origin).dot(fv));
        let uv = |k: usize| DVec2::new(pts[k].1[0] as f64, pts[k].1[1] as f64) / pts[k].2.max(1e-6) as f64;
        let projective = pts.iter().any(|p| (p.2 - 1.0).abs() > 1e-6);
        // plane → uv map: homography from 4 vertices if projective, else affine from the largest triangle
        let h = if projective && pts.len() >= 4 {
            fit([q(pts[0].0), q(pts[1].0), q(pts[2].0), q(pts[3].0)], [uv(0), uv(1), uv(2), uv(3)])
        } else {
            let mut best = (0, 1, 2, 0.0);
            for i in 0..pts.len() {
                for j in i + 1..pts.len() {
                    for k in j + 1..pts.len() {
                        let a = (q(pts[j].0) - q(pts[i].0)).perp_dot(q(pts[k].0) - q(pts[i].0)).abs();
                        if a > best.3 {
                            best = (i, j, k, a);
                        }
                    }
                }
            }
            if best.3 < 1e-12 {
                return;
            }
            let (i, j, k) = (best.0, best.1, best.2);
            // complete to a parallelogram for the 4-point fit
            let (p4, u4) = (q(pts[j].0) + q(pts[k].0) - q(pts[i].0), uv(j) + uv(k) - uv(i));
            fit([q(pts[i].0), q(pts[j].0), p4, q(pts[k].0)], [uv(i), uv(j), u4, uv(k)])
        };
        let inv = h.inverse();
        let at = |t: DVec2| dehom(inv * t.extend(1.0));
        self.checkpoint_keep_sel();
        self.texpos = Some(TexPos {
            face: fi,
            fu,
            fv,
            origin,
            red: at(DVec2::ZERO),
            green: at(DVec2::X),
            blue: at(DVec2::Y),
            yellow: at(DVec2::ONE),
            free: false,
            drag: None,
        });
        // the face shows its texture (not the selection color) while positioning
        self.sel.clear();
        self.dirty = true;
        self.message = "Posicionar textura: vermelho move, verde escala/gira, azul escala/inclina, amarelo distorce.".into();
    }

    pub(crate) fn texpos_apply_pub(&mut self) {
        self.apply_texpos();
    }

    fn apply_texpos(&mut self) {
        let Some(t) = self.texpos.clone() else { return };
        let g = self.active_mut();
        let Some(f) = g.faces.get(t.face) else { return };
        let mut uvs = Vec::new();
        let mut qs = Vec::new();
        for v in f.loops().flatten() {
            let p = g.verts[*v as usize];
            let r = t.uvq(DVec2::new((p - t.origin).dot(t.fu), (p - t.origin).dot(t.fv)));
            uvs.push([r[0] as f32, r[1] as f32]);
            qs.push(r[2] as f32);
        }
        let affine = qs.iter().all(|q| (q - 1.0).abs() < 1e-5);
        let f = &mut g.faces[t.face];
        f.uvs = Some(uvs);
        f.uvq = if affine { None } else { Some(qs) };
        self.dirty = true;
        self.modified = true;
    }

    pub(crate) fn texpos_frame(&mut self, i: &In) -> bool {
        let Some(mut t) = self.texpos.clone() else { return false };
        let xf = self.ctx_xf();
        let to_w = |q: DVec2| xf.transform_point3(t.origin + t.fu * q.x + t.fv * q.y);
        // tile grid (straight lines stay straight under the homography)
        let grid = Color32::from_rgba_unmultiplied(60, 60, 60, 160);
        for k in -2..=3 {
            let k = k as f64;
            self.overlay.lines.push((to_w(t.to_plane(DVec2::new(-2.0, k))), to_w(t.to_plane(DVec2::new(3.0, k))), grid, 1.0));
            self.overlay.lines.push((to_w(t.to_plane(DVec2::new(k, -2.0))), to_w(t.to_plane(DVec2::new(k, 3.0))), grid, 1.0));
        }
        let pins = if t.free {
            let w = Color32::from_gray(250);
            [(t.red, w), (t.green, w), (t.blue, w), (t.yellow, w)]
        } else {
            [(t.red, Color32::from_rgb(220, 30, 30)), (t.green, Color32::from_rgb(20, 170, 40)), (t.blue, Color32::from_rgb(30, 70, 230)), (t.yellow, Color32::from_rgb(235, 190, 20))]
        };
        for (p, c) in pins {
            self.overlay.squares.push((to_w(p), c));
        }
        let (o, d) = self.cam.ray(i.px);
        let n = xf.transform_vector3(t.fu.cross(t.fv)).normalize();
        let p0 = to_w(DVec2::ZERO);
        let den = d.dot(n);
        let hit = (den.abs() > 1e-9).then(|| o + d * ((p0 - o).dot(n) / den)).map(|w| {
            let l = xf.inverse().transform_point3(w) - t.origin;
            DVec2::new(l.dot(t.fu), l.dot(t.fv))
        });
        if i.pressed {
            t.drag = pins.iter().position(|(p, _)| self.cam.project(to_w(*p)).is_some_and(|s| s.distance(i.px) < 10.0));
            if t.drag.is_none() {
                self.texpos = None;
                self.message = "Textura posicionada.".into();
                return true;
            }
        }
        if let (Some(g), Some(h)) = (t.drag, hit) {
            if i.primary_down {
                if t.free {
                    match g {
                        0 => t.red = h,
                        1 => t.green = h,
                        2 => t.blue = h,
                        _ => t.yellow = h,
                    }
                } else {
                    match g {
                        0 => {
                            let dv = h - t.red;
                            t.red += dv;
                            t.green += dv;
                            t.blue += dv;
                            t.yellow += dv;
                        }
                        1 => {
                            let (old, new) = (t.green - t.red, h - t.red);
                            if old.length() > 1e-9 && new.length() > 1e-9 {
                                let s = new.length() / old.length();
                                let rot = DVec2::from_angle(old.angle_to(new));
                                t.blue = t.red + rot.rotate(t.blue - t.red) * s;
                                t.yellow = t.red + rot.rotate(t.yellow - t.red) * s;
                                t.green = h;
                            }
                        }
                        2 => {
                            // affine change keeping red and green: blue → h
                            let (a, b, nb) = (t.green - t.red, t.blue - t.red, h - t.red);
                            let det = a.perp_dot(b);
                            if det.abs() > 1e-12 {
                                let y = t.yellow - t.red;
                                let (s, w) = (y.perp_dot(b) / det, a.perp_dot(y) / det);
                                t.yellow = t.red + a * s + nb * w;
                                t.blue = h;
                            }
                        }
                        _ => t.yellow = h,
                    }
                }
                self.texpos = Some(t.clone());
                self.apply_texpos();
            }
            if i.released {
                t.drag = None;
            }
        }
        if self.texpos.is_some() {
            self.texpos = Some(t);
        }
        true
    }

    /// Floating window with the right-click options of the Position Texture mode.
    pub(crate) fn texpos_window(&mut self, ctx: &egui::Context) {
        let Some(free) = self.texpos.as_ref().map(|t| t.free) else { return };
        let mut act = None;
        let mut free2 = free;
        egui::Window::new("Posicionar textura").collapsible(false).resizable(false).anchor(egui::Align2::LEFT_TOP, egui::vec2(90.0, 140.0)).show(ctx, |ui| {
            ui.checkbox(&mut free2, "Pinos livres (cada pino é um canto da imagem)");
            ui.horizontal(|ui| {
                if ui.button("Espelhar na horizontal").clicked() {
                    act = Some(0);
                }
                if ui.button("Espelhar na vertical").clicked() {
                    act = Some(1);
                }
            });
            ui.horizontal(|ui| {
                for (k, l) in [(2, "Girar 90°"), (3, "180°"), (4, "270°")] {
                    if ui.button(l).clicked() {
                        act = Some(k);
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Redefinir").clicked() {
                    act = Some(5);
                }
                if ui.button("Concluir").clicked() {
                    act = Some(6);
                }
            });
        });
        if free2 != free {
            if let Some(t) = self.texpos.as_mut() {
                t.free = free2;
            }
        }
        let Some(a) = act else { return };
        if a == 6 {
            self.texpos = None;
            return;
        }
        if a == 5 {
            if let Some(t) = &self.texpos {
                let f = t.face;
                let face = &mut self.active_mut().faces[f];
                face.uvs = None;
                face.uvq = None;
                self.dirty = true;
                self.texpos = None;
                self.sel = crate::geom::Sel { faces: [f].into(), ..Default::default() };
                self.start_texture_position();
            }
            return;
        }
        if let Some(t) = self.texpos.as_mut() {
            let (r, g, b, y) = (t.red, t.green, t.blue, t.yellow);
            match a {
                0 => (t.red, t.green, t.blue, t.yellow) = (g, r, y, b),
                1 => (t.red, t.green, t.blue, t.yellow) = (b, y, r, g),
                k => {
                    // rotate the tile corners by 90° steps (counter-clockwise in uv)
                    let mut c = [r, g, y, b];
                    for _ in 0..(k - 1) {
                        c = [c[3], c[0], c[1], c[2]];
                    }
                    (t.red, t.green, t.yellow, t.blue) = (c[0], c[1], c[2], c[3]);
                }
            }
        }
        self.apply_texpos();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tp(red: DVec2, green: DVec2, blue: DVec2, yellow: DVec2) -> TexPos {
        TexPos { face: 0, fu: DVec3::X, fv: DVec3::Y, origin: DVec3::ZERO, red, green, blue, yellow, free: false, drag: None }
    }
    #[test]
    fn affine_and_perspective_pins() {
        let t = tp(DVec2::new(1.0, 1.0), DVec2::new(3.0, 1.0), DVec2::new(1.0, 2.0), DVec2::new(3.0, 2.0));
        let uv = |t: &TexPos, p: DVec2| {
            let r = t.uvq(p);
            DVec2::new(r[0] / r[2], r[1] / r[2])
        };
        assert!(uv(&t, DVec2::new(2.0, 3.0)).distance(DVec2::new(0.5, 2.0)) < 1e-9);
        assert!((t.uvq(DVec2::new(2.0, 3.0))[2] - 1.0).abs() < 1e-9, "affine pins give q = 1");
        // distorted: corners still land on the tile corners
        let d = tp(DVec2::ZERO, DVec2::new(2.0, 0.0), DVec2::new(0.0, 1.0), DVec2::new(1.4, 1.2));
        for (p, u) in [(d.red, DVec2::ZERO), (d.green, DVec2::X), (d.blue, DVec2::Y), (d.yellow, DVec2::ONE)] {
            assert!(uv(&d, p).distance(u) < 1e-9, "{p:?}");
        }
        assert!(d.to_plane(DVec2::ONE).distance(d.yellow) < 1e-9);
    }
}
