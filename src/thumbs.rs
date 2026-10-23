//! Small CPU renderings for the UI: component thumbnails, scene thumbnails and
//! the material preview sphere. Kept independent of the GPU so panels can make
//! them at any time (and tests can check them).

use crate::doc::{Document, Material};
use crate::geom::Geom;
use glam::{DMat4, DVec3, DVec4};
use image::{Rgba, RgbaImage};

pub struct Tri {
    pub p: [DVec3; 3],
    pub c: [u8; 3],
}

/// Triangles and visible edges of a geometry tree (instances included).
pub fn collect(doc: &Document, g: &Geom, xf: DMat4, mat: Option<usize>, tris: &mut Vec<Tri>, lines: &mut Vec<[DVec3; 2]>, depth: usize) {
    if depth > 24 {
        return;
    }
    let color = |m: Option<usize>| m.and_then(|m| doc.materials.get(m)).map(|m| m.color).unwrap_or([236, 236, 230]);
    for f in &g.faces {
        if !doc.face_visible(f) {
            continue;
        }
        let c = color(f.material.or(mat));
        for t in g.triangulate_face(f) {
            tris.push(Tri { p: [xf.transform_point3(g.p(t[0])), xf.transform_point3(g.p(t[1])), xf.transform_point3(g.p(t[2]))], c });
        }
    }
    for e in &g.edges {
        if e.soft || e.hidden {
            continue;
        }
        lines.push([xf.transform_point3(g.p(e.v[0])), xf.transform_point3(g.p(e.v[1]))]);
    }
    for inst in &g.instances {
        if !doc.inst_visible(inst) {
            continue;
        }
        if let Some(d) = doc.defs.get(inst.def) {
            collect(doc, &d.geom, xf * doc.eff_xf(inst), inst.material.or(mat), tris, lines, depth + 1);
        }
    }
}

/// Orthographic 3/4 view fitting the given triangles.
pub fn iso_view(tris: &[Tri]) -> DMat4 {
    let mut lo = DVec3::splat(f64::MAX);
    let mut hi = DVec3::splat(f64::MIN);
    for t in tris {
        for p in t.p {
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    if lo.x > hi.x {
        return DMat4::IDENTITY;
    }
    let c = (lo + hi) * 0.5;
    let r = ((hi - lo).length() * 0.5).max(1e-3);
    let dir = DVec3::new(-1.0, -1.4, 0.9).normalize();
    let eye = c - dir * r * 4.0;
    let view = crate::camera::look_at_rh(eye, c, DVec3::Z);
    crate::camera::orthographic_rh(r * 1.05, r * 1.05, 0.01, r * 10.0) * view
}

/// Z-buffered flat-shaded rasterization with dark edges.
pub fn rasterize(tris: &[Tri], lines: &[[DVec3; 2]], vp: DMat4, w: u32, h: u32, bg: Option<[u8; 3]>) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(w, h, match bg {
        Some(c) => Rgba([c[0], c[1], c[2], 255]),
        None => Rgba([0, 0, 0, 0]),
    });
    let mut zb = vec![f64::INFINITY; (w * h) as usize];
    let to_screen = |p: DVec3| -> Option<DVec3> {
        let q = vp * DVec4::new(p.x, p.y, p.z, 1.0);
        if q.w <= 1e-6 {
            return None;
        }
        let n = q.truncate() / q.w;
        Some(DVec3::new((n.x * 0.5 + 0.5) * w as f64, (0.5 - n.y * 0.5) * h as f64, n.z))
    };
    let light = DVec3::new(0.35, -0.55, 0.75).normalize();
    for t in tris {
        let (Some(a), Some(b), Some(c)) = (to_screen(t.p[0]), to_screen(t.p[1]), to_screen(t.p[2])) else { continue };
        let n = (t.p[1] - t.p[0]).cross(t.p[2] - t.p[0]).normalize_or_zero();
        let k = 0.62 + 0.38 * n.dot(light).abs();
        let col = Rgba([(t.c[0] as f64 * k) as u8, (t.c[1] as f64 * k) as u8, (t.c[2] as f64 * k) as u8, 255]);
        let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        if area.abs() < 1e-9 {
            continue;
        }
        let x0 = a.x.min(b.x).min(c.x).floor().max(0.0) as i64;
        let x1 = a.x.max(b.x).max(c.x).ceil().min(w as f64 - 1.0) as i64;
        let y0 = a.y.min(b.y).min(c.y).floor().max(0.0) as i64;
        let y1 = a.y.max(b.y).max(c.y).ceil().min(h as f64 - 1.0) as i64;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let w0 = ((b.x - px) * (c.y - py) - (b.y - py) * (c.x - px)) / area;
                let w1 = ((c.x - px) * (a.y - py) - (c.y - py) * (a.x - px)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * a.z + w1 * b.z + w2 * c.z;
                let i = (y as u32 * w + x as u32) as usize;
                if z < zb[i] {
                    zb[i] = z;
                    img.put_pixel(x as u32, y as u32, col);
                }
            }
        }
    }
    let edge = Rgba([30, 30, 30, 255]);
    for l in lines {
        let (Some(a), Some(b)) = (to_screen(l[0]), to_screen(l[1])) else { continue };
        let steps = ((b.x - a.x).abs().max((b.y - a.y).abs()).ceil() as usize).clamp(1, 4 * (w + h) as usize);
        for s in 0..=steps {
            let t = s as f64 / steps as f64;
            let p = a.lerp(b, t);
            if p.x < 0.0 || p.y < 0.0 || p.x >= w as f64 || p.y >= h as f64 {
                continue;
            }
            let i = (p.y as u32 * w + p.x as u32) as usize;
            if p.z <= zb[i] + 0.004 {
                img.put_pixel(p.x as u32, p.y as u32, edge);
            }
        }
    }
    img
}

