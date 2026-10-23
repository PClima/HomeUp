//! Sandbox (terrain) tools: From Contours (Delaunay TIN of selected contour
//! lines), From Scratch (flat grid TIN), Smoove (raise/lower with a smooth
//! radius), Stamp (flat pad under an object with a sloped offset), Drape
//! (project edges onto the terrain), Add Detail (subdivide faces) and Flip Edge.
//! TINs are groups named "Terreno" made of triangles with soft interior edges.

use crate::app::{HomeUpApp, In};
use crate::doc::Owner;
use crate::geom::{Edge, Face, Geom, VId};
use eframe::egui::Color32;
use glam::{DMat4, DVec2, DVec3};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxTool {
    FromScratch,
    Smoove,
    Stamp,
    Drape,
    AddDetail,
    FlipEdge,
}

impl SandboxTool {
    pub fn label(self) -> &'static str {
        match self {
            SandboxTool::FromScratch => "A partir do zero",
            SandboxTool::Smoove => "Suavizar relevo (Smoove)",
            SandboxTool::Stamp => "Carimbar",
            SandboxTool::Drape => "Projetar (Drape)",
            SandboxTool::AddDetail => "Adicionar detalhe",
            SandboxTool::FlipEdge => "Inverter aresta",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SandboxState {
    pub p0: Option<DVec3>,
    pub press: Option<(DVec3, f64)>,
    /// Pending height (m) while dragging Smoove/Stamp.
    pub height: f64,
}

pub const TIN_NAME: &str = "Terreno";

/// Geometry from points and triangles; edges used by two triangles are soft.
pub fn tin_geom(pts: &[DVec3], tris: &[[usize; 3]]) -> Geom {
    let mut g = Geom::default();
    g.verts = pts.to_vec();
    let mut count: HashMap<(usize, usize), usize> = HashMap::new();
    for t in tris {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            *count.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    let mut keys: Vec<_> = count.into_iter().collect();
    keys.sort();
    for ((a, b), n) in keys {
        let mut e = Edge::new(a as VId, b as VId);
        if n > 1 {
            e.soft = true;
        }
        g.edges.push(e);
    }
    for t in tris {
        let [a, b, c] = t.map(|i| pts[i]);
        let mut n = (b - a).cross(c - a);
        let mut tri = *t;
        if n.z < 0.0 {
            tri.swap(1, 2);
            n = -n;
        }
        g.faces.push(Face { outer: tri.iter().map(|i| *i as VId).collect(), normal: n.normalize_or(DVec3::Z), ..Default::default() });
    }
    g
}

/// Flat grid TIN covering `w` × `h` meters from the origin.
pub fn grid_tin(w: f64, h: f64, spacing: f64) -> Geom {
    let nx = ((w.abs() / spacing).round() as usize).clamp(1, 400);
    let ny = ((h.abs() / spacing).round() as usize).clamp(1, 400);
    let mut pts = Vec::new();
    for j in 0..=ny {
        for i in 0..=nx {
            pts.push(DVec3::new(w * i as f64 / nx as f64, h * j as f64 / ny as f64, 0.0));
        }
    }
    let id = |i: usize, j: usize| j * (nx + 1) + i;
    let mut tris = Vec::new();
    for j in 0..ny {
        for i in 0..nx {
            tris.push([id(i, j), id(i + 1, j), id(i + 1, j + 1)]);
            tris.push([id(i, j), id(i + 1, j + 1), id(i, j + 1)]);
        }
    }
    tin_geom(&pts, &tris)
}

/// Bowyer–Watson Delaunay triangulation in XY.
pub fn delaunay(pts: &[DVec3]) -> Vec<[usize; 3]> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    let (mut lo, mut hi) = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
    for p in pts {
        lo = lo.min(p.truncate());
        hi = hi.max(p.truncate());
    }
    let d = (hi - lo).max_element().max(1e-6) * 20.0;
    let c = (lo + hi) * 0.5;
    let mut v: Vec<DVec2> = pts.iter().map(|p| p.truncate()).collect();
    v.extend([c + DVec2::new(-d, -d), c + DVec2::new(d, -d), c + DVec2::new(0.0, d)]);
    let mut tris: Vec<[usize; 3]> = vec![[n, n + 1, n + 2]];
    let circ = |t: &[usize; 3], v: &[DVec2]| {
        let (a, b, c) = (v[t[0]], v[t[1]], v[t[2]]);
        let dd = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
        if dd.abs() < 1e-18 {
            return (a, f64::MAX);
        }
        let (a2, b2, c2) = (a.length_squared(), b.length_squared(), c.length_squared());
        let ux = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / dd;
        let uy = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / dd;
        let cc = DVec2::new(ux, uy);
        (cc, cc.distance_squared(a))
    };
    for i in 0..n {
        let p = v[i];
        let (bad, keep): (Vec<[usize; 3]>, Vec<[usize; 3]>) = tris.into_iter().partition(|t| {
            let (cc, r2) = circ(t, &v);
            cc.distance_squared(p) < r2 - 1e-12
        });
        let mut edges: HashMap<(usize, usize), usize> = HashMap::new();
        for t in &bad {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                *edges.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        tris = keep;
        for ((a, b), cnt) in edges {
            if cnt == 1 {
                tris.push([a, b, i]);
            }
        }
    }
    tris.retain(|t| t.iter().all(|i| *i < n));
    tris
}

/// Height of the TIN at (x, y), if inside a triangle.
pub fn z_at(g: &Geom, q: DVec2) -> Option<f64> {
    for f in &g.faces {
        if f.outer.len() != 3 {
            continue;
        }
        let [a, b, c] = [0, 1, 2].map(|k| g.p(f.outer[k]));
        let (a2, b2, c2) = (a.truncate(), b.truncate(), c.truncate());
        let den = (b2 - a2).perp_dot(c2 - a2);
        if den.abs() < 1e-14 {
            continue;
        }
        let u = (q - a2).perp_dot(c2 - a2) / den;
        let v = (b2 - a2).perp_dot(q - a2) / den;
        if u >= -1e-9 && v >= -1e-9 && u + v <= 1.0 + 1e-9 {
            return Some(a.z + (b.z - a.z) * u + (c.z - a.z) * v);
        }
    }
    None
}

/// Smooth falloff used by Smoove (1 at the center, 0 at the radius).
pub fn falloff(d: f64, r: f64) -> f64 {
    if d >= r {
        0.0
    } else {
        0.5 * ((std::f64::consts::PI * d / r).cos() + 1.0)
    }
}

pub fn recompute_normals(g: &mut Geom) {
    for k in 0..g.faces.len() {
        if g.faces[k].outer.len() == 3 {
            let [a, b, c] = [0, 1, 2].map(|i| g.p(g.faces[k].outer[i]));
            let n = (b - a).cross(c - a);
            if n.length() > 1e-14 {
                g.faces[k].normal = n.normalize();
            }
        }
    }
}

/// Re-triangulate faces that are no longer triangles (after cutting lines into the TIN).
pub fn triangulate_tin(g: &mut Geom) {
    let mut out = Vec::new();
    let mut changed = false;
    for f in std::mem::take(&mut g.faces) {
        if f.outer.len() == 3 && f.holes.is_empty() {
            out.push(f);
            continue;
        }
        changed = true;
        for t in g.triangulate_face(&f) {
            let mut t = t.to_vec();
            let [p, q, r] = [0, 1, 2].map(|k| g.p(t[k]));
            if (q - p).cross(r - p).z < 0.0 {
                t.swap(1, 2);
            }
            out.push(Face { outer: t, holes: vec![], uvs: None, uvq: None, ..f.clone() });
        }
    }
    g.faces = out;
    if changed {
        let faces: Vec<Vec<VId>> = g.faces.iter().map(|f| f.outer.clone()).collect();
        for t in faces {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                if g.find_edge(a, b).is_none() {
                    let mut e = Edge::new(a, b);
                    e.soft = true;
                    g.edges.push(e);
                }
            }
        }
    }
    recompute_normals(g);
}

/// TIN convention: edges between two triangles are soft, except those lying on `keep`.
pub fn soften_tin(g: &mut Geom, keep: &[(DVec3, DVec3)]) {
    let mut count: HashMap<(VId, VId), usize> = HashMap::new();
    for f in &g.faces {
        for k in 0..f.outer.len() {
            let (a, b) = (f.outer[k], f.outer[(k + 1) % f.outer.len()]);
            *count.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    let on = |p: DVec3| {
        keep.iter().any(|(a, b)| {
            let ab = *b - *a;
            let t = ((p - *a).dot(ab) / ab.length_squared().max(1e-18)).clamp(0.0, 1.0);
            (*a + ab * t).distance(p) < 1e-5
        })
    };
    for k in 0..g.edges.len() {
        let [a, b] = g.edges[k].v;
        let inner = count.get(&(a.min(b), a.max(b))).copied().unwrap_or(0) >= 2;
        let kept = !keep.is_empty() && on(g.p(a)) && on(g.p(b));
        g.edges[k].soft = inner && !kept;
    }
}

/// Split each selected triangle at its centroid into three.
pub fn add_detail(g: &mut Geom, faces: &HashSet<usize>) -> usize {
    let mut n = 0;
    let mut list: Vec<usize> = faces.iter().copied().filter(|f| g.faces.get(*f).is_some_and(|x| x.outer.len() == 3)).collect();
    list.sort_unstable();
    for fi in list {
        let t = g.faces[fi].outer.clone();
        let c = (g.p(t[0]) + g.p(t[1]) + g.p(t[2])) / 3.0;
        g.verts.push(c);
        let cv = (g.verts.len() - 1) as VId;
        for v in &t {
            let mut e = Edge::new(*v, cv);
            e.soft = true;
            g.edges.push(e);
        }
        for e in &mut g.edges {
            if t.iter().any(|a| e.v[0] == *a) && t.iter().any(|b| e.v[1] == *b) {
                e.soft = true;
            }
        }
        let base = g.faces[fi].clone();
        g.faces[fi] = Face { outer: vec![t[0], t[1], cv], ..base.clone() };
        g.faces.push(Face { outer: vec![t[1], t[2], cv], ..base.clone() });
        g.faces.push(Face { outer: vec![t[2], t[0], cv], ..base });
        n += 1;
    }
    recompute_normals(g);
    n
}

/// Flip the diagonal shared by two triangles. Returns false if not possible.
pub fn flip_edge(g: &mut Geom, ei: usize) -> bool {
    let [a, b] = g.edges[ei].v;
    let fs: Vec<usize> = (0..g.faces.len()).filter(|f| g.faces[*f].outer.len() == 3 && g.faces[*f].outer.contains(&a) && g.faces[*f].outer.contains(&b)).collect();
    if fs.len() != 2 {
        return false;
    }
    let other = |f: usize| *g.faces[f].outer.iter().find(|v| **v != a && **v != b).unwrap();
    let (c, d) = (other(fs[0]), other(fs[1]));
    if g.find_edge(c, d).is_some() {
        return false;
    }
    let (m0, m1) = (g.faces[fs[0]].clone(), g.faces[fs[1]].clone());
    g.faces[fs[0]] = Face { outer: vec![c, d, a], ..m0 };
    g.faces[fs[1]] = Face { outer: vec![d, c, b], ..m1 };
    g.edges[ei].v = [c, d];
    // keep normals pointing up
    for f in [fs[0], fs[1]] {
        let [p, q, r] = [0, 1, 2].map(|k| g.p(g.faces[f].outer[k]));
        if (q - p).cross(r - p).z < 0.0 {
            g.faces[f].outer.swap(1, 2);
        }
    }
    recompute_normals(g);
    true
}

/// Pieces of a segment split where it crosses TIN triangle edges, lifted onto the TIN.
pub fn drape_segment(g: &Geom, a: DVec2, b: DVec2) -> Vec<(DVec3, DVec3)> {
    let mut ts = vec![0.0, 1.0];
    let d = b - a;
    for e in &g.edges {
        let (p, q) = (g.p(e.v[0]).truncate(), g.p(e.v[1]).truncate());
        let s = q - p;
        let den = d.perp_dot(s);
        if den.abs() < 1e-14 {
            continue;
        }
        let t = (p - a).perp_dot(s) / den;
        let u = (p - a).perp_dot(d) / den;
        if t > 1e-9 && t < 1.0 - 1e-9 && (-1e-9..=1.0 + 1e-9).contains(&u) {
            ts.push(t);
        }
    }
    ts.sort_by(|x, y| x.partial_cmp(y).unwrap());
    ts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
    let lift = |t: f64| {
        let q = a + d * t;
        z_at(g, q).map(|z| DVec3::new(q.x, q.y, z))
    };
    ts.windows(2).filter_map(|w| Some((lift(w[0])?, lift(w[1])?))).collect()
}

impl HomeUpApp {
    pub(crate) fn start_sandbox(&mut self, t: SandboxTool) {
        self.set_tool(crate::app::Tool::Select);
        match t {
            SandboxTool::AddDetail => {
                let faces = self.sel.faces.clone();
                if faces.is_empty() {
                    self.message = "Adicionar detalhe: edite o terreno (duplo clique) e selecione as faces.".into();
                    return;
                }
                self.checkpoint_keep_sel();
                let n = add_detail(self.active_mut(), &faces);
                self.sel.clear();
                self.dirty = true;
                self.cache_dirty = true;
                self.message = format!("{n} faces subdivididas.");
                return;
            }
            SandboxTool::Drape if self.sel.edges.is_empty() => {
                self.message = "Projetar: selecione as arestas que ficam acima do terreno e clique no terreno.".into();
                return;
            }
            SandboxTool::Stamp if self.sel.insts.len() != 1 => {
                self.message = "Carimbar: selecione o grupo ou componente acima do terreno e depois clique nele.".into();
                return;
            }
            _ => {}
        }
        self.sandbox = Some((t, SandboxState::default()));
        self.message = match t {
            SandboxTool::FromScratch => "A partir do zero: clique o primeiro canto e o canto oposto (ou digite \"largura;comprimento\"). Espaçamento da grade: digite antes do 1º clique.".into(),
            SandboxTool::Smoove => format!("Smoove: clique no terreno e arraste para cima/baixo (raio {}: digite para mudar).", crate::units::format_len(self.sandbox_radius)),
            SandboxTool::Stamp => format!("Carimbar: clique no objeto selecionado e mova para ajustar a altura (borda {}: digite para mudar).", crate::units::format_len(self.sandbox_offset)),
            SandboxTool::Drape => "Projetar: clique no terreno.".into(),
            SandboxTool::FlipEdge => "Inverter aresta: clique numa aresta interna do terreno (dentro do grupo).".into(),
            SandboxTool::AddDetail => String::new(),
        };
    }

    /// Typed values: grid spacing / dimensions (From Scratch), radius (Smoove), offset (Stamp).
    pub(crate) fn sandbox_vcb(&mut self, text: &str) -> bool {
        let Some((t, st)) = self.sandbox.clone() else { return false };
        let parse = |s: &str| crate::units::parse_len(s.trim());
        match t {
            SandboxTool::FromScratch => {
                if let (Some(p0), Some((a, b))) = (st.p0, text.split_once(';')) {
                    if let (Some(w), Some(h)) = (parse(a), parse(b)) {
                        self.create_scratch_tin(p0, w, h);
                    }
                } else if let Some(v) = parse(text) {
                    self.sandbox_spacing = v.max(0.01);
                    self.message = format!("Espaçamento da grade: {}", crate::units::format_len(self.sandbox_spacing));
                }
            }
            SandboxTool::Smoove => {
                if st.press.is_some() {
                    if let Some(v) = parse(text) {
                        let mut st2 = st.clone();
                        st2.height = v;
                        self.sandbox = Some((t, st2));
                        self.commit_smoove();
                    }
                } else if let Some(v) = parse(text) {
                    self.sandbox_radius = v.max(0.01);
                }
            }
            SandboxTool::Stamp => {
                if let Some(v) = parse(text) {
                    self.sandbox_offset = v.max(0.0);
                }
            }
            _ => {}
        }
        true
    }

    pub(crate) fn create_scratch_tin(&mut self, p0: DVec3, w: f64, h: f64) {
        if w.abs() < 1e-3 || h.abs() < 1e-3 {
            return;
        }
        self.checkpoint();
        let g = grid_tin(w, h, self.sandbox_spacing);
        self.doc.defs.push(crate::doc::Definition { name: TIN_NAME.into(), geom: g, group: true, face_me: false, procedure: None, ifc: None, author: None, light: None });
        let d = self.doc.defs.len() - 1;
        let xf = self.ctx_xf().inverse() * DMat4::from_translation(p0);
        let ga = self.active_mut();
        ga.instances.push(crate::geom::Instance { def: d, xf, ..Default::default() });
        let k = ga.instances.len() - 1;
        self.sel = crate::geom::Sel { insts: [k].into(), ..Default::default() };
        self.sandbox = None;
        self.live_measure = None;
        self.cache_dirty = true;
        self.dirty = true;
        self.message = "Terreno criado (grupo). Dê duplo clique para editar com Smoove, Adicionar detalhe ou Inverter aresta.".into();
    }

    pub(crate) fn tins_from_contours(&mut self) {
        let g = self.active();
        let es: Vec<usize> = self.sel.edges.iter().copied().collect();
        let mut pts: Vec<DVec3> = Vec::new();
        let mut seen = HashSet::new();
        let xf = self.ctx_xf();
        for e in es {
            for v in g.edges[e].v {
                if seen.insert(v) {
                    pts.push(xf.transform_point3(g.p(v)));
                }
            }
        }
        if pts.len() < 3 {
            self.message = "A partir de contornos: selecione as curvas de nível (arestas) primeiro.".into();
            return;
        }
        let tris = delaunay(&pts);
        if tris.is_empty() {
            self.message = "Não foi possível triangular os contornos.".into();
            return;
        }
        let c = pts.iter().fold(DVec3::ZERO, |a, p| a + *p) / pts.len() as f64;
        let origin = DVec3::new(c.x, c.y, pts.iter().map(|p| p.z).fold(f64::MAX, f64::min));
        let local: Vec<DVec3> = pts.iter().map(|p| *p - origin).collect();
        self.checkpoint_keep_sel();
        self.doc.defs.push(crate::doc::Definition { name: TIN_NAME.into(), geom: tin_geom(&local, &tris), group: true, face_me: false, procedure: None, ifc: None, author: None, light: None });
        let d = self.doc.defs.len() - 1;
        let xf = self.ctx_xf().inverse() * DMat4::from_translation(origin);
        let ga = self.active_mut();
        ga.instances.push(crate::geom::Instance { def: d, xf, ..Default::default() });
        self.cache_dirty = true;
        self.dirty = true;
        self.message = format!("Terreno criado a partir de {} pontos ({} triângulos).", pts.len(), tris.len());
    }

    /// The TIN under the cursor: enters it (Smoove/Flip Edge work inside the group).
    fn enter_tin_at(&mut self, px: glam::DVec2) -> bool {
        let (o, d) = self.cam.ray(px);
        if let Some((_, tri)) = self.cache.pick_face(o, d) {
            if let Owner::Inst(k) = self.cache.tris[tri].owner {
                let def = self.active().instances[k].def;
                if self.doc.defs[def].name.starts_with(TIN_NAME) && !self.active().instances[k].locked {
                    self.path.push(k);
                    self.sel.clear();
                    self.cache_dirty = true;
                    self.dirty = true;
                    return true;
                }
            }
        }
        false
    }

    fn commit_smoove(&mut self) {
        let Some((t, st)) = self.sandbox.clone() else { return };
        let Some((c, _)) = st.press else { return };
        self.checkpoint_keep_sel();
        let r = self.sandbox_radius;
        let g = self.active_mut();
        for p in &mut g.verts {
            let k = falloff(p.truncate().distance(c.truncate()), r);
            p.z += st.height * k;
        }
        recompute_normals(g);
        self.preview = None;
        self.sandbox = Some((t, SandboxState::default()));
        self.live_measure = None;
        self.cache_dirty = true;
        self.dirty = true;
    }

    pub(crate) fn sandbox_frame(&mut self, i: &In) -> bool {
        let Some((t, mut st)) = self.sandbox.clone() else { return false };
        let blue = Color32::from_rgb(30, 110, 220);
        match t {
            SandboxTool::FromScratch => {
                let s = self.snap(i.px, st.p0);
                match st.p0 {
                    None => {
                        self.overlay.squares.push((s.p, blue));
                        if i.pressed {
                            st.p0 = Some(s.p);
                        }
                    }
                    Some(p0) => {
                        let (o, d) = self.cam.ray(i.px);
                        let p = if d.z.abs() > 1e-9 { o + d * ((p0.z - o.z) / d.z) } else { s.p };
                        let (w, h) = (p.x - p0.x, p.y - p0.y);
                        let c = [p0, p0 + DVec3::X * w, p0 + DVec3::new(w, h, 0.0), p0 + DVec3::Y * h];
                        for k in 0..4 {
                            self.overlay.lines.push((c[k], c[(k + 1) % 4], blue, 2.0));
                        }
                        self.live_measure = Some(format!("{}; {}", crate::units::format_len(w.abs()), crate::units::format_len(h.abs())));
                        if i.pressed {
                            self.create_scratch_tin(p0, w, h);
                            return true;
                        }
                    }
                }
            }
            SandboxTool::Smoove => {
                let r = self.sandbox_radius;
                match st.press {
                    None => {
                        let (o, d) = self.cam.ray(i.px);
                        if let Some((tt, tri)) = self.cache.pick_face(o, d) {
                            let hit = o + d * tt;
                            if self.cache.tris[tri].owner != Owner::Active {
                                if i.pressed && self.enter_tin_at(i.px) {
                                    self.message = "Editando o terreno — clique e arraste para modelar.".into();
                                }
                            } else {
                                // radius circle
                                for k in 0..32 {
                                    let (a0, a1) = (k as f64 / 32.0 * std::f64::consts::TAU, (k + 1) as f64 / 32.0 * std::f64::consts::TAU);
                                    let q = |a: f64| hit + DVec3::new(a.cos() * r, a.sin() * r, 0.0);
                                    self.overlay.lines.push((q(a0), q(a1), Color32::from_rgb(220, 120, 20), 1.5));
                                }
                                if i.pressed {
                                    let local = self.ctx_xf().inverse().transform_point3(hit);
                                    st.press = Some((local, i.pos.y as f64));
                                }
                            }
                        }
                    }
                    Some((c, y0)) => {
                        let wpp = self.cam.world_per_px(self.cam.depth(self.ctx_xf().transform_point3(c)));
                        st.height = (y0 - i.pos.y as f64) * wpp;
                        let mut doc = self.doc.clone();
                        let g = doc.geom_mut(&self.path);
                        for p in &mut g.verts {
                            p.z += st.height * falloff(p.truncate().distance(c.truncate()), r);
                        }
                        recompute_normals(g);
                        self.preview = Some(doc);
                        self.live_measure = Some(crate::units::format_len(st.height));
                        if i.released && (y0 - i.pos.y as f64).abs() > 2.0 || i.pressed {
                            self.sandbox = Some((t, st));
                            self.commit_smoove();
                            return true;
                        }
                    }
                }
            }
            SandboxTool::Stamp => {
                let Some(&k) = self.sel.insts.iter().next() else {
                    self.sandbox = None;
                    return true;
                };
                let inst = self.active().instances[k].clone();
                let Some((lo, hi)) = self.doc.def_bounds(inst.def) else { return true };
                let m = self.ctx_xf() * self.doc.eff_xf(&inst);
                let corners: Vec<DVec3> = (0..8).map(|c| m.transform_point3(DVec3::new(if c & 1 == 0 { lo.x } else { hi.x }, if c & 2 == 0 { lo.y } else { hi.y }, if c & 4 == 0 { lo.z } else { hi.z }))).collect();
                let (mut a, mut b) = (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN));
                for p in &corners {
                    a = a.min(*p);
                    b = b.max(*p);
                }
                let off = self.sandbox_offset;
                // find a TIN instance under the footprint (in the active context)
                let tin = (0..self.active().instances.len()).find(|j| {
                    let ins = &self.active().instances[*j];
                    *j != k && self.doc.defs[ins.def].name.starts_with(TIN_NAME)
                });
                let Some(tk) = tin else {
                    self.message = "Carimbar: não há terreno (grupo \"Terreno\") neste contexto.".into();
                    self.sandbox = None;
                    return true;
                };
                match st.press {
                    None => {
                        for (p, q) in [(DVec3::new(a.x - off, a.y - off, a.z), DVec3::new(b.x + off, a.y - off, a.z)), (DVec3::new(b.x + off, a.y - off, a.z), DVec3::new(b.x + off, b.y + off, a.z)), (DVec3::new(b.x + off, b.y + off, a.z), DVec3::new(a.x - off, b.y + off, a.z)), (DVec3::new(a.x - off, b.y + off, a.z), DVec3::new(a.x - off, a.y - off, a.z))] {
                            self.overlay.lines.push((p, q, Color32::from_rgb(220, 120, 20), 1.5));
                        }
                        if i.pressed {
                            st.press = Some((a, i.pos.y as f64));
                        }
                    }
                    Some((base, y0)) => {
                        let wpp = self.cam.world_per_px(self.cam.depth(base));
                        st.height = (y0 - i.pos.y as f64) * wpp;
                        let target = base.z + st.height;
                        let tinst = self.active().instances[tk].clone();
                        let tm = self.ctx_xf() * self.doc.eff_xf(&tinst);
                        let inv = tm.inverse();
                        let stamp = |doc: &mut crate::doc::Document| {
                            let g = &mut doc.defs[tinst.def].geom;
                            for p in &mut g.verts {
                                let w = tm.transform_point3(*p);
                                let dx = (a.x - w.x).max(w.x - b.x).max(0.0);
                                let dy = (a.y - w.y).max(w.y - b.y).max(0.0);
                                let d = (dx * dx + dy * dy).sqrt();
                                if d <= off + 1e-9 {
                                    let k = if off > 1e-9 { 1.0 - d / off } else { 1.0 };
                                    let k = k * k * (3.0 - 2.0 * k);
                                    let nz = w.z + (target - w.z) * k;
                                    *p = inv.transform_point3(DVec3::new(w.x, w.y, nz));
                                }
                            }
                            recompute_normals(g);
                        };
                        let mut doc = self.doc.clone();
                        stamp(&mut doc);
                        self.preview = Some(doc);
                        self.live_measure = Some(crate::units::format_len(st.height));
                        if i.pressed {
                            self.checkpoint_keep_sel();
                            let mut d = std::mem::take(&mut self.doc);
                            {
                                // cut the pad and slope outlines into the TIN first
                                let g = &mut d.defs[tinst.def].geom;
                                let mut pieces = Vec::new();
                                for (lo, hi) in [(a, b), (a - DVec3::new(off, off, 0.0), b + DVec3::new(off, off, 0.0))] {
                                    let c = [(lo.x, lo.y), (hi.x, lo.y), (hi.x, hi.y), (lo.x, hi.y)].map(|(x, y)| inv.transform_point3(DVec3::new(x, y, 0.0)).truncate());
                                    for k in 0..4 {
                                        pieces.extend(drape_segment(g, c[k], c[(k + 1) % 4]));
                                    }
                                }
                                let eye = inv.transform_point3(self.cam.eye());
                                g.add_segments(&pieces, None, false, eye);
                                triangulate_tin(g);
                                soften_tin(g, &[]);
                            }
                            stamp(&mut d);
                            self.doc = d;
                            self.preview = None;
                            self.sandbox = None;
                            self.live_measure = None;
                            self.cache_dirty = true;
                            self.dirty = true;
                            self.message = "Carimbo aplicado ao terreno.".into();
                            return true;
                        }
                    }
                }
            }
            SandboxTool::Drape => {
                if i.pressed {
                    let (o, d) = self.cam.ray(i.px);
                    let Some((_, tri)) = self.cache.pick_face(o, d) else { return true };
                    let Owner::Inst(tk) = self.cache.tris[tri].owner else {
                        self.message = "Clique no grupo do terreno.".into();
                        return true;
                    };
                    let tinst = self.active().instances[tk].clone();
                    let tm = self.ctx_xf() * self.doc.eff_xf(&tinst);
                    let inv = tm.inverse();
                    let src = self.active().clone();
                    let xf = self.ctx_xf();
                    let segs: Vec<(DVec2, DVec2)> = self.sel.edges.iter().map(|e| {
                        let [p, q] = src.edges[*e].v.map(|v| inv.transform_point3(xf.transform_point3(src.p(v))).truncate());
                        (p, q)
                    }).collect();
                    self.checkpoint_keep_sel();
                    let g = &mut self.doc.defs[tinst.def].geom;
                    let mut pieces = Vec::new();
                    for (p, q) in segs {
                        pieces.extend(drape_segment(g, p, q));
                    }
                    let n = pieces.len();
                    let eye = inv.transform_point3(self.cam.eye());
                    g.add_segments(&pieces, None, false, eye);
                    triangulate_tin(g);
                    soften_tin(g, &pieces);
                    self.sandbox = None;
                    self.cache_dirty = true;
                    self.dirty = true;
                    self.message = format!("Arestas projetadas no terreno ({n} trechos).");
                }
            }
            SandboxTool::FlipEdge => {
                let hit = self.cache.pick_edge(&self.cam, i.px, 6.0, |e| e.owner != Owner::Outside);
                if let Some(k) = hit {
                    let pe = &self.cache.edges[k];
                    if pe.owner == Owner::Active {
                        let ei = pe.edge;
                        let g = self.active();
                        let xf = self.ctx_xf();
                        let [p, q] = g.edges[ei].v.map(|v| xf.transform_point3(g.p(v)));
                        self.overlay.lines.push((p, q, Color32::from_rgb(220, 30, 200), 3.0));
                        if i.pressed {
                            self.checkpoint_keep_sel();
                            if flip_edge(self.active_mut(), ei) {
                                self.cache_dirty = true;
                                self.dirty = true;
                            } else {
                                self.undo.pop();
                                self.message = "Essa aresta não é a diagonal entre dois triângulos.".into();
                            }
                        }
                    } else if i.pressed {
                        self.enter_tin_at(i.px);
                    }
                }
            }
            SandboxTool::AddDetail => {}
        }
        if self.sandbox.is_some() {
            self.sandbox = Some((t, st));
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grid_and_delaunay() {
        let g = grid_tin(4.0, 2.0, 1.0);
        assert_eq!(g.faces.len(), 16);
        assert_eq!(g.verts.len(), 15);
        assert!(g.edges.iter().filter(|e| e.soft).count() > 0);
        assert!(g.faces.iter().all(|f| f.normal.z > 0.99));
        let pts = [DVec3::new(0.0, 0.0, 0.0), DVec3::new(1.0, 0.0, 0.0), DVec3::new(1.0, 1.0, 1.0), DVec3::new(0.0, 1.0, 1.0), DVec3::new(0.5, 0.5, 0.5)];
        let t = delaunay(&pts);
        assert_eq!(t.len(), 4);
        let tin = tin_geom(&pts, &t);
        let z = z_at(&tin, DVec2::new(0.5, 0.25)).unwrap();
        assert!(z > 0.1 && z < 0.5, "{z}");
    }

    #[test]
    fn detail_flip_and_drape() {
        let mut g = grid_tin(2.0, 2.0, 2.0);
        assert_eq!(g.faces.len(), 2);
        let diag = g.edges.iter().position(|e| e.soft).unwrap();
        assert!(flip_edge(&mut g, diag));
        assert_eq!(g.faces.len(), 2);
        assert!(g.faces.iter().all(|f| f.normal.z > 0.99));
        let n = add_detail(&mut g, &[0usize].into());
        assert_eq!(n, 1);
        assert_eq!(g.faces.len(), 4);
        let pieces = drape_segment(&g, DVec2::new(0.2, 1.0), DVec2::new(1.8, 1.0));
        assert!(pieces.len() >= 2);
        // cutting the pieces keeps the TIN made of triangles, with the cut line hard
        g.add_segments(&pieces, None, false, DVec3::new(1.0, 1.0, 10.0));
        triangulate_tin(&mut g);
        soften_tin(&mut g, &pieces);
        assert!(g.faces.iter().all(|f| f.outer.len() == 3 && f.normal.z > 0.0));
        assert!(g.faces.len() > 4);
        assert!(g.edges.iter().any(|e| !e.soft && (g.p(e.v[0]).y - 1.0).abs() < 1e-9 && (g.p(e.v[1]).y - 1.0).abs() < 1e-9));
        assert!((falloff(0.0, 1.0) - 1.0).abs() < 1e-12 && falloff(1.0, 1.0) == 0.0);
    }
}
