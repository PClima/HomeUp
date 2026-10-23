//! Lights (Draw > Light): point bulbs, spots and panels. A light is a
//! component whose definition carries a `LightDef`; its small emissive geometry
//! shows where it is, and Move / Rotate / Copy work on it like on any object.
//! Lights inside a luminaire component follow every copy of it.

use crate::app::{HomeUpApp, Tool};
use crate::doc::{Definition, Document, Material};
use crate::geom::Geom;
use eframe::egui::{self, RichText};
use glam::{DMat4, DVec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LightKind {
    Point,
    Spot,
    Panel,
}

impl LightKind {
    pub fn label(self) -> &'static str {
        match self {
            LightKind::Point => "Luz pontual (lâmpada)",
            LightKind::Spot => "Spot (foco)",
            LightKind::Panel => "Painel / plafon",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LightDef {
    pub kind: LightKind,
    pub color: [u8; 3],
    /// Luminous flux (lm).
    pub lumens: f32,
    /// Spot: full beam angle (degrees) and soft edge (degrees).
    pub cone_deg: f32,
    pub soft_deg: f32,
    /// Bulb radius (m): bigger = softer shadows.
    pub radius: f32,
    /// Panel size (m).
    pub size: [f32; 2],
    /// Emitter position in the component (m).
    pub offset: [f64; 3],
}

impl LightDef {
    pub fn new(kind: LightKind) -> LightDef {
        match kind {
            LightKind::Point => LightDef { kind, color: [255, 214, 170], lumens: 800.0, cone_deg: 360.0, soft_deg: 0.0, radius: 0.04, size: [0.0, 0.0], offset: [0.0, 0.0, -0.08] },
            LightKind::Spot => LightDef { kind, color: [255, 220, 180], lumens: 500.0, cone_deg: 40.0, soft_deg: 12.0, radius: 0.02, size: [0.0, 0.0], offset: [0.0, 0.0, -0.02] },
            LightKind::Panel => LightDef { kind, color: [255, 244, 229], lumens: 2400.0, cone_deg: 180.0, soft_deg: 0.0, radius: 0.0, size: [0.6, 0.6], offset: [0.0, 0.0, -0.01] },
        }
    }
}

/// Color temperature presets (K → sRGB).
pub const TEMPS: [(&str, [u8; 3]); 4] = [("2700 K (quente)", [255, 200, 140]), ("3000 K", [255, 214, 170]), ("4000 K (neutra)", [255, 236, 214]), ("6500 K (fria)", [242, 246, 255])];

/// A light in world space.
#[derive(Clone, Copy, Debug)]
pub struct WorldLight {
    pub kind: LightKind,
    pub pos: DVec3,
    /// Emission direction (spot/panel): the component's −Z.
    pub dir: DVec3,
    /// Panel axes (half extents included).
    pub u: DVec3,
    pub v: DVec3,
    /// Linear RGB × flux (lm).
    pub flux: [f32; 3],
    pub cos_outer: f32,
    pub cos_inner: f32,
    pub radius: f32,
}

fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn collect(doc: &Document, g: &Geom, xf: DMat4, depth: usize, out: &mut Vec<WorldLight>) {
    if depth > 24 {
        return;
    }
    for inst in &g.instances {
        if !doc.inst_visible(inst) {
            continue;
        }
        let Some(d) = doc.defs.get(inst.def) else { continue };
        let m = xf * doc.eff_xf(inst);
        if let Some(l) = &d.light {
            let pos = m.transform_point3(DVec3::from(l.offset));
            let dir = m.transform_vector3(-DVec3::Z).normalize_or(-DVec3::Z);
            let (u, v) = (m.transform_vector3(DVec3::X * (l.size[0] as f64 * 0.5)), m.transform_vector3(DVec3::Y * (l.size[1] as f64 * 0.5)));
            let half = (l.cone_deg * 0.5).clamp(1.0, 90.0).to_radians();
            let inner = ((l.cone_deg * 0.5 - l.soft_deg).max(0.0)).to_radians().min(half);
            let lin = [srgb_to_linear(l.color[0]), srgb_to_linear(l.color[1]), srgb_to_linear(l.color[2])];
            out.push(WorldLight {
                kind: l.kind,
                pos,
                dir,
                u,
                v,
                flux: lin.map(|c| c * l.lumens.max(0.0)),
                cos_outer: half.cos(),
                cos_inner: inner.cos(),
                radius: l.radius.max(0.0),
            });
        }
        collect(doc, &d.geom, m, depth + 1, out);
    }
}

pub fn world_lights(doc: &Document) -> Vec<WorldLight> {
    let mut out = Vec::new();
    collect(doc, &doc.root, DMat4::IDENTITY, 0, &mut out);
    out
}

/// Real-time shader records: header (count) + 4 vec4 per light.
pub fn gpu_lights(doc: &Document) -> Vec<[f32; 4]> {
    let ls = world_lights(doc);
    let n = ls.len().min(64);
    let mut out = vec![[n as f32, 0.0, 0.0, 0.0]];
    for l in ls.iter().take(n) {
        let k = match l.kind {
            LightKind::Point => 0.0,
            LightKind::Spot => 1.0,
            LightKind::Panel => 2.0,
        };
        out.push([l.pos.x as f32, l.pos.y as f32, l.pos.z as f32, k]);
        out.push([l.dir.x as f32, l.dir.y as f32, l.dir.z as f32, l.cos_outer]);
        out.push([l.flux[0], l.flux[1], l.flux[2], l.cos_inner]);
        out.push([l.u.length() as f32, l.v.length() as f32, l.radius, 0.0]);
    }
    out
}

fn lamp_geom(kind: LightKind, l: &LightDef) -> Geom {
    let mut g = Geom::default();
    let c = DVec3::from(l.offset);
    match kind {
        LightKind::Point => {
            // small octahedron bulb
            let r = (l.radius as f64).max(0.02) * 1.2;
            let p = [c + DVec3::X * r, c - DVec3::X * r, c + DVec3::Y * r, c - DVec3::Y * r, c + DVec3::Z * r, c - DVec3::Z * r];
            for (a, b, t) in [(0, 2, 4), (2, 1, 4), (1, 3, 4), (3, 0, 4), (2, 0, 5), (1, 2, 5), (3, 1, 5), (0, 3, 5)] {
                let tri = [p[a], p[b], p[t]];
                let ctr = (tri[0] + tri[1] + tri[2]) / 3.0;
                g.add_polygon(&tri, None, false, c + (ctr - c) * 50.0);
            }
            // cord to the ceiling
            g.add_segments(&[(c + DVec3::Z * r, DVec3::ZERO)], None, false, DVec3::new(1.0, 1.0, 1.0));
        }
        LightKind::Spot => {
            let n = 16;
            let pts: Vec<DVec3> = (0..n).map(|k| {
                let a = k as f64 / n as f64 * std::f64::consts::TAU;
                DVec3::new(a.cos() * 0.045, a.sin() * 0.045, -0.01)
            }).collect();
            g.add_polygon(&pts, None, false, DVec3::new(0.0, 0.0, -10.0));
        }
        LightKind::Panel => {
            let (w, h) = (l.size[0] as f64 * 0.5, l.size[1] as f64 * 0.5);
            let z = -0.005;
            g.add_polygon(&[DVec3::new(-w, -h, z), DVec3::new(w, -h, z), DVec3::new(w, h, z), DVec3::new(-w, h, z)], None, false, DVec3::new(0.0, 0.0, -10.0));
        }
    }
    g
}

impl HomeUpApp {
    /// Draw > Light: a new light component, placed with the Place tool.
    pub(crate) fn new_light(&mut self, kind: LightKind) {
        let l = LightDef::new(kind);
        let mut m = Material::color(&format!("Lâmpada {}", self.doc.materials.len() + 1), l.color, 1.0);
        m.emissive = 1.0;
        self.doc.materials.push(m);
        let mat = self.doc.materials.len() - 1;
        let mut g = lamp_geom(kind, &l);
        for f in &mut g.faces {
            f.material = Some(mat);
        }
        for e in &mut g.edges {
            e.soft = true;
        }
        let n = self.doc.defs.iter().filter(|d| d.light.is_some()).count() + 1;
        self.doc.defs.push(Definition { name: format!("{} {n}", kind.label()), geom: g, group: false, face_me: false, procedure: None, ifc: None, author: None, light: Some(l) });
        let d = self.doc.defs.len() - 1;
        self.set_tool(Tool::Place(d));
        self.message = "Clique no teto (ou onde a luz deve ficar). Spots e painéis iluminam para baixo; gire para apontar.".into();
    }

    /// Entity Info section of a light component.
    pub(crate) fn light_ui(&mut self, ui: &mut egui::Ui, def: usize) {
        let Some(mut l) = self.doc.defs[def].light.clone() else { return };
        ui.separator();
        ui.label(RichText::new(l.kind.label()).strong());
        ui.horizontal(|ui| {
            ui.label("Cor");
            ui.color_edit_button_srgb(&mut l.color);
            egui::ComboBox::from_id_salt("light-temp").selected_text("Temperatura").show_ui(ui, |ui| {
                for (n, c) in TEMPS {
                    if ui.selectable_label(l.color == c, n).clicked() {
                        l.color = c;
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label("Fluxo");
            ui.add(egui::DragValue::new(&mut l.lumens).range(0.0..=100000.0).speed(10.0).suffix(" lm"));
            ui.weak(format!("≈ {:.0} W LED", l.lumens / 90.0));
        });
        match l.kind {
            LightKind::Spot => {
                ui.horizontal(|ui| {
                    ui.label("Abertura");
                    ui.add(egui::Slider::new(&mut l.cone_deg, 5.0..=170.0).suffix("°"));
                });
                ui.horizontal(|ui| {
                    ui.label("Borda suave");
                    ui.add(egui::Slider::new(&mut l.soft_deg, 0.0..=40.0).suffix("°"));
                });
            }
            LightKind::Panel => {
                ui.horizontal(|ui| {
                    ui.label("Tamanho");
                    ui.add(egui::DragValue::new(&mut l.size[0]).range(0.05..=10.0).speed(0.01).suffix(" m"));
                    ui.label("×");
                    ui.add(egui::DragValue::new(&mut l.size[1]).range(0.05..=10.0).speed(0.01).suffix(" m"));
                });
            }
            LightKind::Point => {}
        }
        if l.kind != LightKind::Panel {
            ui.horizontal(|ui| {
                ui.label("Raio da lâmpada");
                ui.add(egui::DragValue::new(&mut l.radius).range(0.0..=0.5).speed(0.002).suffix(" m"));
            });
        }
        ui.weak("Todas as cópias deste componente usam estas opções.");
        if Some(&l) != self.doc.defs[def].light.as_ref() {
            self.checkpoint_keep_sel();
            let resize = l.size != self.doc.defs[def].light.as_ref().map(|x| x.size).unwrap_or(l.size);
            let color = l.color;
            if resize {
                let mat = self.doc.defs[def].geom.faces.first().and_then(|f| f.material);
                let mut g = lamp_geom(l.kind, &l);
                for f in &mut g.faces {
                    f.material = mat;
                }
                for e in &mut g.edges {
                    e.soft = true;
                }
                self.doc.defs[def].geom = g;
            }
            if let Some(m) = self.doc.defs[def].geom.faces.first().and_then(|f| f.material) {
                self.doc.materials[m].color = color;
            }
            self.doc.defs[def].light = Some(l);
            self.dirty = true;
            self.cache_dirty = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lights_follow_their_instances() {
        let mut doc = Document::new();
        let l = LightDef::new(LightKind::Spot);
        doc.defs.push(Definition { name: "spot".into(), geom: lamp_geom(LightKind::Spot, &l), group: false, face_me: false, procedure: None, ifc: None, author: None, light: Some(l) });
        doc.root.instances.push(crate::geom::Instance { def: 0, xf: DMat4::from_translation(DVec3::new(1.0, 2.0, 2.7)), ..Default::default() });
        let w = world_lights(&doc);
        assert_eq!(w.len(), 1);
        assert!((w[0].pos - DVec3::new(1.0, 2.0, 2.68)).length() < 1e-9);
        assert!((w[0].dir + DVec3::Z).length() < 1e-9);
        assert!(w[0].cos_inner > w[0].cos_outer);
        assert_eq!(gpu_lights(&doc).len(), 5);
        assert!(!lamp_geom(LightKind::Point, &LightDef::new(LightKind::Point)).faces.is_empty());
    }
}
