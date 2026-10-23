//! Photoreal renderer: a progressive CPU path tracer (all cores).
//!
//! - Geometry, materials and UVs come from the same scene builder as the
//!   viewport (forced to the photoreal face style), so what you see is what renders.
//! - Lighting: lamps (point / spot / panel, next-event estimation with soft
//!   shadows), sun (when shadows are on), sky or HDRI environment, and indirect
//!   light (multiple bounces, GGX specular + Lambert diffuse, thin glass).
//! - Output: HDR accumulation, automatic exposure + EV offset, ACES tone
//!   mapping, optional edge-avoiding à-trous denoiser guided by albedo/normal/depth.

use crate::app::HomeUpApp;
use crate::lights::{LightKind, WorldLight};
use eframe::egui::{self, RichText};
use glam::{Vec2, Vec3};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

// ------------------------------------------------------------------ scene

#[derive(Clone, Copy)]
pub(crate) struct Tri {
    pub(crate) p0: Vec3,
    pub(crate) e1: Vec3,
    pub(crate) e2: Vec3,
}

#[derive(Clone, Copy)]
pub(crate) struct Attr {
    pub(crate) n: [Vec3; 3],
    pub(crate) uv: [Vec2; 3],
    pub(crate) q: [f32; 3],
    pub(crate) tan: Vec3,
    pub(crate) tsign: f32,
    pub(crate) col: [f32; 4],
    pub(crate) back: [f32; 3],
    pub(crate) layer: f32,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Node {
    pub(crate) min: Vec3,
    pub(crate) max: Vec3,
    /// leaf: first triangle index; inner: left child (right = left + 1)
    pub(crate) first: u32,
    pub(crate) count: u32,
}

pub(crate) struct Bvh {
    pub(crate) nodes: Vec<Node>,
    pub(crate) idx: Vec<u32>,
}

fn build_bvh(tris: &[Tri]) -> Bvh {
    let n = tris.len();
    let mut idx: Vec<u32> = (0..n as u32).collect();
    let bounds: Vec<(Vec3, Vec3, Vec3)> = tris
        .iter()
        .map(|t| {
            let (a, b, c) = (t.p0, t.p0 + t.e1, t.p0 + t.e2);
            let lo = a.min(b).min(c);
            let hi = a.max(b).max(c);
            (lo, hi, (lo + hi) * 0.5)
        })
        .collect();
    let mut nodes = vec![Node::default()];
    // iterative build with an explicit stack: (node, start, end, depth)
    let mut stack = vec![(0usize, 0usize, n, 0usize)];
    while let Some((ni, s, e, depth)) = stack.pop() {
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        let (mut clo, mut chi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for &i in &idx[s..e] {
            let b = &bounds[i as usize];
            lo = lo.min(b.0);
            hi = hi.max(b.1);
            clo = clo.min(b.2);
            chi = chi.max(b.2);
        }
        nodes[ni].min = lo;
        nodes[ni].max = hi;
        let count = e - s;
        let ext = chi - clo;
        let axis = if ext.x >= ext.y && ext.x >= ext.z { 0 } else if ext.y >= ext.z { 1 } else { 2 };
        if count <= 4 || depth > 60 || ext[axis] < 1e-9 {
            nodes[ni].first = s as u32;
            nodes[ni].count = count as u32;
            continue;
        }
        // binned SAH
        const B: usize = 12;
        let mut bins = [(Vec3::splat(f32::MAX), Vec3::splat(f32::MIN), 0usize); B];
        let k = B as f32 / ext[axis];
        let bin_of = |c: Vec3| (((c[axis] - clo[axis]) * k) as usize).min(B - 1);
        for &i in &idx[s..e] {
            let b = &bounds[i as usize];
            let bi = bin_of(b.2);
            bins[bi].0 = bins[bi].0.min(b.0);
            bins[bi].1 = bins[bi].1.max(b.1);
            bins[bi].2 += 1;
        }
        let area = |lo: Vec3, hi: Vec3| {
            let d = (hi - lo).max(Vec3::ZERO);
            d.x * d.y + d.y * d.z + d.z * d.x
        };
        let mut best = (f32::MAX, 0usize);
        for split in 1..B {
            let (mut l0, mut l1, mut lc) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN), 0usize);
            let (mut r0, mut r1, mut rc) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN), 0usize);
            for (j, b) in bins.iter().enumerate() {
                if b.2 == 0 {
                    continue;
                }
                if j < split {
                    l0 = l0.min(b.0);
                    l1 = l1.max(b.1);
                    lc += b.2;
                } else {
                    r0 = r0.min(b.0);
                    r1 = r1.max(b.1);
                    rc += b.2;
                }
            }
            if lc == 0 || rc == 0 {
                continue;
            }
            let cost = area(l0, l1) * lc as f32 + area(r0, r1) * rc as f32;
            if cost < best.0 {
                best = (cost, split);
            }
        }
        let mid = if best.0 < f32::MAX {
            let split = best.1;
            let slice = &mut idx[s..e];
            let mut m = 0;
            for j in 0..slice.len() {
                if bin_of(bounds[slice[j] as usize].2) < split {
                    slice.swap(j, m);
                    m += 1;
                }
            }
            s + m
        } else {
            s + count / 2
        };
        let mid = if mid == s || mid == e { s + count / 2 } else { mid };
        let left = nodes.len();
        nodes.push(Node::default());
        nodes.push(Node::default());
        nodes[ni].first = left as u32;
        nodes[ni].count = 0;
        stack.push((left, s, mid, depth + 1));
        stack.push((left + 1, mid, e, depth + 1));
    }
    Bvh { nodes, idx }
}

#[inline]
fn ray_box(o: Vec3, inv: Vec3, lo: Vec3, hi: Vec3, tmax: f32) -> f32 {
    let t0 = (lo - o) * inv;
    let t1 = (hi - o) * inv;
    let tmin = t0.min(t1).max_element().max(0.0);
    let tm = t0.max(t1).min_element().min(tmax);
    if tmin <= tm { tmin } else { f32::INFINITY }
}

