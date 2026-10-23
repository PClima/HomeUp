//! 3D file import (OBJ, STL, glTF/GLB, Collada DAE, and SKP through the
//! official SketchUp SDK when present). Everything is read into a neutral
//! triangle soup (`Imported`) and then converted into a HomeUp component.

use crate::doc::Material;
use crate::geom::{Edge, Face, Geom, VId};
use glam::{DMat4, DVec3};
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct ImportMat {
    pub name: String,
    pub color: [u8; 3],
    pub alpha: f32,
    /// Encoded base color image (JPG/PNG), when the material is textured.
    pub texture: Option<Vec<u8>>,
    /// Physically based parameters and maps (glTF metallic-roughness, OBJ bump).
    pub pbr: Option<ImportPbr>,
}

#[derive(Clone, Debug, Default)]
pub struct ImportPbr {
    pub metal: f32,
    pub rough: f32,
    pub normal: Option<Vec<u8>>,
    pub normal_scale: f32,
    /// Packed map: G = roughness, B = metalness.
    pub metal_rough: Option<Vec<u8>>,
    pub occlusion: Option<Vec<u8>>,
    pub occlusion_strength: f32,
}

#[derive(Clone, Debug, Default)]
pub struct MeshPart {
    /// Positions in file units, already transformed by the file's node hierarchy.
    pub positions: Vec<DVec3>,
    pub tris: Vec<[u32; 3]>,
    pub material: Option<usize>,
    /// Texture coordinates per position (empty when absent).
    pub uvs: Vec<[f32; 2]>,
}

#[derive(Clone, Debug, Default)]
pub struct Imported {
    pub parts: Vec<MeshPart>,
    pub materials: Vec<ImportMat>,
    /// Hints read from the file itself.
    pub meters_per_unit: Option<f64>,
    pub z_up: Option<bool>,
    /// Shown to the user after the import (e.g. file newer than the SDK).
    pub warning: Option<String>,
}

