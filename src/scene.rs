//! Builds the GPU scene (triangles + lines) from the document.

use crate::doc::{Document, Owner};
use crate::geom::{Geom, Sel, VId};
use crate::render::{Scene3D, Vertex};
use crate::settings::{EdgeColor, FaceStyle};
use glam::{DMat4, DVec3};
use std::collections::{HashMap, HashSet};

pub struct SceneOpts<'a> {
    pub path: &'a [usize],
    pub sel: &'a Sel,
    pub show_sel: bool,
    pub highlight_face: Option<usize>,
    pub highlight_inst: Option<usize>,
    pub axes: bool,
    pub color_by_tag: bool,
    pub hidden_geometry: bool,
}

pub fn rgb(c: [u8; 3]) -> [f32; 4] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0]
}

const FRONT: [u8; 3] = [242, 242, 236];
const BACK: [u8; 3] = [150, 166, 192];
const SEL: [u8; 3] = [120, 150, 235];
const HOVER: [u8; 3] = [200, 210, 250];
const BG: [f32; 4] = [214.0 / 255.0, 226.0 / 255.0, 236.0 / 255.0, 1.0];

fn fade(c: [f32; 4]) -> [f32; 4] {
    [c[0] * 0.35 + BG[0] * 0.65, c[1] * 0.35 + BG[1] * 0.65, c[2] * 0.35 + BG[2] * 0.65, c[3]]
}

/// Per-(face, vertex) smooth normals across soft edges (local coordinates).
fn smooth_normals(g: &Geom) -> Option<HashMap<(usize, VId), DVec3>> {
    let soft: HashSet<(VId, VId)> =
        g.edges.iter().filter(|e| e.soft).map(|e| (e.v[0].min(e.v[1]), e.v[0].max(e.v[1]))).collect();
    if soft.is_empty() {
        return None;
    }
    let mut by_edge: HashMap<(VId, VId), Vec<usize>> = HashMap::new();
    for (fi, f) in g.faces.iter().enumerate() {
        for (a, b) in f.boundary_edges() {
            let k = (a.min(b), a.max(b));
            if soft.contains(&k) {
                by_edge.entry(k).or_default().push(fi);
            }
        }
    }
    let mut parent: Vec<usize> = (0..g.faces.len()).collect();
    fn find(p: &mut Vec<usize>, x: usize) -> usize {
        if p[x] != x {
            let r = find(p, p[x]);
            p[x] = r;
        }
        p[x]
    }
    for fs in by_edge.values() {
        for w in fs.windows(2) {
            let (a, b) = (find(&mut parent, w[0]), find(&mut parent, w[1]));
            parent[a] = b;
        }
    }
    let mut acc: HashMap<(usize, VId), DVec3> = HashMap::new();
    for (fi, f) in g.faces.iter().enumerate() {
        let grp = find(&mut parent, fi);
        for v in f.loops().flatten() {
            *acc.entry((grp, *v)).or_default() += f.normal;
        }
    }
    let mut out = HashMap::new();
    for (fi, f) in g.faces.iter().enumerate() {
        let grp = find(&mut parent, fi);
        for v in f.loops().flatten() {
            let n = acc[&(grp, *v)];
            out.insert((fi, *v), if n.length() > 1e-9 { n.normalize() } else { f.normal });
        }
    }
    Some(out)
}

/// Material of the first face using each edge (for "edge color by material").
fn edge_materials(g: &Geom) -> HashMap<(VId, VId), Option<usize>> {
    let mut m = HashMap::new();
    for f in &g.faces {
        for l in f.loops() {
            for k in 0..l.len() {
                let (a, b) = (l[k], l[(k + 1) % l.len()]);
                m.entry((a.min(b), a.max(b))).or_insert(f.material);
            }
        }
    }
    m
}