#[inline]
fn ray_tri(o: Vec3, d: Vec3, t: &Tri) -> Option<(f32, f32, f32)> {
    let p = d.cross(t.e2);
    let det = t.e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - t.p0;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(t.e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let tt = t.e2.dot(q) * inv;
    (tt > 1e-5).then_some((tt, u, v))
}

pub(crate) struct Tex {
    pub(crate) w: u32,
    pub(crate) h: u32,
    pub(crate) px: Vec<[f32; 4]>,
}

impl Tex {
    fn from(img: image::RgbaImage, srgb: bool) -> Tex {
        let img = if img.width() > 1024 || img.height() > 1024 { image::imageops::thumbnail(&img, 1024, 1024) } else { img };
        let conv = |c: u8| {
            let c = c as f32 / 255.0;
            if srgb { if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) } } else { c }
        };
        Tex { w: img.width(), h: img.height(), px: img.pixels().map(|p| [conv(p[0]), conv(p[1]), conv(p[2]), p[3] as f32 / 255.0]).collect() }
    }
    fn sample(&self, uv: Vec2) -> [f32; 4] {
        let x = (uv.x - uv.x.floor()) * self.w as f32 - 0.5;
        let y = (uv.y - uv.y.floor()) * self.h as f32 - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |xi: f32, yi: f32| {
            let xi = (xi as i64).rem_euclid(self.w as i64) as usize;
            let yi = (yi as i64).rem_euclid(self.h as i64) as usize;
            self.px[yi * self.w as usize + xi]
        };
        let (a, b, c, d) = (at(x0, y0), at(x0 + 1.0, y0), at(x0, y0 + 1.0), at(x0 + 1.0, y0 + 1.0));
        let mut o = [0.0; 4];
        for k in 0..4 {
            o[k] = (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy;
        }
        o
    }
}

pub(crate) struct Cam {
    pub(crate) eye: Vec3,
    pub(crate) fwd: Vec3,
    pub(crate) right: Vec3,
    pub(crate) up: Vec3,
    pub(crate) tan_half: f32,
    pub(crate) ortho: bool,
    pub(crate) half_h: f32,
}

pub(crate) struct Scene {
    pub(crate) tris: Vec<Tri>,
    pub(crate) attrs: Vec<Attr>,
    pub(crate) bvh: Bvh,
    pub(crate) mats: Vec<[f32; 16]>,
    /// texture layer → (srgb albedo version, linear version)
    pub(crate) tex_srgb: Vec<Option<Tex>>,
    pub(crate) tex_lin: Vec<Option<Tex>>,
    pub(crate) lights: Vec<WorldLight>,
    pub(crate) sun: Option<(Vec3, f32)>,
    pub(crate) env: Option<Tex>,
    pub(crate) env_rot: f32,
    pub(crate) sky_scale: f32,
    pub(crate) clip: Option<(Vec3, f32)>,
    pub(crate) cam: Cam,
    pub(crate) w: u32,
    pub(crate) h: u32,
}

pub(crate) const SUN_LUX: f32 = 90000.0;
pub(crate) const SKY_NITS: f32 = 2500.0;
pub(crate) const LAMP_NITS: f32 = 30000.0;

fn srgb_lin(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

impl Scene {
    fn intersect(&self, o: Vec3, d: Vec3, tmax: f32, shadow: bool) -> Option<(f32, u32, f32, f32)> {
        let inv = Vec3::new(1.0 / d.x, 1.0 / d.y, 1.0 / d.z);
        let mut best: Option<(f32, u32, f32, f32)> = None;
        let mut tbest = tmax;
        let mut stack = [0u32; 96];
        let mut sp = 1;
        while sp > 0 {
            sp -= 1;
            let node = &self.bvh.nodes[stack[sp] as usize];
            if ray_box(o, inv, node.min, node.max, tbest) == f32::INFINITY {
                continue;
            }
            if node.count > 0 {
                for k in node.first..node.first + node.count {
                    let ti = self.bvh.idx[k as usize];
                    if let Some((t, u, v)) = ray_tri(o, d, &self.tris[ti as usize]) {
                        if t >= tbest {
                            continue;
                        }
                        let a = &self.attrs[ti as usize];
                        if let Some((n, dd)) = self.clip {
                            if n.dot(o + d * t) - dd > 0.0 {
                                continue;
                            }
                        }
                        if shadow {
                            // glass and lamp surfaces do not cast shadows
                            if a.col[3] < 0.999 || self.emissive(a) {
                                continue;
                            }
                            return Some((t, ti, u, v));
                        }
                        tbest = t;
                        best = Some((t, ti, u, v));
                    }
                }
            } else if sp + 2 <= stack.len() {
                let (l, r) = (node.first, node.first + 1);
                let (nl, nr) = (&self.bvh.nodes[l as usize], &self.bvh.nodes[r as usize]);
                let (dl, dr) = (ray_box(o, inv, nl.min, nl.max, tbest), ray_box(o, inv, nr.min, nr.max, tbest));
                // nearer child last (popped first)
                if dl < dr {
                    stack[sp] = r;
                    stack[sp + 1] = l;
                } else {
                    stack[sp] = l;
                    stack[sp + 1] = r;
                }
                sp += 2;
            }
        }
        best
    }

    fn slot(&self, a: &Attr, front: bool) -> Option<usize> {
        let both = a.layer >= 999.5;
        let s = if both { a.layer - 1000.0 } else { a.layer };
        (s > -0.5 && (front || both)).then_some((s + 0.5) as usize).filter(|s| *s < self.mats.len())
    }

    fn emissive(&self, a: &Attr) -> bool {
        self.slot(a, true).is_some_and(|s| self.mats[s][13] > 0.0)
    }

    fn sky(&self, d: Vec3) -> Vec3 {
        let c = match &self.env {
            Some(t) => {
                let phi = d.y.atan2(d.x) + self.env_rot;
                let theta = d.z.clamp(-1.0, 1.0).acos();
                let s = t.sample(Vec2::new((0.5 - phi / std::f32::consts::TAU).rem_euclid(1.0), theta / std::f32::consts::PI));
                Vec3::new(s[0], s[1], s[2])
            }
            None => {
                let mix = |a: Vec3, b: Vec3, t: f32| a + (b - a) * t;
                let s = if d.z >= 0.0 {
                    mix(Vec3::new(0.965, 0.970, 0.975), Vec3::new(0.80, 0.85, 0.90), d.z.clamp(0.0, 1.0).sqrt())
                } else {
                    mix(Vec3::new(0.80, 0.80, 0.775), Vec3::new(0.74, 0.74, 0.715), (-d.z).clamp(0.0, 1.0).sqrt())
                };
                Vec3::new(srgb_lin(s.x), srgb_lin(s.y), srgb_lin(s.z))
            }
        };
        c * self.sky_scale
    }
}

// ------------------------------------------------------------------ sampling

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        // splitmix64 so that nearby seeds give unrelated sequences
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        let mut r = Rng(z);
        r.next();
        r
    }
    fn next(&mut self) -> u32 {
        let old = self.0;
        self.0 = old.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let xs = (((old >> 18) ^ old) >> 27) as u32;
        xs.rotate_right((old >> 59) as u32)
    }
    fn f(&mut self) -> f32 {
        (self.next() >> 8) as f32 / 16777216.0
    }
}