impl Imported {
    pub fn triangle_count(&self) -> usize {
        self.parts.iter().map(|p| p.tris.len()).sum()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImportOpts {
    pub meters_per_unit: f64,
    pub z_up: bool,
    /// Edges between faces closer than this angle become soft (smooth).
    pub smooth_deg: f64,
}

pub const UNITS: [(&str, f64); 6] =
    [("Metros", 1.0), ("Centímetros", 0.01), ("Milímetros", 0.001), ("Polegadas", 0.0254), ("Pés", 0.3048), ("Quilômetros", 1000.0)];

pub fn supported(path: &Path) -> bool {
    matches!(ext(path).as_str(), "obj" | "stl" | "gltf" | "glb" | "dae" | "skp")
}

fn ext(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Defaults per format (overridden by hints found in the file).
pub fn default_opts(path: &Path, imp: &Imported) -> ImportOpts {
    let (m, z) = match ext(path).as_str() {
        "stl" => (0.001, true),
        "skp" => (0.0254, true),
        "dae" => (1.0, false),
        _ => (1.0, false),
    };
    ImportOpts { meters_per_unit: imp.meters_per_unit.unwrap_or(m), z_up: imp.z_up.unwrap_or(z), smooth_deg: 20.0 }
}

pub fn load(path: &Path) -> Result<Imported, String> {
    match ext(path).as_str() {
        "obj" => load_obj(path),
        "stl" => load_stl(path),
        "gltf" | "glb" => load_gltf(path),
        "dae" => load_dae(path),
        "skp" => crate::skp::load_skp(path),
        e => Err(format!("Formato não suportado: .{e}")),
    }
}

// ---------------------------------------------------------------- OBJ

fn load_obj(path: &Path) -> Result<Imported, String> {
    let opts = tobj::LoadOptions { single_index: true, triangulate: true, ignore_points: true, ignore_lines: true, ..Default::default() };
    let (models, mats) = tobj::load_obj(path, &opts).map_err(|e| format!("OBJ: {e}"))?;
    let mats = mats.unwrap_or_default();
    let mut imp = Imported::default();
    for m in &mats {
        let c = m.diffuse.unwrap_or([0.8, 0.8, 0.8]);
        let texture = m.diffuse_texture.as_ref().and_then(|t| std::fs::read(path.parent().unwrap_or(Path::new(".")).join(t.trim())).ok());
        let normal = m.normal_texture.as_ref().and_then(|t| std::fs::read(path.parent().unwrap_or(Path::new(".")).join(t.trim())).ok());
        let pbr = normal.map(|n| ImportPbr { rough: 0.85, normal: Some(n), normal_scale: 1.0, occlusion_strength: 1.0, ..Default::default() });
        imp.materials.push(ImportMat { name: m.name.clone(), color: to_u8(c), alpha: m.dissolve.unwrap_or(1.0).clamp(0.05, 1.0), texture, pbr });
    }
    for model in models {
        let mesh = &model.mesh;
        let positions: Vec<DVec3> =
            mesh.positions.chunks_exact(3).map(|p| DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64)).collect();
        let tris = mesh.indices.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();
        // OBJ texture space has v pointing up; images have row 0 at the top
        let uvs = if mesh.texcoords.len() == positions.len() * 2 {
            mesh.texcoords.chunks_exact(2).map(|t| [t[0], 1.0 - t[1]]).collect()
        } else {
            Vec::new()
        };
        imp.parts.push(MeshPart { positions, tris, material: mesh.material_id.filter(|i| *i < imp.materials.len()), uvs });
    }
    Ok(imp)
}

fn to_u8(c: [f32; 3]) -> [u8; 3] {
    [(c[0].clamp(0.0, 1.0) * 255.0) as u8, (c[1].clamp(0.0, 1.0) * 255.0) as u8, (c[2].clamp(0.0, 1.0) * 255.0) as u8]
}

// ---------------------------------------------------------------- STL

fn load_stl(path: &Path) -> Result<Imported, String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mesh = stl_io::read_stl(&mut f).map_err(|e| format!("STL: {e}"))?;
    let positions = mesh.vertices.iter().map(|v| DVec3::new(v.0[0] as f64, v.0[1] as f64, v.0[2] as f64)).collect();
    let tris = mesh.faces.iter().map(|t| [t.vertices[0] as u32, t.vertices[1] as u32, t.vertices[2] as u32]).collect();
    Ok(Imported { parts: vec![MeshPart { positions, tris, material: None, uvs: Vec::new() }], ..Default::default() })
}

// ---------------------------------------------------------------- glTF

fn load_gltf(path: &Path) -> Result<Imported, String> {
    let (doc, buffers, _images) = gltf::import(path).map_err(|e| format!("glTF: {e}"))?;
    let mut imp = Imported { meters_per_unit: Some(1.0), z_up: Some(false), ..Default::default() };
    // original encoded bytes of an image (external file or embedded buffer view)
    let image_bytes = |img: gltf::Image| -> Option<Vec<u8>> {
        match img.source() {
            gltf::image::Source::Uri { uri, .. } => {
                if uri.starts_with("data:") {
                    use base64::Engine;
                    let b64 = uri.split_once(",")?.1;
                    base64::engine::general_purpose::STANDARD.decode(b64).ok()
                } else {
                    let p = path.parent().unwrap_or(Path::new(".")).join(uri.replace("%20", " "));
                    std::fs::read(p).ok()
                }
            }
            gltf::image::Source::View { view, .. } => {
                let data = &buffers[view.buffer().index()];
                data.get(view.offset()..view.offset() + view.length()).map(|s| s.to_vec())
            }
        }
    };
    for m in doc.materials() {
        let c = m.pbr_metallic_roughness().base_color_factor();
        let pm = m.pbr_metallic_roughness();
        let texture = pm.base_color_texture().and_then(|t| image_bytes(t.texture().source()));
        let pbr = ImportPbr {
            metal: pm.metallic_factor(),
            rough: pm.roughness_factor(),
            normal: m.normal_texture().and_then(|t| image_bytes(t.texture().source())),
            normal_scale: m.normal_texture().map(|t| t.scale()).unwrap_or(1.0),
            metal_rough: pm.metallic_roughness_texture().and_then(|t| image_bytes(t.texture().source())),
            occlusion: m.occlusion_texture().and_then(|t| image_bytes(t.texture().source())),
            occlusion_strength: m.occlusion_texture().map(|t| t.strength()).unwrap_or(1.0),
        };
        imp.materials.push(ImportMat {
            texture,
            pbr: Some(pbr),
            name: m.name().map(str::to_string).unwrap_or_else(|| format!("Material {}", m.index().unwrap_or(0) + 1)),
            color: to_u8([c[0].powf(1.0 / 2.2), c[1].powf(1.0 / 2.2), c[2].powf(1.0 / 2.2)]),
            alpha: c[3].clamp(0.05, 1.0),
        });
    }
    let scene = doc.default_scene().or_else(|| doc.scenes().next()).ok_or("glTF sem cena")?;
    fn walk(node: gltf::Node, parent: DMat4, buffers: &[gltf::buffer::Data], imp: &mut Imported) {
        let m = node.transform().matrix();
        let local = DMat4::from_cols_array_2d(&m.map(|c| c.map(|x| x as f64)));
        let xf = parent * local;
        if let Some(mesh) = node.mesh() {
            for prim in mesh.primitives() {
                if prim.mode() != gltf::mesh::Mode::Triangles {
                    continue;
                }
                let reader = prim.reader(|b| Some(&buffers[b.index()]));
                let Some(pos) = reader.read_positions() else { continue };
                let positions: Vec<DVec3> =
                    pos.map(|p| xf.transform_point3(DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64))).collect();
                let idx: Vec<u32> = match reader.read_indices() {
                    Some(i) => i.into_u32().collect(),
                    None => (0..positions.len() as u32).collect(),
                };
                let mut tris: Vec<[u32; 3]> = idx.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();
                if xf.determinant() < 0.0 {
                    for t in &mut tris {
                        t.swap(1, 2);
                    }
                }
                let uvs: Vec<[f32; 2]> = reader.read_tex_coords(0).map(|t| t.into_f32().collect()).unwrap_or_default();
                let uvs = if uvs.len() == positions.len() { uvs } else { Vec::new() };
                imp.parts.push(MeshPart { positions, tris, material: prim.material().index(), uvs });
            }
        }
        for c in node.children() {
            walk(c, xf, buffers, imp);
        }
    }
    for n in scene.nodes() {
        walk(n, DMat4::IDENTITY, &buffers, &mut imp);
    }
    Ok(imp)
}