/// One edge as screen-space quads (body, optional end caps and extensions); see `vs_wide`.
#[allow(clippy::too_many_arguments)]
fn push_wide(s: &mut Scene3D, a: DVec3, b: DVec3, c: [f32; 4], seed: f32, ext: f32, caps: bool, dash: f32) {
    let v = |p: DVec3, q: DVec3, side: f32, kind: f32| Vertex {
        pos: p.as_vec3().to_array(),
        nrm: [0.0; 3],
        col: c,
        back: [q.x as f32, q.y as f32, q.z as f32, side],
        uv: [seed, kind],
        layer: -3.0,
        tan: [ext, dash, 0.0, 1.0],
    };
    // body: corners at a (+/-) and at b (sides flip because b sees the edge reversed)
    let (ap, am, bp, bm) = (v(a, b, 1.0, 0.0), v(a, b, -1.0, 0.0), v(b, a, -1.0, 0.0), v(b, a, 1.0, 0.0));
    s.wide.extend_from_slice(&[ap, am, bm, ap, bm, bp]);
    for (p, q) in [(a, b), (b, a)] {
        if caps {
            let (np, nm, fp, fm) = (v(p, q, 1.0, 1.0), v(p, q, -1.0, 1.0), v(p, q, 1.0, 2.0), v(p, q, -1.0, 2.0));
            s.wide.extend_from_slice(&[np, nm, fm, np, fm, fp]);
        }
        if ext > 0.0 {
            let (np, nm, fp, fm) = (v(p, q, 1.0, 0.0), v(p, q, -1.0, 0.0), v(p, q, 1.0, 3.0), v(p, q, -1.0, 3.0));
            s.wide.extend_from_slice(&[np, nm, fm, np, fm, fp]);
        }
    }
}

fn push_line(s: &mut Scene3D, a: DVec3, b: DVec3, c: [f32; 4]) {
    for p in [a, b] {
        s.lines.push(Vertex::plain(p.as_vec3().to_array(), [0.0; 3], c, c));
    }
}

pub fn box_lines(s: &mut Scene3D, lo: DVec3, hi: DVec3, xf: DMat4, c: [f32; 4]) {
    let corner = |i: usize| {
        xf.transform_point3(DVec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        ))
    };
    for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
        push_line(s, corner(a), corner(b), c);
    }
}

/// GPU record of a material (see `render::GpuMat`).
fn gpu_mat(doc: &Document, m: &crate::doc::Material) -> crate::render::GpuMat {
    let l = |t: Option<usize>| t.filter(|t| *t < doc.textures.len() && *t < crate::tex::MAX_LAYERS).map(|t| t as f32).unwrap_or(-1.0);
    [
        l(m.texture),
        l(m.normal_tex),
        l(m.rough_tex),
        l(m.metal_tex),
        l(m.ao_tex),
        m.metalness.clamp(0.0, 1.0),
        m.roughness.clamp(0.02, 1.0),
        m.normal_scale,
        m.rough_ch as f32,
        m.metal_ch as f32,
        m.ao_ch as f32,
        m.invert_rough as u8 as f32,
        m.ao_strength,
        m.emissive,
        0.0,
        0.0,
    ]
}

/// Tangent (direction of increasing u) and bitangent sign for a triangle.
fn tangent(p: [DVec3; 3], uv: [[f32; 2]; 3], n: DVec3, fallback: DVec3) -> [f32; 4] {
    let e1 = p[1] - p[0];
    let e2 = p[2] - p[0];
    let (du1, dv1) = ((uv[1][0] - uv[0][0]) as f64, (uv[1][1] - uv[0][1]) as f64);
    let (du2, dv2) = ((uv[2][0] - uv[0][0]) as f64, (uv[2][1] - uv[0][1]) as f64);
    let det = du1 * dv2 - du2 * dv1;
    if det.abs() < 1e-12 {
        let t = fallback.normalize_or_zero();
        return [t.x as f32, t.y as f32, t.z as f32, 1.0];
    }
    let r = 1.0 / det;
    let t = ((e1 * dv2 - e2 * dv1) * r).normalize_or_zero();
    let b = (e2 * du1 - e1 * du2) * r;
    let w = if n.cross(t).dot(b) < 0.0 { -1.0 } else { 1.0 };
    [t.x as f32, t.y as f32, t.z as f32, w]
}