fn basis(n: Vec3) -> (Vec3, Vec3) {
    let s = if n.z >= 0.0 { 1.0 } else { -1.0 };
    let a = -1.0 / (s + n.z);
    let b = n.x * n.y * a;
    (Vec3::new(1.0 + s * n.x * n.x * a, s * b, -s * n.x), Vec3::new(b, s + n.y * n.y * a, -n.y))
}

const PI: f32 = std::f32::consts::PI;

struct Surf {
    p: Vec3,
    n: Vec3,
    ng: Vec3,
    albedo: Vec3,
    rough: f32,
    metal: f32,
    alpha: f32,
    emissive: bool,
}

fn ggx_d(ndh: f32, a2: f32) -> f32 {
    let d = ndh * ndh * (a2 - 1.0) + 1.0;
    a2 / (PI * d * d).max(1e-12)
}

fn smith_g(ndv: f32, ndl: f32, a: f32) -> f32 {
    let k = a * 0.5;
    (ndv / (ndv * (1.0 - k) + k)) * (ndl / (ndl * (1.0 - k) + k))
}

/// BRDF × cos for light direction l, view direction v.
fn brdf(s: &Surf, v: Vec3, l: Vec3) -> Vec3 {
    let ndl = s.n.dot(l);
    let ndv = s.n.dot(v).max(1e-4);
    if ndl <= 0.0 {
        return Vec3::ZERO;
    }
    let h = (l + v).normalize_or_zero();
    let a = (s.rough * s.rough).max(0.002);
    let a2 = a * a;
    let f0 = Vec3::splat(0.04).lerp(s.albedo, s.metal);
    let vdh = v.dot(h).max(0.0);
    let f = f0 + (Vec3::ONE - f0) * (1.0 - vdh).powi(5);
    let spec = f * (ggx_d(s.n.dot(h).max(0.0), a2) * smith_g(ndv, ndl, a) / (4.0 * ndv * ndl).max(1e-6));
    let diff = s.albedo * (1.0 - s.metal) * (Vec3::ONE - f) / PI;
    (diff + spec) * ndl
}

fn spec_prob(s: &Surf) -> f32 {
    (0.12 + 0.88 * s.metal).clamp(0.1, 0.95) * (1.0 - 0.5 * s.rough * (1.0 - s.metal))
}

/// Sample a direction; returns (direction, throughput weight, was specular).
fn sample_brdf(s: &Surf, v: Vec3, rng: &mut Rng) -> Option<(Vec3, Vec3, bool)> {
    let (t, b) = basis(s.n);
    let ps = spec_prob(s);
    let a = (s.rough * s.rough).max(0.002);
    let a2 = a * a;
    let (u1, u2) = (rng.f(), rng.f());
    let l = if rng.f() < ps {
        let phi = 2.0 * PI * u1;
        let ct = ((1.0 - u2) / (1.0 + (a2 - 1.0) * u2)).sqrt();
        let st = (1.0 - ct * ct).max(0.0).sqrt();
        let h = t * (st * phi.cos()) + b * (st * phi.sin()) + s.n * ct;
        2.0 * v.dot(h) * h - v
    } else {
        let r = u1.sqrt();
        let phi = 2.0 * PI * u2;
        t * (r * phi.cos()) + b * (r * phi.sin()) + s.n * (1.0 - u1).max(0.0).sqrt()
    };
    let ndl = s.n.dot(l);
    if ndl <= 0.0 || s.ng.dot(l) <= 0.0 {
        return None;
    }
    let h = (l + v).normalize_or_zero();
    let pdf_s = ggx_d(s.n.dot(h).max(0.0), a2) * s.n.dot(h).max(0.0) / (4.0 * v.dot(h).abs()).max(1e-6);
    let pdf_d = ndl / PI;
    let pdf = ps * pdf_s + (1.0 - ps) * pdf_d;
    if pdf <= 1e-8 {
        return None;
    }
    let w = brdf(s, v, l) / pdf;
    Some((l, w, s.rough < 0.25))
}

impl Scene {
    fn surface(&self, ti: u32, u: f32, v: f32, d: Vec3) -> (Surf, bool) {
        let tri = &self.tris[ti as usize];
        let a = &self.attrs[ti as usize];
        let w = 1.0 - u - v;
        let p = tri.p0 + tri.e1 * u + tri.e2 * v;
        let mut ng = tri.e1.cross(tri.e2).normalize_or_zero();
        let front = d.dot(ng) < 0.0;
        let mut n = (a.n[0] * w + a.n[1] * u + a.n[2] * v).normalize_or(ng);
        if !front {
            ng = -ng;
            n = -n;
        }
        if n.dot(ng) < 0.0 {
            n = ng;
        }
        let q = a.q[0] * w + a.q[1] * u + a.q[2] * v;
        let uv = (a.uv[0] * w + a.uv[1] * u + a.uv[2] * v) / q.max(1e-6);
        let base = if front { [a.col[0], a.col[1], a.col[2]] } else { a.back };
        let mut albedo = Vec3::new(srgb_lin(base[0]), srgb_lin(base[1]), srgb_lin(base[2]));
        let (mut rough, mut metal) = (0.85f32, 0.0f32);
        let mut emissive = false;
        if let Some(sl) = self.slot(a, front) {
            let m = &self.mats[sl];
            rough = m[6];
            metal = m[5];
            emissive = m[13] > 0.0;
            let layer = |x: f32| (x > -0.5).then_some((x + 0.5) as usize);
            let pick = |c: [f32; 4], ch: f32| c[(ch as usize).min(3)];
            if let Some(t) = layer(m[0]).and_then(|l| self.tex_srgb.get(l)).and_then(|t| t.as_ref()) {
                let c = t.sample(uv);
                albedo *= Vec3::new(c[0], c[1], c[2]);
            }
            if let Some(t) = layer(m[2]).and_then(|l| self.tex_lin.get(l)).and_then(|t| t.as_ref()) {
                let mut r = pick(t.sample(uv), m[8]);
                if m[11] > 0.5 {
                    r = 1.0 - r;
                }
                rough *= r;
            }
            if let Some(t) = layer(m[3]).and_then(|l| self.tex_lin.get(l)).and_then(|t| t.as_ref()) {
                metal *= pick(t.sample(uv), m[9]);
            }
            if let Some(t) = layer(m[4]).and_then(|l| self.tex_lin.get(l)).and_then(|t| t.as_ref()) {
                albedo *= 1.0 + (pick(t.sample(uv), m[10]) - 1.0) * m[12].clamp(0.0, 1.0);
            }
            if let Some(t) = layer(m[1]).and_then(|l| self.tex_lin.get(l)).and_then(|t| t.as_ref()) {
                let c = t.sample(uv);
                let tn = Vec3::new(c[0] * 2.0 - 1.0, c[1] * 2.0 - 1.0, c[2] * 2.0 - 1.0);
                let tt = (a.tan - n * n.dot(a.tan)).normalize_or_zero();
                if tt.length_squared() > 0.5 {
                    let bb = n.cross(tt) * a.tsign;
                    let nn = (tt * (tn.x * m[7]) - bb * (tn.y * m[7]) + n * tn.z).normalize_or(n);
                    if nn.dot(ng) > 0.05 {
                        n = nn;
                    }
                }
            }
        }
        (Surf { p, n, ng, albedo: albedo.clamp(Vec3::ZERO, Vec3::ONE), rough: rough.clamp(0.03, 1.0), metal: metal.clamp(0.0, 1.0), alpha: a.col[3], emissive }, front)
    }