// ---------------------------------------------------------------- Collada (DAE)

fn floats(s: &str) -> Vec<f64> {
    s.split_ascii_whitespace().filter_map(|x| x.parse().ok()).collect()
}

fn ints(s: &str) -> Vec<usize> {
    s.split_ascii_whitespace().filter_map(|x| x.parse().ok()).collect()
}

fn child<'a, 'i>(n: roxmltree::Node<'a, 'i>, tag: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.children().find(|c| c.has_tag_name(tag))
}

fn children<'a, 'i>(n: roxmltree::Node<'a, 'i>, tag: &'static str) -> impl Iterator<Item = roxmltree::Node<'a, 'i>> {
    n.children().filter(move |c| c.has_tag_name(tag))
}

fn url(n: roxmltree::Node, attr: &str) -> String {
    n.attribute(attr).unwrap_or("").trim_start_matches('#').to_string()
}

fn node_matrix(node: roxmltree::Node) -> DMat4 {
    let mut m = DMat4::IDENTITY;
    for c in node.children().filter(|c| c.is_element()) {
        let v = floats(c.text().unwrap_or(""));
        let t = match c.tag_name().name() {
            "matrix" if v.len() == 16 => DMat4::from_cols_array(&[
                v[0], v[4], v[8], v[12], v[1], v[5], v[9], v[13], v[2], v[6], v[10], v[14], v[3], v[7], v[11], v[15],
            ]),
            "translate" if v.len() == 3 => DMat4::from_translation(DVec3::new(v[0], v[1], v[2])),
            "rotate" if v.len() == 4 => {
                let axis = DVec3::new(v[0], v[1], v[2]);
                if axis.length() < 1e-12 { DMat4::IDENTITY } else { DMat4::from_axis_angle(axis.normalize(), v[3].to_radians()) }
            }
            "scale" if v.len() == 3 => DMat4::from_scale(DVec3::new(v[0], v[1], v[2])),
            _ => continue,
        };
        m *= t;
    }
    m
}