pub fn build(doc: &Document, o: &SceneOpts) -> Scene3D {
    let mut s = Scene3D::default();
    let in_context = !o.path.is_empty();
    let st = &doc.style;
    // a normal facing the shader's light gives exactly the base color (unlit look)
    let flat_n = DVec3::new(0.35, 0.55, 0.75).normalize().as_vec3().to_array();
    let mut mat_slots: HashMap<usize, usize> = HashMap::new();
    let mut mats: Vec<crate::render::GpuMat> = Vec::new();
    doc.visit(o.path, &mut |it| {
        let g = it.geom;
        let outside = in_context && it.owner == Owner::Outside;
        let inst_sel = matches!(it.owner, Owner::Inst(i) if o.show_sel && o.sel.insts.contains(&i));
        let inst_hover = matches!(it.owner, Owner::Inst(i) if o.highlight_inst == Some(i));
        let nm = it.xf.inverse().transpose();
        let flip = it.xf.determinant() < 0.0;
        let smooth = smooth_normals(g);
        for (fi, f) in g.faces.iter().enumerate() {
            let ghost = !doc.face_visible(f);
            if (ghost && !o.hidden_geometry) || st.face == FaceStyle::Wireframe {
                continue;
            }
            let active = it.owner == Owner::Active;
            let selected = active && o.show_sel && o.sel.faces.contains(&fi);
            let mat_rgba = |m: Option<usize>| m.filter(|m| *m < doc.materials.len()).map(|m| (rgb(doc.materials[m].color), doc.materials[m].alpha));
            let (mut front, mut back, mut alpha) = match st.face {
                FaceStyle::Monochrome => (rgb(FRONT), rgb(BACK), 1.0),
                FaceStyle::HiddenLine => (rgb(st.bg), rgb(st.bg), 1.0),
                _ => match mat_rgba(f.material.or(it.mat)) {
                    Some((c, a)) => {
                        let both = f.material.is_none();
                        let b = mat_rgba(f.back_material).map(|x| x.0).unwrap_or(if both { c } else { rgb(BACK) });
                        (c, b, a)
                    }
                    None => (rgb(FRONT), mat_rgba(f.back_material).map(|x| x.0).unwrap_or(rgb(BACK)), 1.0),
                },
            };
            if o.color_by_tag {
                if let Some(t) = f.tag.or(it.tag).and_then(|t| doc.tags.get(t)) {
                    front = rgb(t.color);
                    back = front;
                }
            }
            if selected {
                front = rgb(SEL);
                back = rgb(SEL);
            } else if (active && o.highlight_face == Some(fi)) || inst_hover {
                front = rgb(HOVER);
                back = rgb(HOVER);
            } else if inst_sel {
                front = [front[0] * 0.75, front[1] * 0.8, front[2] * 0.75 + 0.25, 1.0];
            }
            if outside {
                front = fade(front);
                back = fade(back);
            }
            if st.xray {
                alpha = alpha.min(0.45);
            }
            if ghost {
                alpha = 0.22;
            }
            // material slot (PBR record), tile size and whether the texture shows on both sides
            let mut layer = -1.0f32;
            let mut tile = [1.0f64, 1.0];
            let pbr_style = matches!(st.face, FaceStyle::Shaded | FaceStyle::Photoreal);
            if pbr_style && !selected && !(active && o.highlight_face == Some(fi)) && !inst_hover && !o.color_by_tag {
                if let Some(mi) = f.material.or(it.mat).filter(|m| *m < doc.materials.len()) {
                    let m = &doc.materials[mi];
                    let both = f.material.is_none();
                    let slot = *mat_slots.entry(mi).or_insert_with(|| {
                        mats.push(gpu_mat(doc, m));
                        mats.len() - 1
                    });
                    layer = slot as f32 + if both { 1000.0 } else { 0.0 };
                    tile = m.tex_size;
                    if m.texture.is_some_and(|t| t < doc.textures.len() && t < crate::tex::MAX_LAYERS) {
                        let tint = if inst_sel { [0.8, 0.85, 1.0] } else { [1.0, 1.0, 1.0] };
                        front = [tint[0], tint[1], tint[2], 1.0];
                        if both {
                            back = front;
                        }
                        if outside {
                            front = fade(front);
                            back = fade(back);
                        }
                    }
                }
            }
            let uvmap: Option<HashMap<VId, usize>> = (layer >= 0.0 && f.uv_at(0).is_some()).then(|| {
                let mut m = HashMap::new();
                for (k, v) in f.loops().flatten().enumerate() {
                    m.entry(*v).or_insert(k);
                }
                m
            });
            let wn = nm.transform_vector3(f.normal).normalize_or_zero();
            let (pu, pv) = if wn.z.abs() > 0.9 {
                (DVec3::X, DVec3::Y)
            } else {
                let u = DVec3::Z.cross(wn).normalize_or_zero();
                (u, wn.cross(u))
            };
            front[3] = alpha;
            back[3] = alpha;
            let unlit = st.face == FaceStyle::HiddenLine;
            let target = if alpha < 0.999 {
                &mut s.glass
            } else if it.cast {
                &mut s.tris
            } else {
                &mut s.nocast
            };
            for t in g.triangulate_face(f) {
                let order = if flip { [t[0], t[2], t[1]] } else { t };
                let mut tv = [Vertex::plain([0.0; 3], [0.0; 3], front, back); 3];
                let mut wps = [DVec3::ZERO; 3];
                let mut qs = [1.0f32; 3];
                for (k, v) in order.into_iter().enumerate() {
                    let nrm = if unlit {
                        flat_n
                    } else {
                        let n = smooth.as_ref().and_then(|m| m.get(&(fi, v)).copied()).unwrap_or(f.normal);
                        nm.transform_vector3(n).normalize_or_zero().as_vec3().to_array()
                    };
                    let wp = it.xf.transform_point3(g.p(v));
                    let (uv, q) = if layer < 0.0 {
                        ([0.0; 2], 1.0)
                    } else if let Some(k) = uvmap.as_ref().and_then(|m| m.get(&v)) {
                        (f.uv_at(*k).unwrap_or([0.0; 2]), f.uvq_at(*k).max(1e-6))
                    } else {
                        ([(wp.dot(pu) / tile[0]) as f32, (-wp.dot(pv) / tile[1]) as f32], 1.0)
                    };
                    wps[k] = wp;
                    qs[k] = q;
                    tv[k] = Vertex { pos: wp.as_vec3().to_array(), nrm, col: front, back, uv, layer, tan: [1.0, 0.0, 0.0, 1.0] };
                }
                if layer >= 0.0 {
                    let plain = [0, 1, 2].map(|k| [tv[k].uv[0] / qs[k], tv[k].uv[1] / qs[k]]);
                    let tan = tangent(wps, plain, wn, pu);
                    for (k, v) in tv.iter_mut().enumerate() {
                        // |tan.w| carries the projective weight, its sign the bitangent direction
                        v.tan = [tan[0], tan[1], tan[2], tan[3] * qs[k]];
                    }
                }
                target.extend_from_slice(&tv);
            }
        }
        let edge = rgb(st.edge_color);
        let mut edge_mats: Option<HashMap<(VId, VId), Option<usize>>> = None;
        let wide = st.depth_cue > 0.0 || st.endpoints > 0.0 || st.jitter > 0.0;
        let blue = [0.12, 0.31, 1.0, 1.0];
        let faded = fade([0.25, 0.25, 0.25, 1.0]);
        let ghost_c = [0.55, 0.55, 0.6, 1.0];
        for (ei, e) in g.edges.iter().enumerate() {
            let ghost = e.hidden || e.soft;
            if ghost && !o.hidden_geometry {
                continue;
            }
            if e.tag.is_some() && !doc.tag_visible(e.tag) && !doc.show_hidden {
                continue;
            }
            let selected = (it.owner == Owner::Active && o.show_sel && o.sel.edges.contains(&ei)) || inst_sel;
            if !st.edges && !selected && st.face != FaceStyle::Wireframe && st.face != FaceStyle::HiddenLine {
                continue;
            }
            let (a, b) = (it.xf.transform_point3(g.p(e.v[0])), it.xf.transform_point3(g.p(e.v[1])));
            let styled = match st.edge_mode {
                EdgeColor::Same => edge,
                EdgeColor::Axis => {
                    let d = (b - a).normalize_or_zero();
                    let ax = crate::pick::axes();
                    if d.dot(ax[0]).abs() > 0.9995 {
                        [0.85, 0.1, 0.1, 1.0]
                    } else if d.dot(ax[1]).abs() > 0.9995 {
                        [0.1, 0.6, 0.1, 1.0]
                    } else if d.dot(ax[2]).abs() > 0.9995 {
                        [0.1, 0.2, 0.9, 1.0]
                    } else {
                        edge
                    }
                }
                EdgeColor::Material => {
                    let key = (e.v[0].min(e.v[1]), e.v[0].max(e.v[1]));
                    match edge_mats.get_or_insert_with(|| edge_materials(g)).get(&key).copied().flatten().or(it.mat).and_then(|m| doc.materials.get(m)) {
                        Some(m) => {
                            let c = rgb(m.color);
                            [c[0] * 0.6, c[1] * 0.6, c[2] * 0.6, 1.0]
                        }
                        None => edge,
                    }
                }
            };
            let c = if selected {
                blue
            } else if ghost {
                ghost_c
            } else if outside {
                faded
            } else {
                styled
            };
            if doc.shadows.on && doc.shadows.from_edges && !ghost && !outside {
                let key = (e.v[0].min(e.v[1]), e.v[0].max(e.v[1]));
                if !edge_mats.get_or_insert_with(|| edge_materials(g)).contains_key(&key) {
                    for p in [a, b] {
                        s.shadow_lines.push(Vertex::plain(p.as_vec3().to_array(), [0.0; 3], c, c));
                    }
                }
            }
            let ext = if st.extension > 0.0 && !ghost && !selected && st.edges { st.extension } else { 0.0 };
            let dash = st.dashes.then(|| e.tag.or(it.tag).and_then(|t| doc.tags.get(t)).map(|t| t.dash.code())).flatten().filter(|c| *c > 0.0);
            if wide {
                push_wide(&mut s, a, b, c, (ei as f32 * 0.37).fract() * 100.0, ext, st.endpoints > 0.0 && !ghost && !selected, dash.unwrap_or(0.0));
                continue;
            }
            push_line(&mut s, a, b, c);
            if let Some(code) = dash {
                let n = s.lines.len();
                for v in &mut s.lines[n - 2..] {
                    v.layer = -10.0 - code;
                    v.back = [a.x as f32, a.y as f32, a.z as f32, 1.0];
                }
            }
            if ext > 0.0 {
                // extension segments, stretched in screen space by the line shader
                for (p, q) in [(a, b), (b, a)] {
                    let mk = |moving: f32| Vertex {
                        pos: p.as_vec3().to_array(),
                        nrm: [0.0; 3],
                        col: c,
                        back: [q.x as f32, q.y as f32, q.z as f32, 1.0],
                        uv: [st.extension, moving],
                        layer: -2.0,
                        tan: [1.0, 0.0, 0.0, 1.0],
                    };
                    s.lines.push(mk(0.0));
                    s.lines.push(mk(1.0));
                }
            }
        }
    });
    s.mats = mats;
    // selected instances: blue bounding boxes
    let ctx = doc.xf(o.path);
    let active = doc.geom(o.path);
    for i in &o.sel.insts {
        if let Some(inst) = active.instances.get(*i) {
            if let Some((lo, hi)) = doc.def_bounds(inst.def) {
                box_lines(&mut s, lo, hi, ctx * doc.eff_xf(inst), [0.12, 0.31, 1.0, 1.0]);
            }
        }
    }
    if in_context {
        if let Some((lo, hi)) = doc.geom_bounds(active, DMat4::IDENTITY) {
            box_lines(&mut s, lo, hi, ctx, [0.45, 0.45, 0.45, 1.0]);
        }
    }
    // axes
    let len = 2000.0;
    let axis = [[0.86, 0.0, 0.0, 1.0], [0.0, 0.63, 0.0, 1.0], [0.0, 0.0, 0.9, 1.0]];
    let ax = doc.axes;
    for (i, c) in axis.iter().enumerate().filter(|_| o.axes) {
        let a = [ax.x, ax.y, ax.z][i];
        // negative half is drawn dotted by the 2D overlay
        push_line(&mut s, ax.origin, ax.origin + a * len, *c);
    }
    s
}

#[cfg(test)]
mod dash_tests {
    use super::*;
    #[test]
    fn tagged_instance_edges_are_dashed() {
        let mut doc = Document::with_scale_figure();
        doc.tags.push(crate::settings::Tag { name: "t".into(), visible: true, color: [1, 2, 3], dash: crate::hatch::Dash::Dotted });
        doc.root.instances[0].tag = Some(0);
        let sel = Sel::default();
        let o = SceneOpts { path: &[], sel: &sel, show_sel: false, highlight_face: None, highlight_inst: None, axes: false, color_by_tag: false, hidden_geometry: false };
        let s = build(&doc, &o);
        assert!(s.lines.iter().any(|v| v.layer == -12.0), "dotted edges marked");
        doc.style.jitter = 1.0;
        let s = build(&doc, &o);
        assert!(s.wide.iter().any(|v| v.tan[1] == 2.0), "wide (sketch) edges carry the dash code");
        doc.style.jitter = 0.0;
        doc.style.dashes = false;
        let s = build(&doc, &o);
        assert!(s.lines.iter().all(|v| v.layer > -9.5));
    }
}