    /// Direct light from lamps and sun at a surface.
    fn direct(&self, s: &Surf, v: Vec3, rng: &mut Rng) -> Vec3 {
        let mut sum = Vec3::ZERO;
        let o = s.p + s.ng * 2e-4;
        for l in &self.lights {
            let flux = Vec3::new(l.flux[0], l.flux[1], l.flux[2]);
            let (pos, dir) = (l.pos.as_vec3(), l.dir.as_vec3());
            match l.kind {
                LightKind::Panel => {
                    let (u, w) = (l.u.as_vec3(), l.v.as_vec3());
                    let area = 4.0 * u.length() * w.length();
                    if area <= 1e-6 {
                        continue;
                    }
                    let q = pos + u * (2.0 * rng.f() - 1.0) + w * (2.0 * rng.f() - 1.0);
                    let lv = q - o;
                    let d2 = lv.length_squared().max(1e-6);
                    let wi = lv / d2.sqrt();
                    let cl = (-wi).dot(dir);
                    if cl <= 0.0 {
                        continue;
                    }
                    let f = brdf(s, v, wi);
                    if f.max_element() <= 0.0 || self.intersect(o, wi, d2.sqrt() - 1e-3, true).is_some() {
                        continue;
                    }
                    let le = flux / (PI * area);
                    sum += f * le * (cl * area / d2);
                }
                _ => {
                    // jitter the target inside the bulb for soft shadows
                    let mut q = pos;
                    if l.radius > 0.0 {
                        let z = 2.0 * rng.f() - 1.0;
                        let ph = 2.0 * PI * rng.f();
                        let r = (1.0 - z * z).max(0.0).sqrt();
                        q += Vec3::new(r * ph.cos(), r * ph.sin(), z) * l.radius;
                    }
                    let lv = q - o;
                    let d2 = lv.length_squared().max(1e-6);
                    let wi = lv / d2.sqrt();
                    let inten = if l.kind == LightKind::Spot {
                        let c = (-wi).dot(dir);
                        let t = ((c - l.cos_outer) / (l.cos_inner - l.cos_outer).max(1e-4)).clamp(0.0, 1.0);
                        flux / (2.0 * PI * (1.0 - l.cos_outer).max(0.02)) * (t * t * (3.0 - 2.0 * t))
                    } else {
                        flux / (4.0 * PI)
                    };
                    if inten.max_element() <= 0.0 {
                        continue;
                    }
                    let f = brdf(s, v, wi);
                    if f.max_element() <= 0.0 || self.intersect(o, wi, d2.sqrt() - l.radius.max(1e-3), true).is_some() {
                        continue;
                    }
                    sum += f * inten / d2;
                }
            }
        }
        if let Some((sd, e)) = self.sun {
            // sun disk ~0.27° radius
            let (t, b) = basis(sd);
            let r = 0.0047 * rng.f().sqrt();
            let ph = 2.0 * PI * rng.f();
            let wi = (sd + t * (r * ph.cos()) + b * (r * ph.sin())).normalize();
            let f = brdf(s, v, wi);
            if f.max_element() > 0.0 && self.intersect(o, wi, 1e7, true).is_none() {
                sum += f * e;
            }
        }
        sum
    }

    /// One path sample; also returns first-hit albedo, normal and depth (for the denoiser).
    fn radiance(&self, mut o: Vec3, mut d: Vec3, rng: &mut Rng, clamp: f32) -> (Vec3, Vec3, Vec3, f32) {
        let mut l = Vec3::ZERO;
        let mut thr = Vec3::ONE;
        let mut specular = true;
        let (mut alb, mut nor, mut depth) = (Vec3::ONE, Vec3::ZERO, 1e9f32);
        for bounce in 0..10 {
            let Some((t, ti, u, v)) = self.intersect(o, d, 1e7, false) else {
                let sky = self.sky(d);
                l += thr * if bounce > 1 { sky.min(Vec3::splat(clamp)) } else { sky };
                if bounce == 0 {
                    alb = sky.normalize_or(Vec3::ONE);
                }
                break;
            };
            let (s, _) = self.surface(ti, u, v, d);
            if bounce == 0 {
                alb = if s.emissive { Vec3::ONE } else { s.albedo.max(Vec3::splat(0.02)) };
                nor = s.n;
                depth = t;
            }
            if s.emissive {
                if specular {
                    l += thr * s.albedo * LAMP_NITS;
                }
                break;
            }
            let wo = -d;
            if s.alpha < 0.999 {
                // thin glass: Fresnel reflection or straight transmission tinted by the color
                let c = wo.dot(s.n).abs();
                let f = 0.04 + 0.96 * (1.0 - c).powi(5);
                if rng.f() < f {
                    d = (2.0 * wo.dot(s.n) * s.n - wo).normalize();
                    o = s.p + s.ng * 2e-4;
                } else {
                    thr *= Vec3::ONE.lerp(s.albedo, s.alpha.clamp(0.0, 1.0) * 0.6);
                    o = s.p + d * 2e-4;
                }
                continue;
            }
            let direct = self.direct(&s, wo, rng);
            let direct = if bounce > 0 { direct.min(Vec3::splat(clamp)) } else { direct };
            l += thr * direct;
            let Some((nd, w, spec)) = sample_brdf(&s, wo, rng) else { break };
            thr *= w;
            specular = spec;
            o = s.p + s.ng * 2e-4;
            d = nd;
            if bounce >= 3 {
                let p = thr.max_element().clamp(0.05, 0.95);
                if rng.f() > p {
                    break;
                }
                thr /= p;
            }
        }
        (l, alb, nor, depth)
    }