struct DaeMesh {
    /// (positions, triangles, material symbol)
    prims: Vec<(Vec<DVec3>, Vec<[u32; 3]>, String)>,
}

fn dae_geometry(geo: roxmltree::Node) -> Option<DaeMesh> {
    let mesh = child(geo, "mesh")?;
    let mut sources: HashMap<String, (Vec<f64>, usize)> = HashMap::new();
    for s in children(mesh, "source") {
        let arr = child(s, "float_array").map(|a| floats(a.text().unwrap_or(""))).unwrap_or_default();
        let stride = child(s, "technique_common")
            .and_then(|t| child(t, "accessor"))
            .and_then(|a| a.attribute("stride"))
            .and_then(|x| x.parse().ok())
            .unwrap_or(3);
        sources.insert(s.attribute("id").unwrap_or("").to_string(), (arr, stride));
    }
    // <vertices id> -> POSITION source
    let mut vert_alias: HashMap<String, String> = HashMap::new();
    for v in children(mesh, "vertices") {
        if let Some(inp) = children(v, "input").find(|i| i.attribute("semantic") == Some("POSITION")) {
            vert_alias.insert(v.attribute("id").unwrap_or("").to_string(), url(inp, "source"));
        }
    }
    let mut out = DaeMesh { prims: Vec::new() };
    for prim in mesh.children().filter(|c| c.is_element()) {
        let kind = prim.tag_name().name();
        if !matches!(kind, "triangles" | "polylist" | "polygons") {
            continue;
        }
        let inputs: Vec<roxmltree::Node> = children(prim, "input").collect();
        let stride = inputs.iter().filter_map(|i| i.attribute("offset")?.parse::<usize>().ok()).max().unwrap_or(0) + 1;
        let Some(vin) = inputs.iter().find(|i| i.attribute("semantic") == Some("VERTEX")) else { continue };
        let voff: usize = vin.attribute("offset").and_then(|x| x.parse().ok()).unwrap_or(0);
        let src_id = url(*vin, "source");
        let src_id = vert_alias.get(&src_id).cloned().unwrap_or(src_id);
        let Some((arr, pstride)) = sources.get(&src_id) else { continue };
        let pos: Vec<DVec3> = arr.chunks_exact(*pstride).map(|c| DVec3::new(c[0], c[1], c.get(2).copied().unwrap_or(0.0))).collect();
        // polygons as index lists (position indices)
        let mut polys: Vec<Vec<u32>> = Vec::new();
        let pick = |p: &[usize], k: usize| p.get(k * stride + voff).copied().unwrap_or(0) as u32;
        match kind {
            "triangles" => {
                let p = ints(child(prim, "p").and_then(|p| p.text()).unwrap_or(""));
                for t in 0..p.len() / (3 * stride) {
                    polys.push((0..3).map(|k| pick(&p, t * 3 + k)).collect());
                }
            }
            "polylist" => {
                let p = ints(child(prim, "p").and_then(|p| p.text()).unwrap_or(""));
                let vc = ints(child(prim, "vcount").and_then(|p| p.text()).unwrap_or(""));
                let mut k = 0;
                for n in vc {
                    polys.push((0..n).map(|j| pick(&p, k + j)).collect());
                    k += n;
                }
            }
            _ => {
                for pe in children(prim, "p") {
                    let p = ints(pe.text().unwrap_or(""));
                    polys.push((0..p.len() / stride).map(|j| pick(&p, j)).collect());
                }
            }
        }
        let mut tris = Vec::new();
        for poly in polys {
            for j in 1..poly.len().saturating_sub(1) {
                tris.push([poly[0], poly[j], poly[j + 1]]);
            }
        }
        tris.retain(|t| t.iter().all(|i| (*i as usize) < pos.len()));
        out.prims.push((pos, tris, prim.attribute("material").unwrap_or("").to_string()));
    }
    Some(out)
}

