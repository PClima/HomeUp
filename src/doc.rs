//! Document: root geometry, group/component definitions, materials, guides, scenes.

use crate::geom::{Geom, Instance, Sel};
use glam::{DMat4, DVec3};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    pub color: [u8; 3],
    pub alpha: f32,
    /// Index into `Document::textures`.
    #[serde(default)]
    pub texture: Option<usize>,
    /// Real size of one texture tile, meters (width, height).
    #[serde(default = "unit_tile")]
    pub tex_size: [f64; 2],
    /// Where the material came from (credit line), e.g. "ambientCG (CC0)".
    #[serde(default)]
    pub source: String,
    // ---- photoreal (PBR) options
    #[serde(default)]
    pub metalness: f32,
    #[serde(default = "default_rough")]
    pub roughness: f32,
    #[serde(default)]
    pub invert_rough: bool,
    #[serde(default = "one_f32")]
    pub normal_scale: f32,
    #[serde(default = "one_f32")]
    pub ao_strength: f32,
    #[serde(default)]
    pub normal_tex: Option<usize>,
    #[serde(default)]
    pub rough_tex: Option<usize>,
    #[serde(default)]
    pub metal_tex: Option<usize>,
    #[serde(default)]
    pub ao_tex: Option<usize>,
    /// Channel (0..3) read from each map (packed glTF maps use G = roughness, B = metal).
    #[serde(default)]
    pub rough_ch: u8,
    #[serde(default)]
    pub metal_ch: u8,
    #[serde(default)]
    pub ao_ch: u8,
    /// Section-fill hatch for objects with this material.
    #[serde(default)]
    pub hatch: Option<crate::hatch::Hatch>,
    /// Self-lit (lamp surfaces): drawn at full brightness.
    #[serde(default)]
    pub emissive: f32,
}

fn default_rough() -> f32 {
    0.85
}

fn one_f32() -> f32 {
    1.0
}

fn unit_tile() -> [f64; 2] {
    [1.0, 1.0]
}