    fn camera_ray(&self, x: f32, y: f32) -> (Vec3, Vec3) {
        let c = &self.cam;
        let aspect = self.w as f32 / self.h as f32;
        let nx = (x / self.w as f32) * 2.0 - 1.0;
        let ny = 1.0 - (y / self.h as f32) * 2.0;
        if c.ortho {
            let o = c.eye + c.right * (nx * c.half_h * aspect) + c.up * (ny * c.half_h) - c.fwd * 1000.0;
            (o, c.fwd)
        } else {
            (c.eye, (c.fwd + c.right * (nx * c.tan_half * aspect) + c.up * (ny * c.tan_half)).normalize())
        }
    }
}

// ------------------------------------------------------------------ building

pub(crate) fn build_scene(doc: &crate::doc::Document, cam: &crate::camera::Camera, size: [u32; 2], decode_textures: bool) -> Scene {
    let mut d = doc.clone();
    d.style.face = crate::settings::FaceStyle::Photoreal;
    d.style.xray = false;
    let sel = crate::geom::Sel::default();
    let opts = crate::scene::SceneOpts { path: &[], sel: &sel, show_sel: false, highlight_face: None, highlight_inst: None, axes: false, color_by_tag: false, hidden_geometry: false };
    let sc = crate::scene::build(&d, &opts);
    let mut tris = Vec::new();
    let mut attrs = Vec::new();
    for list in [&sc.tris, &sc.nocast, &sc.glass] {
        for t in list.chunks_exact(3) {
            let p: [Vec3; 3] = [0, 1, 2].map(|k| Vec3::from(t[k].pos));
            let (e1, e2) = (p[1] - p[0], p[2] - p[0]);
            if e1.cross(e2).length_squared() < 1e-16 {
                continue;
            }
            tris.push(Tri { p0: p[0], e1, e2 });
            attrs.push(Attr {
                n: [0, 1, 2].map(|k| Vec3::from(t[k].nrm)),
                uv: [0, 1, 2].map(|k| Vec2::from(t[k].uv)),
                q: [0, 1, 2].map(|k| t[k].tan[3].abs().max(1e-6)),
                tan: Vec3::new(t[0].tan[0], t[0].tan[1], t[0].tan[2]),
                tsign: t[0].tan[3].signum(),
                col: t[0].col,
                back: [t[0].back[0], t[0].back[1], t[0].back[2]],
                layer: t[0].layer,
            });
        }
    }
    let bvh = build_bvh(&tris);
    let n = doc.textures.len().min(crate::tex::MAX_LAYERS);
    let mut tex_srgb = Vec::with_capacity(n);
    let mut tex_lin = Vec::with_capacity(n);
    for t in doc.textures.iter().take(if decode_textures { n } else { 0 }) {
        let img = t.decode();
        tex_srgb.push(img.clone().map(|i| Tex::from(i, true)));
        tex_lin.push(img.map(|i| Tex::from(i, false)));
    }
    let sh = &doc.shadows;
    let sun = sh.on.then(|| crate::settings::sun_direction(sh, &doc.location)).filter(|s| s.z > 0.0).map(|s| (s.as_vec3(), SUN_LUX * (sh.light / 100.0).max(0.0)));
    let env = doc.env.image.and_then(|i| doc.env_images.get(i)).and_then(|t| t.decode()).map(|i| Tex::from(i, true));
    let sky_scale = SKY_NITS * if env.is_some() { doc.env.sky_exposure } else { 1.0 } * if doc.style.sky || env.is_some() { 1.0 } else { 0.6 };
    let clip = crate::annot::active_section(doc).map(|p| (p.n.as_vec3(), p.d as f32));
    let fwd = cam.forward().as_vec3();
    let up0 = cam.view_up().as_vec3();
    let right = fwd.cross(up0).normalize_or(Vec3::X);
    let up = right.cross(fwd);
    let half_h = (cam.dist * (cam.fov_y * 0.5).tan()) as f32;
    Scene {
        tris,
        attrs,
        bvh,
        mats: sc.mats,
        tex_srgb,
        tex_lin,
        lights: crate::lights::world_lights(doc),
        sun,
        env,
        env_rot: doc.env.rotation_deg.to_radians(),
        sky_scale,
        clip,
        cam: Cam { eye: cam.eye().as_vec3(), fwd, right, up, tan_half: (cam.fov_y as f32 * 0.5).tan(), ortho: cam.ortho, half_h },
        w: size[0],
        h: size[1],
    }
}

// ------------------------------------------------------------------ post

fn aces(x: f32) -> f32 {
    ((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14)).clamp(0.0, 1.0)
}