fn load_dae(path: &Path) -> Result<Imported, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let xml = roxmltree::Document::parse_with_options(&text, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() })
        .map_err(|e| format!("DAE: {e}"))?;
    let root = xml.root_element();
    let mut imp = Imported::default();
    if let Some(asset) = child(root, "asset") {
        if let Some(u) = child(asset, "unit").and_then(|u| u.attribute("meter")).and_then(|m| m.parse().ok()) {
            imp.meters_per_unit = Some(u);
        }
        if let Some(up) = child(asset, "up_axis").and_then(|u| u.text()) {
            imp.z_up = Some(up.trim() == "Z_UP");
        }
    }
    let by_id = |lib: &str, tag: &'static str| -> HashMap<String, roxmltree::Node> {
        child(root, lib)
            .map(|l| children(l, tag).filter_map(|n| Some((n.attribute("id")?.to_string(), n))).collect())
            .unwrap_or_default()
    };
    let effects = by_id("library_effects", "effect");
    let materials = by_id("library_materials", "material");
    let geometries = by_id("library_geometries", "geometry");
    let lib_nodes: HashMap<String, roxmltree::Node> = child(root, "library_nodes")
        .map(|l| l.descendants().filter(|n| n.has_tag_name("node")).filter_map(|n| Some((n.attribute("id")?.to_string(), n))).collect())
        .unwrap_or_default();

    // material id -> index in imp.materials
    let mut mat_index: HashMap<String, usize> = HashMap::new();
    for (id, m) in &materials {
        let eff = child(*m, "instance_effect").map(|e| url(e, "url")).unwrap_or_default();
        let mut color = [200u8, 200, 200];
        let mut alpha = 1.0f32;
        if let Some(e) = effects.get(&eff) {
            if let Some(tech) = e.descendants().find(|n| matches!(n.tag_name().name(), "lambert" | "phong" | "blinn" | "constant")) {
                if let Some(c) = child(tech, "diffuse").and_then(|d| child(d, "color")).and_then(|c| c.text()) {
                    let v = floats(c);
                    if v.len() >= 3 {
                        color = to_u8([v[0] as f32, v[1] as f32, v[2] as f32]);
                        alpha = v.get(3).copied().unwrap_or(1.0) as f32;
                    }
                }
                if let Some(t) = child(tech, "transparency").and_then(|d| child(d, "float")).and_then(|c| c.text()) {
                    if let Ok(t) = t.trim().parse::<f32>() {
                        if t < 1.0 && t > 0.0 {
                            alpha = t;
                        }
                    }
                }
            }
        }
        let name = m.attribute("name").unwrap_or(id).to_string();
        mat_index.insert(id.clone(), imp.materials.len());
        imp.materials.push(ImportMat { name, color, alpha: alpha.clamp(0.05, 1.0), ..Default::default() });
    }

    let mut geo_cache: HashMap<String, DaeMesh> = HashMap::new();
    fn walk(
        node: roxmltree::Node,
        parent: DMat4,
        depth: usize,
        lib_nodes: &HashMap<String, roxmltree::Node>,
        geometries: &HashMap<String, roxmltree::Node>,
        geo_cache: &mut HashMap<String, DaeMesh>,
        mat_index: &HashMap<String, usize>,
        imp: &mut Imported,
    ) {
        if depth > 64 {
            return;
        }
        let xf = parent * node_matrix(node);
        for ig in children(node, "instance_geometry") {
            let gid = url(ig, "url");
            if !geo_cache.contains_key(&gid) {
                if let Some(g) = geometries.get(&gid).and_then(|g| dae_geometry(*g)) {
                    geo_cache.insert(gid.clone(), g);
                }
            }
            let Some(g) = geo_cache.get(&gid) else { continue };
            let mut bind: HashMap<String, String> = HashMap::new();
            for im in ig.descendants().filter(|n| n.has_tag_name("instance_material")) {
                bind.insert(im.attribute("symbol").unwrap_or("").to_string(), url(im, "target"));
            }
            let flip = xf.determinant() < 0.0;
            for (pos, tris, sym) in &g.prims {
                let mat = bind.get(sym).or(Some(sym)).and_then(|t| mat_index.get(t)).copied();
                let mut tris = tris.clone();
                if flip {
                    for t in &mut tris {
                        t.swap(1, 2);
                    }
                }
                imp.parts.push(MeshPart { positions: pos.iter().map(|p| xf.transform_point3(*p)).collect(), tris, material: mat, uvs: Vec::new() });
            }
        }
        for inode in children(node, "instance_node") {
            if let Some(n) = lib_nodes.get(&url(inode, "url")) {
                walk(*n, xf, depth + 1, lib_nodes, geometries, geo_cache, mat_index, imp);
            }
        }
        for c in children(node, "node") {
            walk(c, xf, depth + 1, lib_nodes, geometries, geo_cache, mat_index, imp);
        }
    }
    let scene_url = child(root, "scene").and_then(|s| child(s, "instance_visual_scene")).map(|i| url(i, "url"));
    let scenes = by_id("library_visual_scenes", "visual_scene");
    let scene = scene_url.and_then(|u| scenes.get(&u).copied()).or_else(|| scenes.values().next().copied()).ok_or("DAE sem cena")?;
    for n in children(scene, "node") {
        walk(n, DMat4::IDENTITY, 0, &lib_nodes, &geometries, &mut geo_cache, &mat_index, &mut imp);
    }
    Ok(imp)
}

