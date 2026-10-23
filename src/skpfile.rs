//! Native reader for SketchUp .skp files (SketchUp 2021+ container format),
//! written from analysis of sample files only — no SketchUp code or SDK.
//!
//! Layout found in the samples:
//! - a UTF-16 header ("SketchUp Model", "{25.0.0}" …) followed by a ZIP archive;
//! - the ZIP holds `model.dat` (the model), `materials/<name>/material.xml`
//!   (+ texture images), styles, thumbnails;
//! - `model.dat` is a tree of records `u16 tag, u32 length, payload`; payloads of
//!   container tags are again record lists. IDs are little-endian integers whose
//!   width is the record length.
//!
//! Tags used (container → children):
//! - 0x01F6 root → 0x1388 entities;  0x01F9 → 0x1770 → 0x1771 → 0x157C definition
//! - 0x157C: 0x1388 entities, 0x157E name, 0x1582 always-face-camera
//! - 0x1388 entities: 0x07D0 header (id), 0x1389 vertices, 0x1396/0x1397 curves/arcs,
//!   0x138A edges, 0x138B faces, 0x138C instances
//! - 0x07D0 entity header: 0x05DC→0x05DE id, 0x07D1 material, 0x07D2 tag, 0x07D3 flags
//! - 0x09C4 vertex: 0x09C5 position (3×f64, inches)
//! - 0x0BB8 edge: 0x0BB9 start vertex, 0x0BBA end vertex, 0x0BBB curve
//! - 0x0DAC face: 0x0DAD plane (4×f64), 0x0DAF back material, 0x0DAE loops →
//!   0x1194 → 0x1195 → 0x0FA0 (0x0FA1 edge, 0x0FA2 reversed)
//! - 0x1964 instance: 0x1965 name, 0x1966 transform (13×f64: x, y, z axes, origin, w),
//!   0x1967 definition (its entities id)
//! - 0x4A38 curve (first/last edge); 0x4C2C arc (curve + parameters)
//! - 0x32C8 material: 0x32CC name, 0x32CE → 0x2134 thumbnail path (folder in the ZIP)
//! - 0x3C8C tag: 0x3C8D name;  0x34BC camera;  0x6590 location

use crate::doc::{Definition, Document, Material};
use crate::geom::{Edge, Face, Geom, Instance, VId};
use crate::settings::Tag;
use glam::{DMat4, DVec3, DVec4};
use std::collections::HashMap;
use std::io::Read;

const INCH: f64 = 0.0254;

#[derive(Clone, Copy, Debug)]
struct Rec {
    tag: u16,
    start: usize,
    len: usize,
}

struct Dat<'a> {
    b: &'a [u8],
}

impl<'a> Dat<'a> {
    /// Children of a record, if its payload is a complete record list.
    fn kids(&self, r: Rec) -> Vec<Rec> {
        self.list(r.start, r.start + r.len).unwrap_or_default()
    }
    fn list(&self, start: usize, end: usize) -> Option<Vec<Rec>> {
        let mut out = Vec::new();
        let mut p = start;
        while p < end {
            if p + 6 > end {
                return None;
            }
            let tag = u16::from_le_bytes([self.b[p], self.b[p + 1]]);
            let len = u32::from_le_bytes([self.b[p + 2], self.b[p + 3], self.b[p + 4], self.b[p + 5]]) as usize;
            if p + 6 + len > end {
                return None;
            }
            out.push(Rec { tag, start: p + 6, len });
            p += 6 + len;
        }
        Some(out)
    }
    fn child(&self, r: Rec, tag: u16) -> Option<Rec> {
        self.kids(r).into_iter().find(|c| c.tag == tag)
    }
    fn bytes(&self, r: Rec) -> &'a [u8] {
        &self.b[r.start..r.start + r.len]
    }
    fn int(&self, r: Rec) -> u64 {
        self.bytes(r).iter().take(8).rev().fold(0u64, |a, b| (a << 8) | *b as u64)
    }
    fn f64s(&self, r: Rec) -> Vec<f64> {
        self.bytes(r).chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect()
    }
    fn text(&self, r: Rec) -> String {
        String::from_utf8_lossy(self.bytes(r)).to_string()
    }
    /// Records with `tag` anywhere below `r` (not descending into matches).
    fn find_all(&self, r: Rec, tag: u16, out: &mut Vec<Rec>, depth: usize) {
        if depth > 12 {
            return;
        }
        for c in self.kids(r) {
            if c.tag == tag {
                out.push(c);
            } else if c.len >= 6 {
                self.find_all(c, tag, out, depth + 1);
            }
        }
    }
}