fn to_srgb8(c: f32) -> u8 {
    let c = if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Edge-avoiding à-trous filter on illumination (color / albedo).
fn denoise(col: &[Vec3], alb: &[Vec3], nor: &[Vec3], dep: &[f32], w: usize, h: usize, spp: u32) -> Vec<Vec3> {
    let mut illum: Vec<Vec3> = col.iter().zip(alb).map(|(c, a)| *c / a.max(Vec3::splat(0.02))).collect();
    // outlier rejection: a pixel far darker or brighter than all its neighbours on
    // the same surface takes their mean (fireflies and unlucky dark samples)
    {
        let src = illum.clone();
        let lum = |c: Vec3| c.dot(Vec3::new(0.2126, 0.7152, 0.0722));
        for y in 1..h.saturating_sub(1) {
            for x in 1..w.saturating_sub(1) {
                let i = y * w + x;
                let (mut lo, mut hi, mut sum, mut n) = (f32::MAX, 0.0f32, Vec3::ZERO, 0.0);
                for (dx, dy) in [(-1i32, -1i32), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
                    let j = (y as i32 + dy) as usize * w + (x as i32 + dx) as usize;
                    if nor[i].dot(nor[j]) < 0.9 || (dep[i] - dep[j]).abs() > 0.05 * dep[i].max(0.01) {
                        continue;
                    }
                    let l = lum(src[j]);
                    lo = lo.min(l);
                    hi = hi.max(l);
                    sum += src[j];
                    n += 1.0;
                }
                if n >= 4.0 {
                    let l = lum(src[i]);
                    if l < lo * 0.6 || l > hi * 1.6 {
                        illum[i] = sum / n;
                    }
                }
            }
        }
    }
    let kern = [1.0 / 16.0, 1.0 / 4.0, 3.0 / 8.0, 1.0 / 4.0, 1.0 / 16.0];
    let sigma_l = (2.0 / (spp as f32).sqrt()).clamp(0.15, 2.0);
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    for it in 0..5 {
        let step = 1usize << it;
        let src = illum.clone();
        let rows = Mutex::new(illum.chunks_mut(w).enumerate());
        std::thread::scope(|sc| {
            for _ in 0..threads {
                sc.spawn(|| loop {
                    let Some((y, row)) = rows.lock().unwrap().next() else { break };
                    for x in 0..w {
                        let i = y * w + x;
                        let (cp, np, zp) = (src[i], nor[i], dep[i]);
                        let lp = cp.dot(Vec3::new(0.2126, 0.7152, 0.0722));
                        let mut acc = Vec3::ZERO;
                        let mut wsum = 0.0;
                        for (ky, wy) in kern.iter().enumerate() {
                            let yy = y as isize + (ky as isize - 2) * step as isize;
                            if yy < 0 || yy >= h as isize {
                                continue;
                            }
                            for (kx, wx) in kern.iter().enumerate() {
                                let xx = x as isize + (kx as isize - 2) * step as isize;
                                if xx < 0 || xx >= w as isize {
                                    continue;
                                }
                                let j = yy as usize * w + xx as usize;
                                let cq = src[j];
                                let lq = cq.dot(Vec3::new(0.2126, 0.7152, 0.0722));
                                let wn = np.dot(nor[j]).max(0.0).powi(32);
                                let wz = (-(zp - dep[j]).abs() / (0.03 * zp.max(0.01) * step as f32)).exp();
                                let wl = (-(lp - lq).abs() / (sigma_l * (lp.max(lq) + 1e-3))).exp();
                                let ww = wx * wy * wn * wz * wl;
                                acc += cq * ww;
                                wsum += ww;
                            }
                        }
                        row[x] = if wsum > 1e-9 { acc / wsum } else { cp };
                    }
                });
            }
        });
    }
    for (v, a) in illum.iter_mut().zip(alb) {
        *v *= a.max(Vec3::splat(0.02));
    }
    illum
}

// ------------------------------------------------------------------ job

pub struct Job {
    pub size: [u32; 2],
    pub target: u32,
    pub spp: AtomicU32,
    pub cancel: AtomicBool,
    pub finished: AtomicBool,
    /// exposure EV offset ×100 and denoise flag, read when producing images
    pub ev: AtomicU32,
    pub denoise: AtomicBool,
    pub version: AtomicU64,
    pub preview: Mutex<Option<(image::RgbaImage, u64)>>,
    pub save: Mutex<Option<std::path::PathBuf>>,
    pub status: Mutex<String>,
    pub started: std::time::Instant,
    pub elapsed_ms: AtomicU64,
}

pub(crate) struct Buffers {
    pub(crate) acc: Vec<Vec3>,
    pub(crate) alb: Vec<Vec3>,
    pub(crate) nor: Vec<Vec3>,
    pub(crate) dep: Vec<f32>,
    pub(crate) spp: u32,
    pub(crate) auto: f32,
}

pub(crate) fn image_from(job: &Job, b: &Buffers, w: usize, h: usize, max_w: Option<usize>) -> image::RgbaImage {
    let col: Vec<Vec3> = b.acc.iter().map(|c| *c / b.spp.max(1) as f32).collect();
    let col = if job.denoise.load(Ordering::Relaxed) && b.spp >= 4 && max_w.is_none() { denoise(&col, &b.alb, &b.nor, &b.dep, w, h, b.spp) } else { col };
    let ev = (job.ev.load(Ordering::Relaxed) as f32 - 1000.0) / 100.0;
    let k = b.auto * 2f32.powf(ev);
    let (ow, step) = match max_w {
        Some(m) if w > m => (m, w as f32 / m as f32),
        _ => (w, 1.0),
    };
    let oh = ((h as f32 / step).round() as usize).max(1);
    image::RgbaImage::from_fn(ow as u32, oh as u32, |x, y| {
        let sx = ((x as f32 * step) as usize).min(w - 1);
        let sy = ((y as f32 * step) as usize).min(h - 1);
        let c = col[sy * w + sx] * k;
        image::Rgba([to_srgb8(aces(c.x)), to_srgb8(aces(c.y)), to_srgb8(aces(c.z)), 255])
    })
}

pub(crate) fn run(job: Arc<Job>, doc: crate::doc::Document, cam: crate::camera::Camera, ctx: egui::Context) {
    let (w, h) = (job.size[0] as usize, job.size[1] as usize);
    *job.status.lock().unwrap() = "Preparando a cena (geometria, texturas, BVH)…".into();
    ctx.request_repaint();
    let scene = build_scene(&doc, &cam, job.size, true);
    drop(doc);
    *job.status.lock().unwrap() = format!("{} triângulos, {} luzes", scene.tris.len(), scene.lights.len());
    let mut b = Buffers { acc: vec![Vec3::ZERO; w * h], alb: vec![Vec3::ONE; w * h], nor: vec![Vec3::Z; w * h], dep: vec![1e9; w * h], spp: 0, auto: 1.0 };
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let mut last_preview = std::time::Instant::now();
    let mut shown_version = u64::MAX;
    loop {
        if job.cancel.load(Ordering::Relaxed) {
            break;
        }
        if b.spp < job.target && !job.finished.load(Ordering::Relaxed) {
            let pass = b.spp;
            let clamp = if pass == 0 { f32::MAX } else { 30.0 / b.auto.max(1e-9) };
            let first = pass == 0;
            {
                let rows = Mutex::new(b.acc.chunks_mut(w).zip(b.alb.chunks_mut(w)).zip(b.nor.chunks_mut(w)).zip(b.dep.chunks_mut(w)).enumerate());
                let scene = &scene;
                let job = &job;
                std::thread::scope(|sc| {
                    for _ in 0..threads {
                        sc.spawn(|| loop {
                            if job.cancel.load(Ordering::Relaxed) {
                                break;
                            }
                            let Some((y, (((acc, alb), nor), dep))) = rows.lock().unwrap().next() else { break };
                            for x in 0..w {
                                let mut rng = Rng::new(((pass as u64) << 40) ^ ((y as u64) << 20) ^ x as u64);
                                let (o, d) = scene.camera_ray(x as f32 + rng.f(), y as f32 + rng.f());
                                let (l, a, n, z) = scene.radiance(o, d, &mut rng, clamp);
                                // a broken sample (NaN) is replaced by the running mean
                                if l.is_finite() {
                                    acc[x] += l;
                                } else if pass > 0 {
                                    acc[x] += acc[x] / pass as f32;
                                }
                                if first {
                                    alb[x] = a;
                                    nor[x] = n;
                                    dep[x] = z;
                                }
                            }
                        });
                    }
                });
            }
            if job.cancel.load(Ordering::Relaxed) {
                break;
            }
            b.spp += 1;
            if b.spp == 1 || b.spp == 4 || b.spp == 16 {
                // automatic exposure: log-average luminance → middle grey
                let mut sum = 0.0f64;
                let mut cnt = 0usize;
                for c in b.acc.iter().step_by(7) {
                    let lum = (*c / b.spp as f32).dot(Vec3::new(0.2126, 0.7152, 0.0722));
                    if lum > 1e-6 {
                        sum += (lum as f64).ln();
                        cnt += 1;
                    }
                }
                if cnt > 0 {
                    b.auto = 0.18 / ((sum / cnt as f64).exp() as f32).max(1e-9);
                }
            }
            job.spp.store(b.spp, Ordering::Relaxed);
            job.elapsed_ms.store(job.started.elapsed().as_millis() as u64, Ordering::Relaxed);
            if b.spp >= job.target {
                job.finished.store(true, Ordering::Relaxed);
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
        let version = job.version.load(Ordering::Relaxed);
        let done = job.finished.load(Ordering::Relaxed);
        if version != shown_version || (last_preview.elapsed().as_millis() > 400 && !done) || (done && shown_version != u64::MAX - 1) {
            let mut img = image_from(&job, &b, w, h, Some(1600));
            if done && job.denoise.load(Ordering::Relaxed) {
                // the preview shows the denoised result once rendering stops
                let full = image_from(&job, &b, w, h, None);
                img = if w > 1600 { image::imageops::thumbnail(&full, 1600, (1600 * h / w) as u32) } else { full };
            }
            *job.preview.lock().unwrap() = Some((img, b.spp as u64 * 1000 + version));
            shown_version = if done { u64::MAX - 1 } else { version };
            if done && version != shown_version {
                shown_version = u64::MAX - 1;
            }
            last_preview = std::time::Instant::now();
            ctx.request_repaint();
        }
        if let Some(path) = job.save.lock().unwrap().take() {
            *job.status.lock().unwrap() = "Salvando…".into();
            ctx.request_repaint();
            let img = image_from(&job, &b, w, h, None);
            let res = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg")) {
                image::DynamicImage::ImageRgba8(img).to_rgb8().save(&path)
            } else {
                img.save(&path)
            };
            *job.status.lock().unwrap() = match res {
                Ok(()) => format!("Salvo: {}", path.display()),
                Err(e) => format!("Erro ao salvar: {e}"),
            };
            ctx.request_repaint();
        }
        if done {
            // keep serving exposure changes and saves until the window closes
            let v = job.version.load(Ordering::Relaxed);
            if v != version {
                shown_version = v.wrapping_sub(1);
            }
        }
    }
}

/// Render synchronously to a file (tests and the snapshot mode).
pub fn render_to_file(doc: &crate::doc::Document, cam: &crate::camera::Camera, size: [u32; 2], spp: u32, path: &std::path::Path, gpu: Option<crate::gpupt::GpuCtx>) -> Result<(), String> {
    let job = Arc::new(Job {
        size,
        target: spp,
        spp: AtomicU32::new(0),
        cancel: AtomicBool::new(false),
        finished: AtomicBool::new(false),
        ev: AtomicU32::new(1000),
        denoise: AtomicBool::new(true),
        version: AtomicU64::new(0),
        preview: Mutex::new(None),
        save: Mutex::new(Some(path.to_path_buf())),
        status: Mutex::new(String::new()),
        started: std::time::Instant::now(),
        elapsed_ms: AtomicU64::new(0),
    });
    let j = job.clone();
    let (doc, cam) = (doc.clone(), cam.clone());
    // the save request is served once the target is reached
    *job.save.lock().unwrap() = None;
    let handle = spawn_engine(j, doc, cam, egui::Context::default(), gpu).map_err(|e| e.to_string())?;
    while !job.finished.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    *job.save.lock().unwrap() = Some(path.to_path_buf());
    while job.save.lock().unwrap().is_some() {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
    let status = job.status.lock().unwrap().clone();
    job.cancel.store(true, Ordering::Relaxed);
    let _ = handle.join();
    eprintln!("pathtrace: {} spp in {:.1} s — {status}", job.spp.load(Ordering::Relaxed), job.started.elapsed().as_secs_f64());
    Ok(())
}

/// Start the render on a worker thread: on the GPU when there is one, else
/// (or if the GPU run fails) on the CPU.
fn spawn_engine(job: Arc<Job>, doc: crate::doc::Document, cam: crate::camera::Camera, ctx: egui::Context, gpu: Option<crate::gpupt::GpuCtx>) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new().name("homeup-pathtrace".into()).stack_size(64 << 20).spawn(move || {
        if let Some(g) = gpu {
            let (j, d, c, x) = (job.clone(), doc.clone(), cam.clone(), ctx.clone());
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || crate::gpupt::run_gpu(j, d, c, x, g))).is_ok() {
                return;
            }
            if job.cancel.load(Ordering::Relaxed) {
                return;
            }
            eprintln!("pathtrace: a GPU falhou; usando o processador");
            job.spp.store(0, Ordering::Relaxed);
            job.finished.store(false, Ordering::Relaxed);
        }
        run(job, doc, cam, ctx)
    })
}