// ---------------------------------------------------------------- conversion

/// Convert imported triangles into a HomeUp geometry (meters, Z up).
/// New materials are appended to `doc_mats` (reused by name when they exist).
pub fn to_geom(imp: &Imported, o: &ImportOpts, doc_mats: &mut Vec<Material>, doc_tex: &mut Vec<crate::tex::TextureData>) -> Geom {
    let conv = |p: DVec3| {
        let p = if o.z_up { p } else { DVec3::new(p.x, -p.z, p.y) };
        p * o.meters_per_unit
    };
    let mat_map: Vec<usize> = imp
        .materials
        .iter()
        .map(|m| match doc_mats.iter().position(|d| d.name == m.name) {
            Some(i) => i,
            None => {
                let mut mat = Material::color(&m.name, m.color, m.alpha);
                if let Some(t) = m.texture.as_ref().and_then(|b| crate::tex::TextureData::from_bytes(&m.name, b.clone()).ok()) {
                    mat.color = t.average();
                    doc_tex.push(t);
                    mat.texture = Some(doc_tex.len() - 1);
                }
                if let Some(p) = &m.pbr {
                    let mut add = |b: &Option<Vec<u8>>, what: &str| -> Option<usize> {
                        let t = crate::tex::TextureData::from_bytes(&format!("{} ({what})", m.name), b.clone()?).ok()?;
                        doc_tex.push(t);
                        Some(doc_tex.len() - 1)
                    };
                    mat.metalness = p.metal.clamp(0.0, 1.0);
                    mat.roughness = p.rough.clamp(0.02, 1.0);
                    mat.normal_tex = add(&p.normal, "normal");
                    mat.normal_scale = p.normal_scale;
                    if let Some(mr) = add(&p.metal_rough, "metal/rugosidade") {
                        (mat.rough_tex, mat.metal_tex, mat.rough_ch, mat.metal_ch) = (Some(mr), Some(mr), 1, 2);
                    }
                    mat.ao_tex = add(&p.occlusion, "AO");
                    mat.ao_strength = p.occlusion_strength;
                }
                doc_mats.push(mat);
                doc_mats.len() - 1
            }
        })
        .collect();

    let mut g = Geom::default();
    let eps = 1e-6;
    let mut grid: HashMap<(i64, i64, i64), VId> = HashMap::new();
    let mut vid = |p: DVec3, g: &mut Geom| -> VId {
        let k = ((p.x / eps).round() as i64, (p.y / eps).round() as i64, (p.z / eps).round() as i64);
        *grid.entry(k).or_insert_with(|| {
            g.verts.push(p);
            (g.verts.len() - 1) as VId
        })
    };
    for part in &imp.parts {
        let ids: Vec<VId> = part.positions.iter().map(|p| vid(conv(*p), &mut g)).collect();
        let mat = part.material.and_then(|m| mat_map.get(m).copied());
        for t in &part.tris {
            let tri = [ids[t[0] as usize], ids[t[1] as usize], ids[t[2] as usize]];
            if tri[0] == tri[1] || tri[1] == tri[2] || tri[0] == tri[2] {
                continue;
            }
            let (a, b, c) = (g.p(tri[0]), g.p(tri[1]), g.p(tri[2]));
            let n = (b - a).cross(c - a);
            if n.length() < 1e-14 {
                continue;
            }
            let uvs = (!part.uvs.is_empty()).then(|| t.iter().map(|i| part.uvs[*i as usize]).collect());
            g.faces.push(Face { outer: tri.to_vec(), holes: vec![], normal: n.normalize(), material: mat, uvs, ..Default::default() });
        }
    }
    // edges, classified by the angle between their faces
    let mut edge_faces: HashMap<(VId, VId), Vec<usize>> = HashMap::new();
    for (fi, f) in g.faces.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (f.outer[k], f.outer[(k + 1) % 3]);
            edge_faces.entry((a.min(b), a.max(b))).or_default().push(fi);
        }
    }
    let cos_smooth = o.smooth_deg.to_radians().cos();
    let mut keys: Vec<(VId, VId)> = edge_faces.keys().copied().collect();
    keys.sort_unstable();
    for k in keys {
        let fs = &edge_faces[&k];
        let mut e = Edge::new(k.0, k.1);
        if fs.len() == 2 {
            let (f1, f2) = (&g.faces[fs[0]], &g.faces[fs[1]]);
            let d = f1.normal.dot(f2.normal);
            if d > 0.99999 && f1.material == f2.material {
                // coplanar triangles: looks like one face
                e.soft = true;
                e.hidden = true;
            } else if d > cos_smooth {
                e.soft = true;
            }
        }
        g.edges.push(e);
    }
    g
}