/// Thumbnail of a component/group definition (transparent background).
pub fn definition(doc: &Document, def: usize, size: u32) -> Option<RgbaImage> {
    let d = doc.defs.get(def)?;
    let (mut tris, mut lines) = (Vec::new(), Vec::new());
    collect(doc, &d.geom, DMat4::IDENTITY, None, &mut tris, &mut lines, 0);
    if tris.is_empty() && lines.is_empty() {
        return None;
    }
    let vp = iso_view(&tris);
    Some(rasterize(&tris, &lines, vp, size, size, None))
}

/// Thumbnail of the whole model seen through a camera.
pub fn model_view(doc: &Document, vp: DMat4, w: u32, h: u32) -> RgbaImage {
    let (mut tris, mut lines) = (Vec::new(), Vec::new());
    collect(doc, &doc.root, DMat4::IDENTITY, None, &mut tris, &mut lines, 0);
    rasterize(&tris, &lines, vp, w, h, Some(doc.style.bg))
}

/// Material preview: a lit sphere showing color, texture, roughness and metalness.
pub fn material_sphere(m: &Material, tex: Option<&RgbaImage>, size: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(size, size, Rgba([0, 0, 0, 0]));
    let l = DVec3::new(-0.45, 0.6, 0.65).normalize();
    let v = DVec3::Z;
    let rough = m.roughness.clamp(0.04, 1.0) as f64;
    let metal = m.metalness.clamp(0.0, 1.0) as f64;
    let r = size as f64 * 0.47;
    let c = size as f64 * 0.5;
    for y in 0..size {
        for x in 0..size {
            let (dx, dy) = ((x as f64 + 0.5 - c) / r, (c - y as f64 - 0.5) / r);
            let d2 = dx * dx + dy * dy;
            if d2 > 1.0 {
                continue;
            }
            let n = DVec3::new(dx, dy, (1.0 - d2).sqrt());
            // texture: two tiles around the sphere
            let base = match tex {
                Some(t) => {
                    let u = (n.x.atan2(n.z) / std::f64::consts::PI * 0.5 + 0.5) * 2.0;
                    let vv = n.y.acos() / std::f64::consts::PI * 2.0;
                    let px = ((u.fract() * t.width() as f64) as u32).min(t.width() - 1);
                    let py = ((vv.fract() * t.height() as f64) as u32).min(t.height() - 1);
                    let p = t.get_pixel(px, py);
                    DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64) / 255.0
                }
                None => DVec3::new(m.color[0] as f64, m.color[1] as f64, m.color[2] as f64) / 255.0,
            };
            let ndl = n.dot(l).max(0.0);
            let h = (l + v).normalize();
            let a2 = (rough * rough).powi(2);
            let ndh = n.dot(h).max(0.0);
            let dd = ndh * ndh * (a2 - 1.0) + 1.0;
            let spec_d = a2 / (std::f64::consts::PI * dd * dd);
            let f0 = DVec3::splat(0.04).lerp(base, metal);
            let fres = f0 + (DVec3::ONE - f0) * (1.0 - n.dot(v).max(0.0)).powi(5);
            let spec = fres * (spec_d * ndl * 0.25).min(4.0);
            // environment: light sky above, darker floor below
            let refl = 2.0 * n.dot(v) * n - v;
            let env = DVec3::new(0.86, 0.9, 0.95).lerp(DVec3::new(0.42, 0.4, 0.38), (0.5 - refl.y * 0.5).clamp(0.0, 1.0));
            let diffuse = base * (1.0 - metal) * (0.32 + 0.78 * ndl);
            let col = diffuse + spec + env * fres * (1.0 - 0.75 * rough);
            let a = (m.alpha.clamp(0.0, 1.0) * 255.0) as u8;
            let to8 = |v: f64| (v.clamp(0.0, 1.0) * 255.0) as u8;
            img.put_pixel(x, y, Rgba([to8(col.x), to8(col.y), to8(col.z), a.max(60)]));
        }
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rasterizes_a_box_and_a_sphere() {
        let mut doc = Document::default();
        let mut g = Geom::default();
        let pts = [DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0), DVec3::Y];
        g.add_polygon(&pts, None, false, DVec3::Z * 10.0);
        doc.defs.push(crate::doc::Definition { name: "q".into(), geom: g, group: false, face_me: false, procedure: None, ifc: None, author: None, light: None });
        let img = definition(&doc, 0, 48).unwrap();
        let filled = img.pixels().filter(|p| p[3] > 0).count();
        assert!(filled > 200, "{filled}");
        let m = Material::color("m", [200, 50, 50], 1.0);
        let s = material_sphere(&m, None, 32);
        assert!(s.get_pixel(16, 16)[0] > s.get_pixel(16, 16)[2]);
        assert_eq!(s.get_pixel(0, 0)[3], 0);
    }
}