pub struct RenderView {
    pub job: Arc<Job>,
    pub tex: Option<egui::TextureHandle>,
    pub tex_key: u64,
    pub ev: f32,
    pub denoise: bool,
}

impl HomeUpApp {
    pub(crate) fn start_path_trace(&mut self, ctx: &egui::Context, size: [u32; 2], samples: u32) {
        if let Some(v) = &self.render_view {
            v.job.cancel.store(true, Ordering::Relaxed);
        }
        let job = Arc::new(Job {
            size,
            target: samples.max(1),
            spp: AtomicU32::new(0),
            cancel: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            ev: AtomicU32::new(1000),
            denoise: AtomicBool::new(true),
            version: AtomicU64::new(0),
            preview: Mutex::new(None),
            save: Mutex::new(None),
            status: Mutex::new(String::new()),
            started: std::time::Instant::now(),
            elapsed_ms: AtomicU64::new(0),
        });
        let (doc, cam, ctx2, j) = (self.doc.clone(), self.cam.clone(), ctx.clone(), job.clone());
        let gpu = if std::env::var_os("HOMEUP_PT_CPU").is_some() { None } else { self.gpu_ctx.clone() };
        spawn_engine(j, doc, cam, ctx2, gpu).ok();
        self.render_view = Some(RenderView { job, tex: None, tex_key: u64::MAX, ev: 0.0, denoise: true });
    }