/// Move geometry so its bounding-box minimum sits at the origin. Returns that offset.
pub fn recenter(g: &mut Geom) -> DVec3 {
    let mut lo = DVec3::splat(f64::MAX);
    for f in &g.faces {
        for v in &f.outer {
            lo = lo.min(g.p(*v));
        }
    }
    if lo.x == f64::MAX {
        return DVec3::ZERO;
    }
    for v in &mut g.verts {
        *v -= lo;
    }
    lo
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str, body: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("homeup-import-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    const CUBE_OBJ: &str = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\n\
f 1 4 3 2\nf 5 6 7 8\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n";

    #[test]
    fn obj_cube_becomes_component_geometry() {
        let p = tmp("cube.obj", CUBE_OBJ);
        let imp = load(&p).unwrap();
        assert_eq!(imp.triangle_count(), 12);
        let mut mats = Vec::new();
        let o = ImportOpts { meters_per_unit: 2.0, z_up: true, smooth_deg: 20.0 };
        let g = to_geom(&imp, &o, &mut mats, &mut Vec::new());
        assert_eq!(g.faces.len(), 12);
        assert_eq!(g.verts.len(), 8);
        // 12 cube edges visible + 6 hidden diagonals
        assert_eq!(g.edges.iter().filter(|e| !e.hidden).count(), 12);
        assert_eq!(g.edges.iter().filter(|e| e.hidden).count(), 6);
        let hi = g.verts.iter().fold(DVec3::ZERO, |a, b| a.max(*b));
        assert!((hi - DVec3::splat(2.0)).length() < 1e-9);
    }

    #[test]
    fn y_up_files_are_rotated_to_z_up() {
        let p = tmp("tri.obj", "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n");
        let imp = load(&p).unwrap();
        let g = to_geom(&imp, &ImportOpts { meters_per_unit: 1.0, z_up: false, smooth_deg: 20.0 }, &mut Vec::new(), &mut Vec::new());
        // file Y (up) becomes HomeUp Z
        assert!(g.verts.iter().any(|v| (v.z - 1.0).abs() < 1e-9));
    }

    #[test]
    fn stl_ascii() {
        let body = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1000 0 0\nvertex 0 1000 0\nendloop\nendfacet\nendsolid t\n";
        let p = tmp("t.stl", body);
        let imp = load(&p).unwrap();
        assert_eq!(imp.triangle_count(), 1);
        let o = default_opts(&p, &imp);
        assert_eq!(o.meters_per_unit, 0.001);
        let g = to_geom(&imp, &o, &mut Vec::new(), &mut Vec::new());
        assert!(g.verts.iter().any(|v| (v.x - 1.0).abs() < 1e-9));
    }

    #[test]
    fn dae_with_units_materials_and_nodes() {
        let body = r##"<?xml version="1.0"?>
<COLLADA xmlns="http://www.collada.org/2005/11/COLLADASchema" version="1.4.1">
 <asset><unit meter="0.0254" name="inch"/><up_axis>Z_UP</up_axis></asset>
 <library_effects><effect id="fx"><profile_COMMON><technique sid="c"><lambert>
  <diffuse><color>1 0 0 1</color></diffuse></lambert></technique></profile_COMMON></effect></library_effects>
 <library_materials><material id="red" name="Vermelho"><instance_effect url="#fx"/></material></library_materials>
 <library_geometries><geometry id="g"><mesh>
  <source id="pos"><float_array id="pa" count="12">0 0 0 10 0 0 10 10 0 0 10 0</float_array>
   <technique_common><accessor source="#pa" count="4" stride="3"/></technique_common></source>
  <vertices id="v"><input semantic="POSITION" source="#pos"/></vertices>
  <polylist material="m" count="1"><input semantic="VERTEX" source="#v" offset="0"/><vcount>4</vcount><p>0 1 2 3</p></polylist>
 </mesh></geometry></library_geometries>
 <library_visual_scenes><visual_scene id="s">
  <node><translate>100 0 0</translate>
   <instance_geometry url="#g"><bind_material><technique_common>
    <instance_material symbol="m" target="#red"/></technique_common></bind_material></instance_geometry>
  </node></visual_scene></library_visual_scenes>
 <scene><instance_visual_scene url="#s"/></scene>
</COLLADA>"##;
        let p = tmp("q.dae", body);
        let imp = load(&p).unwrap();
        assert_eq!(imp.triangle_count(), 2);
        assert_eq!(imp.meters_per_unit, Some(0.0254));
        assert_eq!(imp.z_up, Some(true));
        assert_eq!(imp.materials[0].name, "Vermelho");
        assert_eq!(imp.parts[0].material, Some(0));
        assert!((imp.parts[0].positions[0].x - 100.0).abs() < 1e-9);
        let mut mats = Vec::new();
        let g = to_geom(&imp, &default_opts(&p, &imp), &mut mats, &mut Vec::new());
        assert_eq!(mats[0].color, [255, 0, 0]);
        assert_eq!(g.faces[0].material, Some(0));
    }
}