impl Material {
    pub fn color(name: &str, color: [u8; 3], alpha: f32) -> Material {
        Material {
            name: name.into(),
            color,
            alpha,
            texture: None,
            tex_size: unit_tile(),
            source: String::new(),
            metalness: 0.0,
            roughness: default_rough(),
            invert_rough: false,
            normal_scale: 1.0,
            ao_strength: 1.0,
            normal_tex: None,
            rough_tex: None,
            metal_tex: None,
            ao_tex: None,
            rough_ch: 0,
            metal_ch: 0,
            ao_ch: 0,
            hatch: None,
            emissive: 0.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Definition {
    pub name: String,
    pub geom: Geom,
    /// Groups are single-use definitions; components may have many instances.
    pub group: bool,
    /// Always turns to face the camera around its vertical axis (scale figures).
    #[serde(default)]
    pub face_me: bool,
    /// Procedural component (Copy Along): its copies are regenerated from the path.
    #[serde(default)]
    pub procedure: Option<crate::procedural::CopyAlong>,
    /// Classifier: IFC entity type (e.g. "IfcWall").
    #[serde(default)]
    pub ifc: Option<String>,
    /// Credits: who made this component.
    #[serde(default)]
    pub author: Option<String>,
    /// Light component (bulb, spot, panel).
    #[serde(default)]
    pub light: Option<crate::lights::LightDef>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Guide {
    Line { p: DVec3, dir: DVec3 },
    Point { p: DVec3 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dimension {
    pub a: DVec3,
    pub b: DVec3,
    /// Offset of the dimension line from the measured points.
    pub off: DVec3,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Label {
    pub anchor: DVec3,
    pub pos: DVec3,
    pub text: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SectionPlane {
    pub p: DVec3,
    /// Geometry on the side this normal points to is cut away.
    pub n: DVec3,
    pub active: bool,
    #[serde(default)]
    pub name: String,
    /// Plane symbol hidden (the cut still applies while active).
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ModelAxes {
    pub origin: DVec3,
    pub x: DVec3,
    pub y: DVec3,
    pub z: DVec3,
}

impl Default for ModelAxes {
    fn default() -> Self {
        ModelAxes { origin: DVec3::ZERO, x: DVec3::X, y: DVec3::Y, z: DVec3::Z }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scene {
    pub name: String,
    /// target xyz, yaw, pitch, dist
    pub cam: [f64; 6],
    pub ortho: bool,
    /// Matched photo shown behind the model in this scene.
    #[serde(default)]
    pub photo: Option<crate::matchphoto::MatchPhoto>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Document {
    pub root: Geom,
    #[serde(default)]
    pub defs: Vec<Definition>,
    #[serde(default)]
    pub materials: Vec<Material>,
    #[serde(default)]
    pub textures: Vec<crate::tex::TextureData>,
    #[serde(default)]
    pub guides: Vec<Guide>,
    #[serde(default)]
    pub scenes: Vec<Scene>,
    #[serde(default)]
    pub dims: Vec<Dimension>,
    #[serde(default)]
    pub labels: Vec<Label>,
    #[serde(default)]
    pub sections: Vec<SectionPlane>,
    #[serde(default)]
    pub axes: ModelAxes,
    #[serde(default)]
    pub tags: Vec<crate::settings::Tag>,
    #[serde(default)]
    pub style: crate::settings::Style,
    #[serde(default)]
    pub shadows: crate::settings::Shadows,
    #[serde(default)]
    pub location: crate::settings::Location,
    #[serde(default)]
    pub units: crate::settings::UnitsCfg,
    #[serde(default)]
    pub env: crate::settings::Environment,
    /// Environments stored in the model (Environments panel).
    #[serde(default)]
    pub envs: Vec<crate::settings::Environment>,
    /// Panoramas used by the environments (kept apart from face textures).
    #[serde(default)]
    pub env_images: Vec<crate::tex::TextureData>,
    /// Matched photos (Match Photo scenes).
    #[serde(default)]
    pub photos: Vec<crate::tex::TextureData>,
    /// Credits: authors who claimed the model.
    #[serde(default)]
    pub credits: Vec<String>,
    /// Imported .PAT hatch patterns.
    #[serde(default)]
    pub hatch_pats: Vec<crate::hatch::PatPattern>,
    /// Images used by style watermarks.
    #[serde(default)]
    pub watermark_images: Vec<crate::tex::TextureData>,
    /// Show hidden objects / hide the rest of the model while editing (display, not saved).
    #[serde(skip)]
    pub show_hidden: bool,
    #[serde(skip)]
    pub hide_rest: bool,
    /// Camera yaw, used to orient face-me components (not saved).
    #[serde(skip)]
    pub view_yaw: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// Geometry of the active editing context.
    Active,
    /// Content of instance `i` of the active context.
    Inst(usize),
    /// Everything outside the active context.
    Outside,
}

pub struct VisitItem<'a> {
    pub geom: &'a Geom,
    pub xf: DMat4,
    pub owner: Owner,
    /// Material inherited from enclosing instances.
    pub mat: Option<usize>,
    /// Tag inherited from enclosing instances.
    pub tag: Option<usize>,
    /// False when an enclosing instance does not cast shadows.
    pub cast: bool,
}

impl Document {
    pub fn new() -> Self {
        let m = Material::color;
        Document {
            materials: vec![
                m("Branco", [245, 245, 240], 1.0),
                m("Tijolo", [168, 74, 52], 1.0),
                m("Madeira", [176, 128, 78], 1.0),
                m("Concreto", [160, 160, 156], 1.0),
                m("Grama", [96, 150, 64], 1.0),
                m("Telha", [140, 60, 40], 1.0),
                m("Azul", [70, 110, 190], 1.0),
                m("Grafite", [60, 60, 64], 1.0),
                m("Vidro", [150, 195, 225], 0.35),
            ],
            ..Default::default()
        }
    }

    /// New model with the default scale figure (a 1.75 m face-me person).
    pub fn with_scale_figure() -> Self {
        let mut d = Document::new();
        d.add_scale_figure();
        d
    }

    pub fn add_scale_figure(&mut self) {
        // right half of the silhouette (x, z), from the top of the head down to the crotch
        let mut half: Vec<(f64, f64)> = Vec::new();
        for k in 0..=7 {
            let a = (90.0 - k as f64 * 20.0f64).to_radians();
            half.push((0.115 * a.cos(), 1.62 + 0.115 * a.sin()));
        }
        half.extend([
            (0.05, 1.49),
            (0.20, 1.44),
            (0.25, 1.36),
            (0.27, 0.88),
            (0.23, 0.83),
            (0.20, 1.18),
            (0.18, 1.00),
            (0.17, 0.86),
            (0.15, 0.05),
            (0.19, 0.0),
            (0.03, 0.0),
            (0.03, 0.76),
        ]);
        let mut outline: Vec<DVec3> = half.iter().map(|(x, z)| DVec3::new(*x, 0.0, *z)).collect();
        outline.push(DVec3::new(0.0, 0.0, 0.79));
        outline.extend(half.iter().rev().filter(|(x, _)| x.abs() > 1e-9).map(|(x, z)| DVec3::new(-*x, 0.0, *z)));
        let mut g = Geom::default();
        g.add_polygon(&outline, None, false, DVec3::new(0.0, -10.0, 1.0));
        let mat = self.materials.len();
        self.materials.push(Material::color("Figura de escala", [96, 116, 150], 1.0));
        self.defs.push(Definition { name: "Pessoa (escala)".into(), geom: g, group: false, face_me: true, procedure: None, ifc: None, author: None, light: None });
        let def = self.defs.len() - 1;
        self.root.instances.push(Instance {
            def,
            xf: DMat4::from_translation(DVec3::new(-1.0, 0.4, 0.0)),
            material: Some(mat),
            name: String::new(),
            ..Default::default()
        });
    }

    /// Instance transform, with face-me components turned toward the camera.
    pub fn eff_xf(&self, inst: &Instance) -> DMat4 {
        match self.defs.get(inst.def) {
            Some(d) if d.face_me => inst.xf * DMat4::from_rotation_z(self.view_yaw + std::f64::consts::FRAC_PI_2),
            _ => inst.xf,
        }
    }

    pub fn has_face_me(&self) -> bool {
        self.defs.iter().any(|d| d.face_me)
    }

    /// Geometry of the context designated by `path` (instance indices from the root).
    pub fn geom(&self, path: &[usize]) -> &Geom {
        let mut g = &self.root;
        for &i in path {
            g = &self.defs[g.instances[i].def].geom;
        }
        g
    }

    pub fn def_at(&self, path: &[usize]) -> Option<usize> {
        let (last, parent) = path.split_last()?;
        Some(self.geom(parent).instances[*last].def)
    }

    pub fn geom_mut(&mut self, path: &[usize]) -> &mut Geom {
        match self.def_at(path) {
            Some(d) => &mut self.defs[d].geom,
            None => &mut self.root,
        }
    }

    /// World transform of the context `path`.
    pub fn xf(&self, path: &[usize]) -> DMat4 {
        let mut m = DMat4::IDENTITY;
        let mut g = &self.root;
        for &i in path {
            let inst = &g.instances[i];
            m *= self.eff_xf(inst);
            g = &self.defs[inst.def].geom;
        }
        m
    }

    pub fn path_valid(&self, path: &[usize]) -> bool {
        let mut g = &self.root;
        for &i in path {
            match g.instances.get(i) {
                Some(inst) if inst.def < self.defs.len() => g = &self.defs[inst.def].geom,
                _ => return false,
            }
        }
        true
    }

    pub fn instance_count(&self, def: usize) -> usize {
        std::iter::once(&self.root)
            .chain(self.defs.iter().map(|d| &d.geom))
            .flat_map(|g| g.instances.iter())
            .filter(|i| i.def == def)
            .count()
    }

    /// Visit every drawable geometry with its world transform and ownership.
    pub fn visit<'a>(&'a self, path: &[usize], cb: &mut dyn FnMut(VisitItem<'a>)) {
        self.visit_outside(&self.root, DMat4::IDENTITY, path, None, cb, 0);
    }

    /// Whether an instance is drawn (hidden flag and tag visibility).
    pub fn inst_visible(&self, inst: &Instance) -> bool {
        self.show_hidden || (!inst.hidden && self.tag_visible(inst.tag))
    }

    pub fn tag_visible(&self, tag: Option<usize>) -> bool {
        tag.and_then(|t| self.tags.get(t)).is_none_or(|t| t.visible)
    }

    /// Whether a face is drawn.
    pub fn face_visible(&self, f: &crate::geom::Face) -> bool {
        self.show_hidden || (!f.hidden && self.tag_visible(f.tag))
    }

    fn visit_outside<'a>(&'a self, g: &'a Geom, xf: DMat4, path: &[usize], mat: Option<usize>, cb: &mut dyn FnMut(VisitItem<'a>), depth: usize) {
        if depth > 32 {
            return;
        }
        if path.is_empty() {
            cb(VisitItem { geom: g, xf, owner: Owner::Active, mat, tag: None, cast: true });
            for (i, inst) in g.instances.iter().enumerate() {
                if let Some(d) = self.defs.get(inst.def).filter(|_| self.inst_visible(inst)) {
                    self.visit_all(&d.geom, xf * self.eff_xf(inst), Owner::Inst(i), inst.material.or(mat), inst.tag, inst.cast_shadows, cb, depth + 1);
                }
            }
            return;
        }
        if !self.hide_rest {
            cb(VisitItem { geom: g, xf, owner: Owner::Outside, mat, tag: None, cast: true });
        }
        for (i, inst) in g.instances.iter().enumerate() {
            let Some(d) = self.defs.get(inst.def) else { continue };
            let x = xf * self.eff_xf(inst);
            let m = inst.material.or(mat);
            if i == path[0] {
                self.visit_outside(&d.geom, x, &path[1..], m, cb, depth + 1);
            } else if !self.hide_rest && self.inst_visible(inst) {
                self.visit_all(&d.geom, x, Owner::Outside, m, inst.tag, inst.cast_shadows, cb, depth + 1);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn visit_all<'a>(&'a self, g: &'a Geom, xf: DMat4, owner: Owner, mat: Option<usize>, tag: Option<usize>, cast: bool, cb: &mut dyn FnMut(VisitItem<'a>), depth: usize) {
        if depth > 32 {
            return;
        }
        cb(VisitItem { geom: g, xf, owner, mat, tag, cast });
        for inst in &g.instances {
            if let Some(d) = self.defs.get(inst.def).filter(|_| self.inst_visible(inst)) {
                self.visit_all(&d.geom, xf * self.eff_xf(inst), owner, inst.material.or(mat), inst.tag.or(tag), cast && inst.cast_shadows, cb, depth + 1);
            }
        }
    }

    /// Bounds of a geometry (recursively including instances) under `xf`.
    pub fn geom_bounds(&self, g: &Geom, xf: DMat4) -> Option<(DVec3, DVec3)> {
        let mut acc: Option<(DVec3, DVec3)> = None;
        self.geom_bounds_rec(g, xf, &mut acc, 0);
        acc
    }

    fn geom_bounds_rec(&self, g: &Geom, xf: DMat4, acc: &mut Option<(DVec3, DVec3)>, depth: usize) {
        if depth > 32 {
            return;
        }
        let mut add = |p: DVec3| {
            let p = xf.transform_point3(p);
            *acc = Some(match *acc {
                Some((lo, hi)) => (lo.min(p), hi.max(p)),
                None => (p, p),
            });
        };
        for e in &g.edges {
            add(g.p(e.v[0]));
            add(g.p(e.v[1]));
        }
        for f in &g.faces {
            for v in &f.outer {
                add(g.p(*v));
            }
        }
        for inst in &g.instances {
            if let Some(d) = self.defs.get(inst.def) {
                self.geom_bounds_rec(&d.geom, xf * self.eff_xf(inst), acc, depth + 1);
            }
        }
    }

    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        self.geom_bounds(&self.root, DMat4::IDENTITY)
    }

    /// Local bounds of a definition (in its own coordinates).
    pub fn def_bounds(&self, def: usize) -> Option<(DVec3, DVec3)> {
        self.geom_bounds(&self.defs[def].geom, DMat4::IDENTITY)
    }

    /// Turn the selection of context `path` into a new group/component.
    /// Returns the index of the new instance in that context.
    pub fn make_def(&mut self, path: &[usize], sel: &Sel, group: bool) -> Option<usize> {
        if sel.is_empty() {
            return None;
        }
        let part = self.geom(path).subset(sel);
        let origin = self.geom_bounds(&part, DMat4::IDENTITY).map(|b| b.0).unwrap_or(DVec3::ZERO);
        let mut part = part;
        part.transform_all(DMat4::from_translation(-origin));
        let n = self.defs.len() + 1;
        let name = if group { format!("Grupo#{n}") } else { format!("Componente#{n}") };
        self.defs.push(Definition { name, geom: part, group, face_me: false, procedure: None, ifc: None, author: None, light: None });
        let def = self.defs.len() - 1;
        let g = self.geom_mut(path);
        g.remove_sel(sel);
        g.instances.push(Instance { def, xf: DMat4::from_translation(origin), material: None, name: String::new(), ..Default::default() });
        Some(g.instances.len() - 1)
    }

    /// Replace instance `i` of context `path` by its geometry. Returns the new selection.
    pub fn explode(&mut self, path: &[usize], i: usize) -> Sel {
        let inst = self.geom(path).instances[i].clone();
        let content = self.defs[inst.def].geom.clone();
        let g = self.geom_mut(path);
        g.instances.remove(i);
        let sel = g.append(&content, inst.xf, inst.material);
        let (em, fm) = g.weld();
        sel.remap(&em, &fm)
    }

    pub fn make_unique(&mut self, path: &[usize], i: usize) {
        let def = self.geom(path).instances[i].def;
        let mut d = self.defs[def].clone();
        d.name = format!("{}#{}", d.name, self.defs.len() + 1);
        self.defs.push(d);
        let nd = self.defs.len() - 1;
        self.geom_mut(path).instances[i].def = nd;
    }

    /// Remove definitions that are not reachable from the root.
    pub fn purge_unused(&mut self) -> usize {
        let mut used: HashSet<usize> = HashSet::new();
        let mut stack: Vec<usize> = self.root.instances.iter().map(|i| i.def).collect();
        while let Some(d) = stack.pop() {
            if d < self.defs.len() && used.insert(d) {
                stack.extend(self.defs[d].geom.instances.iter().map(|i| i.def));
            }
        }
        let before = self.defs.len();
        let mut map: HashMap<usize, usize> = HashMap::new();
        let mut defs = Vec::new();
        for (i, d) in self.defs.drain(..).enumerate() {
            if used.contains(&i) {
                map.insert(i, defs.len());
                defs.push(d);
            }
        }
        self.defs = defs;
        let fix = |g: &mut Geom| {
            for inst in &mut g.instances {
                inst.def = map[&inst.def];
            }
        };
        fix(&mut self.root);
        for d in &mut self.defs {
            fix(&mut d.geom);
        }
        before - self.defs.len()
    }

    pub fn remove_material(&mut self, m: usize) {
        self.materials.remove(m);
        let fix = |x: &mut Option<usize>| {
            *x = match *x {
                Some(i) if i == m => None,
                Some(i) if i > m => Some(i - 1),
                o => o,
            }
        };
        let fix_geom = |g: &mut Geom| {
            for f in &mut g.faces {
                fix(&mut f.material);
            }
            for i in &mut g.instances {
                fix(&mut i.material);
            }
        };
        fix_geom(&mut self.root);
        for d in &mut self.defs {
            fix_geom(&mut d.geom);
        }
    }

    pub fn to_obj(&self) -> String {
        let mut s = String::from("# HomeUp export\n");
        let mut base = 1usize;
        let mut body = String::new();
        self.visit(&[], &mut |it| {
            let g = it.geom;
            if g.faces.is_empty() {
                return;
            }
            for v in &g.verts {
                let p = it.xf.transform_point3(*v);
                body += &format!("v {} {} {}\n", p.x, p.z, -p.y);
            }
            let flip = it.xf.determinant() < 0.0;
            for f in &g.faces {
                for t in g.triangulate_face(f) {
                    let (a, b, c) = (t[0] as usize + base, t[1] as usize + base, t[2] as usize + base);
                    if flip {
                        body += &format!("f {a} {c} {b}\n");
                    } else {
                        body += &format!("f {a} {b} {c}\n");
                    }
                }
            }
            base += g.verts.len();
        });
        s += &body;
        s
    }
}

#[derive(Serialize, Deserialize)]
pub struct SaveFile {
    pub format: String,
    pub version: u32,
    pub doc: Document,
    pub camera: [f64; 6],
    #[serde(default)]
    pub ortho: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    const EYE: DVec3 = DVec3::new(10.0, -10.0, 10.0);

    fn boxed(doc: &mut Document) {
        let g = &mut doc.root;
        g.add_polygon(
            &[DVec3::new(1.0, 1.0, 0.0), DVec3::new(2.0, 1.0, 0.0), DVec3::new(2.0, 2.0, 0.0), DVec3::new(1.0, 2.0, 0.0)],
            None,
            false,
            EYE,
        );
        g.push_pull(0, 1.0, false, EYE);
    }

    #[test]
    fn group_explode_roundtrip() {
        let mut doc = Document::new();
        boxed(&mut doc);
        let all = Sel { faces: (0..6).collect(), edges: (0..12).collect(), insts: HashSet::new() };
        let i = doc.make_def(&[], &all, true).unwrap();
        assert!(doc.root.faces.is_empty());
        assert_eq!(doc.root.instances.len(), 1);
        // definition is stored relative to its origin (bbox min)
        let (lo, hi) = doc.def_bounds(0).unwrap();
        assert!(lo.length() < 1e-9 && (hi - DVec3::splat(1.0)).length() < 1e-9);
        let (wlo, _) = doc.bounds().unwrap();
        assert!((wlo - DVec3::new(1.0, 1.0, 0.0)).length() < 1e-9);
        // edit inside the group
        assert_eq!(doc.geom(&[i]).faces.len(), 6);
        assert_eq!(doc.xf(&[i]).transform_point3(DVec3::ZERO), DVec3::new(1.0, 1.0, 0.0));
        let sel = doc.explode(&[], i);
        assert_eq!(sel.faces.len(), 6);
        assert_eq!(doc.root.faces.len(), 6);
        assert!(doc.root.instances.is_empty());
        assert_eq!(doc.purge_unused(), 1);
    }

    #[test]
    fn scale_figure_is_a_face_me_person() {
        let mut doc = Document::with_scale_figure();
        assert_eq!(doc.defs.len(), 1);
        assert!(doc.defs[0].face_me);
        assert_eq!(doc.defs[0].geom.faces.len(), 1);
        let (lo, hi) = doc.bounds().unwrap();
        assert!((hi.z - 1.735).abs() < 0.01 && lo.z.abs() < 1e-9);
        // turning the camera turns the figure around its vertical axis
        doc.view_yaw = 0.0;
        let a = doc.bounds().unwrap();
        doc.view_yaw = std::f64::consts::FRAC_PI_2;
        let b = doc.bounds().unwrap();
        assert!(((a.1 - a.0).y - (b.1 - b.0).x).abs() < 1e-9);
    }

    #[test]
    fn component_instances_share_definition() {
        let mut doc = Document::new();
        boxed(&mut doc);
        let all = Sel { faces: (0..6).collect(), edges: (0..12).collect(), insts: HashSet::new() };
        let i = doc.make_def(&[], &all, false).unwrap();
        let copy = doc.root.copy_sel(&Sel { insts: [i].into(), ..Default::default() }, DMat4::from_translation(DVec3::X * 3.0), true);
        assert_eq!(copy.insts.len(), 1);
        assert_eq!(doc.instance_count(0), 2);
        let mut faces = 0;
        doc.visit(&[], &mut |it| faces += it.geom.faces.len());
        assert_eq!(faces, 12);
        // editing the definition affects both instances
        doc.geom_mut(&[i]).faces.pop();
        let mut faces = 0;
        doc.visit(&[], &mut |it| faces += it.geom.faces.len());
        assert_eq!(faces, 10);
        let json = serde_json::to_string(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back.defs.len(), 1);
    }
}