    pub(crate) fn render_window(&mut self, ctx: &egui::Context) {
        let Some(view) = self.render_view.as_mut() else { return };
        let job = view.job.clone();
        if let Some((img, key)) = job.preview.lock().unwrap().as_ref() {
            if *key != view.tex_key {
                let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                view.tex = Some(ctx.load_texture("homeup-render", ci, egui::TextureOptions::LINEAR));
                view.tex_key = *key;
            }
        }
        let mut open = true;
        let mut save = false;
        let mut stop = false;
        egui::Window::new("Render fotorrealista").open(&mut open).default_size([900.0, 620.0]).resizable(true).show(ctx, |ui| {
            let spp = job.spp.load(Ordering::Relaxed);
            let done = job.finished.load(Ordering::Relaxed);
            let el = job.elapsed_ms.load(Ordering::Relaxed) as f64 / 1000.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} × {}", job.size[0], job.size[1])).strong());
                ui.separator();
                ui.label(format!("{spp} / {} amostras", job.target));
                ui.separator();
                let eta = if spp > 0 && !done { el / spp as f64 * (job.target - spp) as f64 } else { 0.0 };
                ui.label(if done { format!("concluído em {:.0} s", el) } else if spp > 0 { format!("{:.0} s, faltam ~{:.0} s", el, eta) } else { "começando…".into() });
                ui.separator();
                ui.weak(job.status.lock().unwrap().clone());
            });
            ui.add(egui::ProgressBar::new(spp as f32 / job.target as f32).show_percentage());
            ui.horizontal(|ui| {
                ui.label("Exposição");
                if ui.add(egui::Slider::new(&mut view.ev, -4.0..=4.0).step_by(0.1).suffix(" EV")).changed() {
                    job.ev.store(((view.ev * 100.0).round() + 1000.0) as u32, Ordering::Relaxed);
                    job.version.fetch_add(1, Ordering::Relaxed);
                }
                if ui.checkbox(&mut view.denoise, "Remover ruído").changed() {
                    job.denoise.store(view.denoise, Ordering::Relaxed);
                    job.version.fetch_add(1, Ordering::Relaxed);
                }
                if ui.add_enabled(!done, egui::Button::new("Parar aqui")).clicked() {
                    stop = true;
                }
                if ui.button(RichText::new("Salvar imagem…").strong()).clicked() {
                    save = true;
                }
            });
            if let Some(t) = &view.tex {
                let avail = ui.available_size();
                let sz = t.size_vec2();
                let k = (avail.x / sz.x).min(avail.y / sz.y).min(1.0).max(0.05);
                ui.image((t.id(), sz * k));
            } else {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| ui.spinner());
            }
        });
        if stop {
            job.finished.store(true, Ordering::Relaxed);
            job.version.fetch_add(1, Ordering::Relaxed);
        }
        if save {
            if let Some(p) = rfd::FileDialog::new().add_filter("Imagem PNG", &["png"]).add_filter("JPEG", &["jpg"]).set_file_name("render.png").save_file() {
                *job.save.lock().unwrap() = Some(p);
            }
        }
        if !open {
            job.cancel.store(true, Ordering::Relaxed);
            self.render_view = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;

    #[test]
    fn bvh_hits_the_nearest_triangle() {
        let mk = |z: f32| Tri { p0: Vec3::new(-1.0, -1.0, z), e1: Vec3::new(2.0, 0.0, 0.0), e2: Vec3::new(0.0, 2.0, 0.0) };
        let tris: Vec<Tri> = (0..50).map(|k| mk(k as f32 * 0.5 + 1.0)).collect();
        let bvh = build_bvh(&tris);
        let attrs = vec![Attr { n: [Vec3::Z; 3], uv: [Vec2::ZERO; 3], q: [1.0; 3], tan: Vec3::X, tsign: 1.0, col: [1.0; 4], back: [1.0; 3], layer: -1.0 }; tris.len()];
        let sc = Scene {
            tris,
            attrs,
            bvh,
            mats: vec![],
            tex_srgb: vec![],
            tex_lin: vec![],
            lights: vec![],
            sun: None,
            env: None,
            env_rot: 0.0,
            sky_scale: 1.0,
            clip: None,
            cam: Cam { eye: Vec3::ZERO, fwd: Vec3::Z, right: Vec3::X, up: Vec3::Y, tan_half: 0.5, ortho: false, half_h: 1.0 },
            w: 4,
            h: 4,
        };
        let (t, ..) = sc.intersect(Vec3::new(-0.5, -0.5, 0.0), Vec3::Z, 1e9, false).unwrap();
        assert!((t - 1.0).abs() < 1e-4, "{t}");
        let (t, ..) = sc.intersect(Vec3::new(-0.5, -0.5, 10.2), Vec3::Z, 1e9, false).unwrap();
        assert!((t - 0.3).abs() < 1e-4, "{t}");
    }

    #[test]
    fn a_lit_room_renders_something() {
        // closed box with a bulb inside, camera inside: every pixel should get light
        let mut doc = crate::doc::Document::new();
        let s = 3.0;
        let p = |x: f64, y: f64, z: f64| DVec3::new(x, y, z);
        let eye_in = p(1.5, 1.5, 1.5);
        let faces = [
            [p(0., 0., 0.), p(s, 0., 0.), p(s, s, 0.), p(0., s, 0.)],
            [p(0., 0., s), p(0., s, s), p(s, s, s), p(s, 0., s)],
            [p(0., 0., 0.), p(0., s, 0.), p(0., s, s), p(0., 0., s)],
            [p(s, 0., 0.), p(s, 0., s), p(s, s, s), p(s, s, 0.)],
            [p(0., 0., 0.), p(0., 0., s), p(s, 0., s), p(s, 0., 0.)],
            [p(0., s, 0.), p(s, s, 0.), p(s, s, s), p(0., s, s)],
        ];
        for f in faces {
            doc.root.add_polygon(&f, None, false, eye_in);
        }
        let l = crate::lights::LightDef::new(crate::lights::LightKind::Point);
        doc.defs.push(crate::doc::Definition { name: "luz".into(), geom: crate::geom::Geom::default(), group: false, face_me: false, procedure: None, ifc: None, author: None, light: Some(l) });
        doc.root.instances.push(crate::geom::Instance { def: 0, xf: glam::DMat4::from_translation(p(1.5, 1.5, 2.9)), ..Default::default() });
        let mut cam = crate::camera::Camera::default();
        cam.target = p(1.5, 2.5, 1.2);
        cam.dist = 1.0;
        cam.yaw = -std::f64::consts::FRAC_PI_2;
        cam.pitch = 0.1;
        let sc = build_scene(&doc, &cam, [16, 12], true);
        assert!(sc.tris.len() >= 12);
        let mut rng = Rng::new(7);
        let mut total = 0.0;
        for k in 0..64 {
            let (o, d) = sc.camera_ray((k % 16) as f32 + 0.5, (k / 16) as f32 * 3.0 + 0.5);
            total += sc.radiance(o, d, &mut rng, f32::MAX).0.length();
        }
        assert!(total > 0.0 && total.is_finite(), "{total}");
        assert!(aces(10.0) <= 1.0 && to_srgb8(1.0) == 255);
    }
}