#[derive(Default, Clone, Copy)]
struct Header {
    id: u64,
    material: Option<u64>,
    tag: Option<u64>,
    flags: u8,
}

fn header(d: &Dat, r: Rec) -> Header {
    let mut h = Header::default();
    if let Some(hd) = d.child(r, 0x07d0) {
        for c in d.kids(hd) {
            match c.tag {
                0x05dc => {
                    if let Some(i) = d.child(c, 0x05de) {
                        h.id = d.int(i);
                    }
                }
                0x07d1 => h.material = Some(d.int(c)),
                0x07d2 => h.tag = Some(d.int(c)),
                0x07d3 => h.flags = d.int(c) as u8,
                _ => {}
            }
        }
    }
    h
}

fn own_id(d: &Dat, r: Rec) -> Option<u64> {
    d.child(r, 0x05dc).and_then(|c| d.child(c, 0x05de)).map(|i| d.int(i))
}

/// Camera stored in the file (inches).
#[derive(Clone, Copy, Debug)]
pub struct SkpCamera {
    pub eye: DVec3,
    pub target: DVec3,
    pub up: DVec3,
    pub fov_deg: f64,
    pub perspective: bool,
}

pub struct SkpModel {
    pub doc: Document,
    pub camera: Option<SkpCamera>,
    pub version: String,
    pub stats: String,
}

struct Ctx<'a> {
    d: Dat<'a>,
    /// entities ids used by groups (0x138D), collected while reading
    groups: std::cell::RefCell<std::collections::HashSet<u64>>,
    materials: HashMap<u64, usize>,
    tags: HashMap<u64, usize>,
    /// entities id → definition index
    defs: HashMap<u64, usize>,
}

fn read_entities(cx: &Ctx, ents: Rec) -> Geom {
    let d = &cx.d;
    let groups = &cx.groups;
    let mut g = Geom::default();
    let mut vmap: HashMap<u64, VId> = HashMap::new();
    let mut emap: HashMap<u64, usize> = HashMap::new();
    let mut curves: HashMap<u64, u32> = HashMap::new();
    let mut arcs: std::collections::HashSet<u64> = Default::default();
    for c in d.kids(ents) {
        match c.tag {
            0x1389 => {
                for v in d.kids(c).into_iter().filter(|v| v.tag == 0x09c4) {
                    let h = header(d, v);
                    let id = own_id(d, v).unwrap_or(h.id);
                    if let Some(p) = d.child(v, 0x09c5) {
                        let f = d.f64s(p);
                        if f.len() >= 3 {
                            g.verts.push(DVec3::new(f[0], f[1], f[2]) * INCH);
                            vmap.insert(id, (g.verts.len() - 1) as VId);
                        }
                    }
                }
            }
            0x1397 => {
                // arcs: the curve record inside marks its edges as arc edges
                for a in d.kids(c).into_iter().filter(|a| a.tag == 0x4c2c) {
                    if let Some(cv) = d.child(a, 0x4a38) {
                        if let Some(i) = own_id(d, cv) {
                            arcs.insert(i);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let mut next_curve = 0u32;
    for c in d.kids(ents).into_iter().filter(|c| c.tag == 0x138a) {
        for e in d.kids(c).into_iter().filter(|e| e.tag == 0x0bb8) {
            let h = header(d, e);
            let (Some(a), Some(b)) = (d.child(e, 0x0bb9).map(|r| d.int(r)), d.child(e, 0x0bba).map(|r| d.int(r))) else { continue };
            let (Some(&va), Some(&vb)) = (vmap.get(&a), vmap.get(&b)) else { continue };
            if va == vb {
                continue;
            }
            let mut edge = Edge::new(va, vb);
            edge.soft = h.flags & 0x01 != 0;
            edge.hidden = h.flags & 0x08 != 0;
            edge.tag = h.tag.and_then(|t| cx.tags.get(&t).copied());
            if let Some(cv) = d.child(e, 0x0bbb).map(|r| d.int(r)) {
                let k = *curves.entry(cv).or_insert_with(|| {
                    next_curve += 1;
                    next_curve
                });
                edge.curve = Some(k);
                edge.arc = arcs.contains(&cv);
            }
            g.edges.push(edge);
            emap.insert(h.id, g.edges.len() - 1);
        }
    }
    g.next_curve = next_curve;
    for c in d.kids(ents).into_iter().filter(|c| c.tag == 0x138b) {
        for f in d.kids(c).into_iter().filter(|f| f.tag == 0x0dac) {
            let h = header(d, f);
            let plane = d.child(f, 0x0dad).map(|p| d.f64s(p)).unwrap_or_default();
            if plane.len() < 3 {
                continue;
            }
            let n = DVec3::new(plane[0], plane[1], plane[2]).normalize_or_zero();
            let mut loops: Vec<Vec<VId>> = Vec::new();
            if let Some(ls) = d.child(f, 0x0dae) {
                for l in d.kids(ls).into_iter().filter(|l| l.tag == 0x1194) {
                    let Some(inner) = d.child(l, 0x1195) else { continue };
                    let mut lp: Vec<VId> = Vec::new();
                    for u in d.kids(inner).into_iter().filter(|u| u.tag == 0x0fa0) {
                        let (Some(er), rev) = (d.child(u, 0x0fa1).map(|r| d.int(r)), d.child(u, 0x0fa2).map(|r| d.int(r) != 0).unwrap_or(false)) else { continue };
                        let Some(&ei) = emap.get(&er) else { continue };
                        let [a, b] = g.edges[ei].v;
                        lp.push(if rev { b } else { a });
                    }
                    if lp.len() >= 3 {
                        loops.push(lp);
                    }
                }
            }
            if loops.is_empty() {
                continue;
            }
            // outer loop counter-clockwise about the normal, holes clockwise
            let newell = |l: &[VId]| {
                let mut s = DVec3::ZERO;
                for k in 0..l.len() {
                    let (p, q) = (g.p(l[k]), g.p(l[(k + 1) % l.len()]));
                    s += p.cross(q);
                }
                s
            };
            let outer_k = (0..loops.len()).max_by(|a, b| newell(&loops[*a]).length().partial_cmp(&newell(&loops[*b]).length()).unwrap()).unwrap();
            let mut outer = loops[outer_k].clone();
            if newell(&outer).dot(n) < 0.0 {
                outer.reverse();
            }
            let holes: Vec<Vec<VId>> = loops
                .iter()
                .enumerate()
                .filter(|(k, _)| *k != outer_k)
                .map(|(_, l)| {
                    let mut l = l.clone();
                    if newell(&l).dot(n) > 0.0 {
                        l.reverse();
                    }
                    l
                })
                .collect();
            g.faces.push(Face {
                outer,
                holes,
                normal: n,
                material: h.material.and_then(|m| cx.materials.get(&m).copied()),
                back_material: d.child(f, 0x0daf).and_then(|r| cx.materials.get(&d.int(r)).copied()),
                tag: h.tag.and_then(|t| cx.tags.get(&t).copied()),
                hidden: h.flags & 0x01 != 0,
                ..Default::default()
            });
        }
    }
    // 0x138C: component instances; 0x138D: groups (0x1D4C wrapping an instance)
    let mut insts: Vec<(Rec, bool)> = Vec::new();
    for c in d.kids(ents) {
        match c.tag {
            0x138c => insts.extend(d.kids(c).into_iter().filter(|i| i.tag == 0x1964).map(|i| (i, false))),
            0x138d => {
                for w in d.kids(c).into_iter().filter(|w| w.tag == 0x1d4c) {
                    insts.extend(d.kids(w).into_iter().filter(|i| i.tag == 0x1964).map(|i| (i, true)));
                }
            }
            _ => {}
        }
    }
    {
        for (i, is_group) in insts {
            if is_group {
                groups.borrow_mut().insert(d.child(i, 0x1967).map(|r| d.int(r)).unwrap_or(0));
            }
            let h = header(d, i);
            let Some(def) = d.child(i, 0x1967).and_then(|r| cx.defs.get(&d.int(r)).copied()) else { continue };
            let t = d.child(i, 0x1966).map(|r| d.f64s(r)).unwrap_or_default();
            let xf = if t.len() >= 13 {
                let w = if t[12].abs() > 1e-12 { t[12] } else { 1.0 };
                DMat4::from_cols(
                    DVec4::new(t[0], t[1], t[2], 0.0) / w,
                    DVec4::new(t[3], t[4], t[5], 0.0) / w,
                    DVec4::new(t[6], t[7], t[8], 0.0) / w,
                    DVec4::new(t[9] * INCH / w, t[10] * INCH / w, t[11] * INCH / w, 1.0),
                )
            } else {
                DMat4::IDENTITY
            };
            g.instances.push(Instance {
                def,
                xf,
                material: h.material.and_then(|m| cx.materials.get(&m).copied()),
                name: d.child(i, 0x1965).map(|r| d.text(r)).unwrap_or_default(),
                tag: h.tag.and_then(|t| cx.tags.get(&t).copied()),
                hidden: h.flags & 0x01 != 0,
                ..Default::default()
            });
        }
    }
    g
}

fn attr<'s>(xml: &'s str, el: &str, name: &str) -> Option<&'s str> {
    let i = xml.find(&format!("<{el} "))?;
    let end = xml[i..].find('>')? + i;
    let tag = &xml[i..end];
    let key = format!(" {name}=\"");
    let a = tag.find(&key)? + key.len();
    let b = tag[a..].find('"')? + a;
    Some(&tag[a..b])
}

fn xml_unescape(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// Material from `materials/<folder>/material.xml` (+ texture files).
fn read_material(zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str, folder: &str, textures: &mut Vec<crate::tex::TextureData>) -> Material {
    let mut file = |p: &str| -> Option<Vec<u8>> {
        let mut f = zip.by_name(p).ok()?;
        let mut v = Vec::new();
        f.read_to_end(&mut v).ok()?;
        Some(v)
    };
    let mut m = Material::color(name, [200, 200, 200], 1.0);
    let Some(xml) = file(&format!("{folder}/material.xml")).map(|b| String::from_utf8_lossy(&b).to_string()) else { return m };
    let num = |el: &str, a: &str| attr(&xml, el, a).and_then(|v| v.parse::<f64>().ok());
    let c = |a: &str| num("mat:material", a).unwrap_or(200.0).clamp(0.0, 255.0) as u8;
    m.color = [c("colorRed"), c("colorGreen"), c("colorBlue")];
    if attr(&xml, "mat:material", "useTrans") == Some("1") {
        m.alpha = num("mat:material", "trans").unwrap_or(1.0).clamp(0.05, 1.0) as f32;
    }
    let tex = |file_name: Option<&str>, textures: &mut Vec<crate::tex::TextureData>, file: &mut dyn FnMut(&str) -> Option<Vec<u8>>| -> Option<usize> {
        let fname = xml_unescape(file_name?);
        let bytes = file(&format!("{folder}/{fname}"))?;
        let t = crate::tex::TextureData::from_bytes(&fname, bytes).ok()?;
        textures.push(t);
        Some(textures.len() - 1)
    };
    if attr(&xml, "mat:material", "hasTexture") == Some("1") {
        if let Some(t) = tex(attr(&xml, "mat:image", "file_name"), textures, &mut file) {
            m.texture = Some(t);
            let sx = num("mat:texture", "xScale").unwrap_or(39.37) * INCH;
            let sy = num("mat:texture", "yScale").unwrap_or(39.37) * INCH;
            m.tex_size = [sx.max(0.001), sy.max(0.001)];
        }
    }
    // photoreal maps
    let flag = |k: &str| xml.contains(&format!("<mat:{k}>1</mat:{k}>"));
    let value = |k: &str| -> Option<f32> {
        let a = xml.find(&format!("<mat:{k}>"))? + k.len() + 6;
        let b = xml[a..].find('<')? + a;
        xml[a..b].trim().parse().ok()
    };
    if flag("enable_metalness") {
        m.metalness = value("metallicFactor").unwrap_or(1.0);
        m.metal_tex = tex(attr(&xml, "mat:metallic_texture", "file_name"), textures, &mut file);
    }
    if flag("enable_roughness") {
        m.roughness = value("roughnessFactor").unwrap_or(1.0);
        m.rough_tex = tex(attr(&xml, "mat:roughness_texture", "file_name"), textures, &mut file);
        m.invert_rough = value("roughness_texture_invert").is_some_and(|v| v > 0.5);
    }
    if flag("enable_normal") {
        m.normal_tex = tex(attr(&xml, "mat:normal_texture", "file_name"), textures, &mut file);
        m.normal_scale = value("normalScale").unwrap_or(1.0).min(4.0) * 0.5;
    }
    if flag("enable_occlusion") {
        m.ao_tex = tex(attr(&xml, "mat:occlusion_texture", "file_name"), textures, &mut file);
        m.ao_strength = value("occlusionStrength").unwrap_or(1.0);
    }
    m.source = "SketchUp (.skp)".into();
    m
}

/// Read a .skp file into a HomeUp document.
pub fn read(bytes: &[u8]) -> Result<SkpModel, String> {
    let head: Vec<u16> = bytes.get(4..32).unwrap_or(&[]).chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    if !String::from_utf16_lossy(&head).starts_with("SketchUp Model") {
        return Err("não é um arquivo do SketchUp".into());
    }
    let zip_at = bytes.windows(4).position(|w| w == b"PK\x03\x04").ok_or("este .skp é de uma versão antiga do SketchUp (anterior a 2021), ainda não suportada")?;
    let version = {
        let h = &bytes[..zip_at];
        let txt: String = h.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).filter_map(|u| char::from_u32(u as u32)).collect();
        txt.split('{').nth(1).and_then(|s| s.split('}').next()).unwrap_or("?").to_string()
    };
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes[zip_at..])).map_err(|e| format!("ZIP inválido: {e}"))?;
    let mut model = Vec::new();
    zip.by_name("model.dat").map_err(|_| "model.dat ausente")?.read_to_end(&mut model).map_err(|e| e.to_string())?;
    let d = Dat { b: &model };
    let top = d.list(0, model.len()).and_then(|l| l.into_iter().next()).ok_or("model.dat inválido")?;
    let mut doc = Document::default();
    // materials
    let mut materials = HashMap::new();
    let mut mats = Vec::new();
    d.find_all(top, 0x32c8, &mut mats, 0);
    for r in mats {
        let Some(id) = own_id(&d, r) else { continue };
        if materials.contains_key(&id) {
            continue;
        }
        let name = d.child(r, 0x32cc).map(|n| d.text(n)).unwrap_or_default();
        let folder = d
            .child(r, 0x32ce)
            .and_then(|t| d.child(t, 0x2134))
            .map(|p| d.text(p))
            .and_then(|p| p.rsplit_once('/').map(|(a, _)| a.to_string()))
            .unwrap_or_else(|| format!("materials/{name}"));
        let m = read_material(&mut zip, &name, &folder, &mut doc.textures);
        doc.materials.push(m);
        materials.insert(id, doc.materials.len() - 1);
    }
    // dash styles (0x4076: name + pattern) referenced by tags
    let mut dashes: HashMap<u64, crate::hatch::Dash> = HashMap::new();
    let mut drs = Vec::new();
    d.find_all(top, 0x4076, &mut drs, 0);
    for r in drs {
        let Some(id) = own_id(&d, r) else { continue };
        let name = d.child(r, 0x4077).map(|n| d.text(n).to_lowercase()).unwrap_or_default();
        use crate::hatch::Dash;
        let dash = if name.contains("solid") {
            Dash::Solid
        } else if name.contains("long") {
            Dash::LongDash
        } else if name.contains("dash") && name.contains("dot") {
            Dash::DashDot
        } else if name.contains("dot") {
            Dash::Dotted
        } else {
            Dash::Dashed
        };
        dashes.insert(id, dash);
    }
    // tags (layers)
    let mut tags = HashMap::new();
    let mut lrs = Vec::new();
    d.find_all(top, 0x3c8c, &mut lrs, 0);
    for r in lrs {
        let Some(id) = own_id(&d, r) else { continue };
        let name = d.child(r, 0x3c8d).map(|n| d.text(n)).unwrap_or_default();
        if name == "Layer0" || name == "Untagged" {
            continue; // the default tag is "no tag" in HomeUp
        }
        let color = d.child(r, 0x3c8f).and_then(|m| d.child(m, 0x32c8)).and_then(|m| own_id(&d, m)).and_then(|i| materials.get(&i)).map(|k| doc.materials[*k].color).unwrap_or([180, 180, 180]);
        let dash = d.child(r, 0x3c91).and_then(|x| dashes.get(&d.int(x)).copied()).unwrap_or_default();
        doc.tags.push(Tag { name, visible: true, color, dash });
        tags.insert(id, doc.tags.len() - 1);
    }
    // definitions: allocate indices first (instances may reference any of them)
    let mut def_recs = Vec::new();
    if let Some(defs_root) = d.child(top, 0x01f9) {
        d.find_all(defs_root, 0x157c, &mut def_recs, 0);
    }
    let mut cx = Ctx { d: Dat { b: &model }, groups: Default::default(), materials, tags, defs: HashMap::new() };
    for (k, r) in def_recs.iter().enumerate() {
        if let Some(ents) = cx.d.child(*r, 0x1388) {
            cx.defs.insert(header(&cx.d, ents).id, k);
        }
    }
    for r in &def_recs {
        let name = cx.d.child(*r, 0x157e).map(|n| cx.d.text(n)).unwrap_or_default();
        let face_me = cx.d.child(*r, 0x1582).is_some_and(|f| cx.d.int(f) != 0);
        let geom = cx.d.child(*r, 0x1388).map(|e| read_entities(&cx, e)).unwrap_or_default();
        doc.defs.push(Definition { name, geom, group: false, face_me, procedure: None, ifc: None, author: None, light: None });
    }
    if let Some(root) = cx.d.child(top, 0x01f6).and_then(|r| cx.d.child(r, 0x1388)) {
        doc.root = read_entities(&cx, root);
    }
    // definitions placed through the groups list are groups
    for (ents_id, k) in &cx.defs {
        if cx.groups.borrow().contains(ents_id) {
            doc.defs[*k].group = true;
        }
    }
    // camera
    let camera = cx.d.child(top, 0x01fa).and_then(|c| cx.d.child(c, 0x34bc)).and_then(|c| {
        let v = |t: u16| cx.d.child(c, t).map(|r| cx.d.f64s(r)).filter(|f| f.len() >= 3).map(|f| DVec3::new(f[0], f[1], f[2]));
        Some(SkpCamera {
            eye: v(0x34bd)? * INCH,
            target: v(0x34be)? * INCH,
            up: v(0x34bf)?,
            fov_deg: cx.d.child(c, 0x34c4).map(|r| cx.d.f64s(r)).and_then(|f| f.first().copied()).unwrap_or(35.0),
            perspective: cx.d.child(c, 0x34c2).is_none_or(|r| cx.d.int(r) != 0),
        })
    });
    // location
    if let Some(l) = cx.d.child(top, 0x0204).and_then(|r| cx.d.child(r, 0x6590)) {
        let f = |t: u16| cx.d.child(l, t).map(|r| cx.d.f64s(r)).and_then(|v| v.first().copied());
        let city = cx.d.child(l, 0x6593).map(|r| cx.d.text(r)).unwrap_or_default();
        let country = cx.d.child(l, 0x6594).map(|r| cx.d.text(r)).unwrap_or_default();
        if let (Some(lon), Some(lat)) = (f(0x6595), f(0x6596)) {
            doc.location.lat = lat;
            doc.location.lon = lon;
            doc.location.name = [city, country].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ");
        }
        if let Some(tz) = f(0x6597) {
            doc.shadows.tz = tz;
        }
    }
    let count = |f: &dyn Fn(&Geom) -> usize| f(&doc.root) + doc.defs.iter().map(|x| f(&x.geom)).sum::<usize>();
    let stats = format!(
        "{} faces, {} arestas, {} componentes/grupos, {} materiais ({} imagens de textura), {} etiquetas",
        count(&|g| g.faces.len()),
        count(&|g| g.edges.len()),
        doc.defs.len(),
        doc.materials.len(),
        doc.textures.len(),
        doc.tags.len()
    );
    Ok(SkpModel { doc, camera, version, stats })
}

/// Shift every material / texture / tag / definition index of an imported
/// document so it can be appended to another one.
fn shift(m: &mut SkpModel, mats: usize, texs: usize, tags: usize, defs: usize) {
    let fix_geom = |g: &mut Geom| {
        for e in &mut g.edges {
            e.tag = e.tag.map(|x| x + tags);
        }
        for f in &mut g.faces {
            f.material = f.material.map(|x| x + mats);
            f.back_material = f.back_material.map(|x| x + mats);
            f.tag = f.tag.map(|x| x + tags);
        }
        for i in &mut g.instances {
            i.def += defs;
            i.material = i.material.map(|x| x + mats);
            i.tag = i.tag.map(|x| x + tags);
        }
    };
    fix_geom(&mut m.doc.root);
    for d in &mut m.doc.defs {
        fix_geom(&mut d.geom);
    }
    for mt in &mut m.doc.materials {
        for t in [&mut mt.texture, &mut mt.normal_tex, &mut mt.rough_tex, &mut mt.metal_tex, &mut mt.ao_tex] {
            *t = t.map(|x| x + texs);
        }
    }
}

impl crate::app::HomeUpApp {
    /// File > Open of a .skp: the model becomes the current document.
    pub(crate) fn open_skp(&mut self, path: std::path::PathBuf) {
        let res = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| read(&b));
        match res {
            Ok(m) => {
                self.new_file();
                self.doc = m.doc;
                if let Some(c) = m.camera {
                    let f = (c.target - c.eye).normalize_or(glam::DVec3::Y);
                    self.cam.target = c.target;
                    self.cam.dist = c.eye.distance(c.target).max(0.1);
                    self.cam.pitch = (-f.z).clamp(-1.0, 1.0).asin();
                    self.cam.yaw = (-f.y).atan2(-f.x);
                    self.cam.fov_y = c.fov_deg.clamp(5.0, 120.0).to_radians();
                    self.cam.ortho = !c.perspective;
                    let roll = crate::matchphoto::roll_of(f, c.up.normalize_or(glam::DVec3::Z));
                    self.cam.roll = if roll.abs() > 1e-3 { roll } else { 0.0 };
                } else {
                    self.zoom_extents();
                }
                self.cache_dirty = true;
                self.dirty = true;
                self.show_home = false;
                self.add_recent(&path);
                self.message = format!("SketchUp {} aberto: {}. Salvar grava um .homeup.", m.version, m.stats);
                // keep the .skp untouched: saving asks for a .homeup name
                self.file = None;
                self.modified = true;
            }
            Err(e) => self.message = format!("Erro ao abrir o .skp: {e}"),
        }
    }

    /// File > Import of a .skp: the whole model becomes one component to place.
    pub(crate) fn import_skp(&mut self, path: &std::path::Path) -> Result<(), String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let mut m = read(&bytes)?;
        self.checkpoint_keep_sel();
        let (mats, texs, tags, defs) = (self.doc.materials.len(), self.doc.textures.len(), self.doc.tags.len(), self.doc.defs.len());
        shift(&mut m, mats, texs, tags, defs);
        self.doc.materials.extend(m.doc.materials);
        self.doc.textures.extend(m.doc.textures);
        self.doc.tags.extend(m.doc.tags);
        self.doc.defs.extend(m.doc.defs);
        let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("SketchUp".into());
        self.doc.defs.push(Definition { name: name.clone(), geom: m.doc.root, group: false, face_me: false, procedure: None, ifc: None, author: None, light: None });
        let d = self.doc.defs.len() - 1;
        self.cache_dirty = true;
        self.set_tool(crate::app::Tool::Place(d));
        self.message = format!("{name} (SketchUp {}): {} — clique para posicionar.", m.version, m.stats);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal synthetic file: header, ZIP with a model.dat holding one square face.
    fn rec(tag: u16, payload: &[u8]) -> Vec<u8> {
        let mut v = tag.to_le_bytes().to_vec();
        v.extend((payload.len() as u32).to_le_bytes());
        v.extend_from_slice(payload);
        v
    }
    fn hdr(id: u16, flags: u8) -> Vec<u8> {
        let mut h = rec(0x05dc, &rec(0x05de, &id.to_le_bytes()));
        h.extend(rec(0x07d3, &[flags]));
        rec(0x07d0, &h)
    }
    fn synthetic() -> Vec<u8> {
        let pts = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0]];
        let mut verts = Vec::new();
        for (k, p) in pts.iter().enumerate() {
            let mut b = Vec::new();
            for c in p {
                b.extend((*c as f64).to_le_bytes());
            }
            let mut v = rec(0x05dc, &rec(0x05de, &(100 + k as u16).to_le_bytes()));
            v.extend(rec(0x09c5, &b));
            verts.extend(rec(0x09c4, &v));
        }
        let mut edges = Vec::new();
        let mut uses = Vec::new();
        for k in 0..4u16 {
            let mut e = hdr(200 + k, 6);
            e.extend(rec(0x0bb9, &(100 + k).to_le_bytes()));
            e.extend(rec(0x0bba, &(100 + (k + 1) % 4).to_le_bytes()));
            edges.extend(rec(0x0bb8, &e));
            let mut u = rec(0x0fa1, &(200 + k).to_le_bytes());
            u.extend(rec(0x0fa2, &[0]));
            uses.extend(rec(0x0fa0, &u));
        }
        let mut plane = Vec::new();
        for c in [0.0f64, 0.0, 1.0, 0.0] {
            plane.extend(c.to_le_bytes());
        }
        let mut face = hdr(300, 6);
        face.extend(rec(0x0dad, &plane));
        face.extend(rec(0x0dae, &rec(0x1194, &rec(0x1195, &uses))));
        let mut ents = hdr(1, 6);
        ents.extend(rec(0x1389, &verts));
        ents.extend(rec(0x138a, &edges));
        ents.extend(rec(0x138b, &rec(0x0dac, &face)));
        let model = rec(0x01f4, &rec(0x01f6, &rec(0x1388, &ents)));
        let mut zipbuf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut zipbuf);
            z.start_file("model.dat", zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut z, &model).unwrap();
            z.finish().unwrap();
        }
        let mut out = vec![0xff, 0xfe, 0xff, 0x0e];
        for u in "SketchUp Model".encode_utf16() {
            out.extend(u.to_le_bytes());
        }
        out.extend([0xff, 0xfe, 0xff, 0x08]);
        for u in "{25.0.0}".encode_utf16() {
            out.extend(u.to_le_bytes());
        }
        out.extend(zipbuf.into_inner());
        out
    }

    #[test]
    fn reads_a_synthetic_square() {
        let m = read(&synthetic()).unwrap();
        assert_eq!(m.version, "25.0.0");
        assert_eq!(m.doc.root.faces.len(), 1);
        assert_eq!(m.doc.root.edges.len(), 4);
        let f = &m.doc.root.faces[0];
        assert_eq!(f.outer.len(), 4);
        assert!((m.doc.root.p(f.outer[2]) - DVec3::new(0.254, 0.254, 0.0)).length() < 1e-9);
        assert!(read(b"not a model").is_err());
    }
}
