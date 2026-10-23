//! GPU path tracer (wgpu compute). Same scene, BVH, materials, lights and
//! sampling as the CPU tracer in pathtrace.rs, so both give the same picture;
//! the CPU one remains the fallback when the GPU cannot run it.
//!
//! Each pass adds one sample per pixel. Large images are split into tiles that
//! fit the GPU's buffer limits, and each pass is dispatched in chunks so no
//! single GPU command runs long enough to trip the driver watchdog.

use crate::pathtrace::{image_from, Buffers, Job, Scene, LAMP_NITS};
use eframe::egui;
use eframe::wgpu;
use eframe::wgpu::util::DeviceExt;
use glam::Vec3;
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub const SHADER: &str = r#"
struct P {
    eye: vec4<f32>,
    fwd: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    cam: vec4<f32>,      // tan_half, aspect, ortho, half_h
    size: vec4<u32>,     // image w, h, pass, light count
    tile: vec4<u32>,     // buffer origin x, y, width, height
    chunk: vec4<u32>,    // dispatch origin x, y, width, height
    sun: vec4<f32>,      // dir, irradiance (0 = none)
    sky: vec4<f32>,      // scale, env rotation, has env, indirect clamp
    clip: vec4<f32>,     // plane normal, d
    flags: vec4<f32>,    // clip on, mat count, lamp nits, -
};
@group(0) @binding(0) var<uniform> p: P;
@group(0) @binding(1) var<storage, read> tris: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> attrs: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> nodes: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read> idx: array<u32>;
@group(0) @binding(5) var<storage, read> mats: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read> lights: array<vec4<f32>>;
@group(0) @binding(7) var<storage, read_write> accum: array<vec4<f32>>;
@group(0) @binding(8) var<storage, read_write> aux: array<vec4<u32>>;
@group(0) @binding(9) var tex: texture_2d_array<f32>;
@group(0) @binding(10) var env: texture_2d<f32>;
@group(0) @binding(11) var smp: sampler;

const PI: f32 = 3.14159265;

var<private> seed: u32;

fn pcg(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}
fn rnd() -> f32 {
    seed = pcg(seed);
    return f32(seed >> 8u) / 16777216.0;
}

fn srgb_lin(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

struct Hit {
    t: f32,
    tri: u32,
    u: f32,
    v: f32,
    ok: bool,
};

fn slot_of(layer: f32, front: bool) -> i32 {
    let both = layer >= 999.5;
    let s = select(layer, layer - 1000.0, both);
    if (s > -0.5 && (front || both) && i32(s + 0.5) < i32(p.flags.y)) {
        return i32(s + 0.5);
    }
    return -1;
}

fn is_emissive(tri: u32) -> bool {
    let s = slot_of(attrs[tri * 8u].w, true);
    return s >= 0 && mats[u32(s) * 4u + 3u].y > 0.0;
}

fn box_t(o: vec3<f32>, inv: vec3<f32>, lo: vec3<f32>, hi: vec3<f32>, tmax: f32) -> f32 {
    let t0 = (lo - o) * inv;
    let t1 = (hi - o) * inv;
    let tn = max(max(min(t0.x, t1.x), min(t0.y, t1.y)), max(min(t0.z, t1.z), 0.0));
    let tf = min(min(max(t0.x, t1.x), max(t0.y, t1.y)), min(max(t0.z, t1.z), tmax));
    return select(1e30, tn, tn <= tf);
}

fn intersect(o: vec3<f32>, d: vec3<f32>, tmax: f32, shadow: bool) -> Hit {
    var h: Hit;
    h.ok = false;
    h.t = tmax;
    let inv = 1.0 / d;
    var stack: array<u32, 48>;
    var sp = 1;
    stack[0] = 0u;
    while (sp > 0) {
        sp = sp - 1;
        let ni = stack[sp];
        let a = nodes[ni * 2u];
        let b = nodes[ni * 2u + 1u];
        if (box_t(o, inv, a.xyz, b.xyz, h.t) >= 1e29) {
            continue;
        }
        let count = bitcast<u32>(b.w);
        let first = bitcast<u32>(a.w);
        if (count > 0u) {
            for (var k = first; k < first + count; k = k + 1u) {
                let ti = idx[k];
                let p0 = tris[ti * 3u].xyz;
                let e1 = tris[ti * 3u + 1u].xyz;
                let e2 = tris[ti * 3u + 2u].xyz;
                let pv = cross(d, e2);
                let det = dot(e1, pv);
                if (abs(det) < 1e-12) {
                    continue;
                }
                let id = 1.0 / det;
                let s = o - p0;
                let u = dot(s, pv) * id;
                if (u < 0.0 || u > 1.0) {
                    continue;
                }
                let q = cross(s, e1);
                let v = dot(d, q) * id;
                if (v < 0.0 || u + v > 1.0) {
                    continue;
                }
                let t = dot(e2, q) * id;
                if (t <= 1e-5 || t >= h.t) {
                    continue;
                }
                if (p.flags.x > 0.5 && dot(p.clip.xyz, o + d * t) - p.clip.w > 0.0) {
                    continue;
                }
                if (shadow) {
                    if (attrs[ti * 8u + 1u].w < 0.999 || is_emissive(ti)) {
                        continue;
                    }
                    h.ok = true;
                    h.t = t;
                    return h;
                }
                h.ok = true;
                h.t = t;
                h.tri = ti;
                h.u = u;
                h.v = v;
            }
        } else if (sp + 2 <= 48) {
            let l = first;
            let r = first + 1u;
            let la = nodes[l * 2u];
            let lb = nodes[l * 2u + 1u];
            let ra = nodes[r * 2u];
            let rb = nodes[r * 2u + 1u];
            let dl = box_t(o, inv, la.xyz, lb.xyz, h.t);
            let dr = box_t(o, inv, ra.xyz, rb.xyz, h.t);
            if (dl < dr) {
                stack[sp] = r;
                stack[sp + 1] = l;
            } else {
                stack[sp] = l;
                stack[sp + 1] = r;
            }
            sp = sp + 2;
        }
    }
    return h;
}

fn sky(d: vec3<f32>) -> vec3<f32> {
    var c: vec3<f32>;
    if (p.sky.z > 0.5) {
        let phi = atan2(d.y, d.x) + p.sky.y;
        let theta = acos(clamp(d.z, -1.0, 1.0));
        let uv = vec2<f32>(fract(0.5 - phi / (2.0 * PI)), theta / PI);
        c = srgb_lin(textureSampleLevel(env, smp, uv, 0.0).rgb);
    } else if (d.z >= 0.0) {
        c = srgb_lin(mix(vec3<f32>(0.965, 0.970, 0.975), vec3<f32>(0.80, 0.85, 0.90), sqrt(clamp(d.z, 0.0, 1.0))));
    } else {
        c = srgb_lin(mix(vec3<f32>(0.80, 0.80, 0.775), vec3<f32>(0.74, 0.74, 0.715), sqrt(clamp(-d.z, 0.0, 1.0))));
    }
    return c * p.sky.x;
}

struct Surf {
    p: vec3<f32>,
    n: vec3<f32>,
    ng: vec3<f32>,
    albedo: vec3<f32>,
    rough: f32,
    metal: f32,
    alpha: f32,
    emissive: bool,
};

fn tex_at(layer: f32, uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(tex, smp, uv, i32(layer + 0.5), 0.0);
}

fn pick(c: vec4<f32>, ch: f32) -> f32 {
    if (ch < 0.5) { return c.r; }
    if (ch < 1.5) { return c.g; }
    if (ch < 2.5) { return c.b; }
    return c.a;
}

fn surface(h: Hit, d: vec3<f32>) -> Surf {
    let ti = h.tri;
    let p0 = tris[ti * 3u].xyz;
    let e1 = tris[ti * 3u + 1u].xyz;
    let e2 = tris[ti * 3u + 2u].xyz;
    let a0 = attrs[ti * 8u];
    let a1 = attrs[ti * 8u + 1u];
    let a2 = attrs[ti * 8u + 2u];
    let a3 = attrs[ti * 8u + 3u];
    let a4 = attrs[ti * 8u + 4u];
    let a5 = attrs[ti * 8u + 5u];
    let w = 1.0 - h.u - h.v;
    var s: Surf;
    s.p = p0 + e1 * h.u + e2 * h.v;
    var ng = normalize(cross(e1, e2));
    let front = dot(d, ng) < 0.0;
    var n = a0.xyz * w + a1.xyz * h.u + a2.xyz * h.v;
    n = select(ng, normalize(n), dot(n, n) > 1e-12);
    if (!front) {
        ng = -ng;
        n = -n;
    }
    if (dot(n, ng) < 0.0) {
        n = ng;
    }
    let q = max(a4.z * w + a4.w * h.u + a5.w * h.v, 1e-6);
    let uv = (a3.xy * w + a3.zw * h.u + a4.xy * h.v) / q;
    var base = select(attrs[ti * 8u + 7u].rgb, attrs[ti * 8u + 6u].rgb, front);
    var albedo = srgb_lin(base);
    var rough = 0.85;
    var metal = 0.0;
    s.emissive = false;
    let sl = slot_of(a0.w, front);
    if (sl >= 0) {
        let m0 = mats[u32(sl) * 4u];
        let m1 = mats[u32(sl) * 4u + 1u];
        let m2 = mats[u32(sl) * 4u + 2u];
        let m3 = mats[u32(sl) * 4u + 3u];
        rough = m1.z;
        metal = m1.y;
        s.emissive = m3.y > 0.0;
        if (m0.x > -0.5) {
            albedo = albedo * srgb_lin(tex_at(m0.x, uv).rgb);
        }
        if (m0.z > -0.5) {
            var r = pick(tex_at(m0.z, uv), m2.x);
            if (m2.w > 0.5) {
                r = 1.0 - r;
            }
            rough = rough * r;
        }
        if (m0.w > -0.5) {
            metal = metal * pick(tex_at(m0.w, uv), m2.y);
        }
        if (m1.x > -0.5) {
            albedo = albedo * (1.0 + (pick(tex_at(m1.x, uv), m2.z) - 1.0) * clamp(m3.x, 0.0, 1.0));
        }
        if (m0.y > -0.5) {
            let tn = tex_at(m0.y, uv).xyz * 2.0 - 1.0;
            let tt0 = a5.xyz - n * dot(n, a5.xyz);
            if (dot(tt0, tt0) > 1e-8) {
                let tt = normalize(tt0);
                let bb = cross(n, tt) * select(-1.0, 1.0, a2.w >= 0.0);
                let nn = normalize(tt * (tn.x * m1.w) - bb * (tn.y * m1.w) + n * tn.z);
                if (dot(nn, ng) > 0.05) {
                    n = nn;
                }
            }
        }
    }
    s.n = n;
    s.ng = ng;
    s.albedo = clamp(albedo, vec3<f32>(0.0), vec3<f32>(1.0));
    s.rough = clamp(rough, 0.03, 1.0);
    s.metal = clamp(metal, 0.0, 1.0);
    s.alpha = a1.w;
    return s;
}

fn basis_t(n: vec3<f32>) -> mat2x3<f32> {
    let sg = select(-1.0, 1.0, n.z >= 0.0);
    let a = -1.0 / (sg + n.z);
    let b = n.x * n.y * a;
    return mat2x3<f32>(vec3<f32>(1.0 + sg * n.x * n.x * a, sg * b, -sg * n.x), vec3<f32>(b, sg + n.y * n.y * a, -n.y));
}

fn ggx_d(ndh: f32, a2: f32) -> f32 {
    let d = ndh * ndh * (a2 - 1.0) + 1.0;
    return a2 / max(PI * d * d, 1e-12);
}

fn brdf(s: Surf, v: vec3<f32>, l: vec3<f32>) -> vec3<f32> {
    let ndl = dot(s.n, l);
    if (ndl <= 0.0) {
        return vec3<f32>(0.0);
    }
    let ndv = max(dot(s.n, v), 1e-4);
    let h = normalize(l + v);
    let a = max(s.rough * s.rough, 0.002);
    let a2 = a * a;
    let f0 = mix(vec3<f32>(0.04), s.albedo, s.metal);
    let f = f0 + (1.0 - f0) * pow(1.0 - max(dot(v, h), 0.0), 5.0);
    let k = a * 0.5;
    let g = (ndv / (ndv * (1.0 - k) + k)) * (ndl / (ndl * (1.0 - k) + k));
    let spec = f * (ggx_d(max(dot(s.n, h), 0.0), a2) * g / max(4.0 * ndv * ndl, 1e-6));
    let diff = s.albedo * (1.0 - s.metal) * (1.0 - f) / PI;
    return (diff + spec) * ndl;
}

fn spec_prob(s: Surf) -> f32 {
    return clamp(0.12 + 0.88 * s.metal, 0.1, 0.95) * (1.0 - 0.5 * s.rough * (1.0 - s.metal));
}

struct Bsdf {
    dir: vec3<f32>,
    w: vec3<f32>,
    spec: bool,
    ok: bool,
};

fn sample_brdf(s: Surf, v: vec3<f32>) -> Bsdf {
    var r: Bsdf;
    r.ok = false;
    let tb = basis_t(s.n);
    let ps = spec_prob(s);
    let a = max(s.rough * s.rough, 0.002);
    let a2 = a * a;
    let u1 = rnd();
    let u2 = rnd();
    var l: vec3<f32>;
    if (rnd() < ps) {
        let phi = 2.0 * PI * u1;
        let ct = sqrt((1.0 - u2) / (1.0 + (a2 - 1.0) * u2));
        let st = sqrt(max(1.0 - ct * ct, 0.0));
        let h = tb[0] * (st * cos(phi)) + tb[1] * (st * sin(phi)) + s.n * ct;
        l = 2.0 * dot(v, h) * h - v;
    } else {
        let rr = sqrt(u1);
        let phi = 2.0 * PI * u2;
        l = tb[0] * (rr * cos(phi)) + tb[1] * (rr * sin(phi)) + s.n * sqrt(max(1.0 - u1, 0.0));
    }
    let ndl = dot(s.n, l);
    if (ndl <= 0.0 || dot(s.ng, l) <= 0.0) {
        return r;
    }
    let h = normalize(l + v);
    let pdf_s = ggx_d(max(dot(s.n, h), 0.0), a2) * max(dot(s.n, h), 0.0) / max(4.0 * abs(dot(v, h)), 1e-6);
    let pdf = ps * pdf_s + (1.0 - ps) * ndl / PI;
    if (pdf <= 1e-8) {
        return r;
    }
    r.dir = l;
    r.w = brdf(s, v, l) / pdf;
    r.spec = s.rough < 0.25;
    r.ok = true;
    return r;
}

fn direct(s: Surf, v: vec3<f32>) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    let o = s.p + s.ng * 2e-4;
    let count = p.size.w;
    for (var k = 0u; k < count; k = k + 1u) {
        let L0 = lights[k * 5u];
        let L1 = lights[k * 5u + 1u];
        let L2 = lights[k * 5u + 2u];
        let L3 = lights[k * 5u + 3u];
        let L4 = lights[k * 5u + 4u];
        let flux = L2.rgb;
        if (L0.w > 1.5) {
            // panel
            let u = L3.xyz;
            let w = L4.xyz;
            let area = 4.0 * length(u) * length(w);
            if (area <= 1e-6) {
                continue;
            }
            let q = L0.xyz + u * (2.0 * rnd() - 1.0) + w * (2.0 * rnd() - 1.0);
            let lv = q - o;
            let d2 = max(dot(lv, lv), 1e-6);
            let wi = lv / sqrt(d2);
            let cl = dot(-wi, L1.xyz);
            if (cl <= 0.0) {
                continue;
            }
            let f = brdf(s, v, wi);
            if (max(f.x, max(f.y, f.z)) <= 0.0) {
                continue;
            }
            if (intersect(o, wi, sqrt(d2) - 1e-3, true).ok) {
                continue;
            }
            sum = sum + f * (flux / (PI * area)) * (cl * area / d2);
        } else {
            var q = L0.xyz;
            let rad = L3.w;
            if (rad > 0.0) {
                let z = 2.0 * rnd() - 1.0;
                let ph = 2.0 * PI * rnd();
                let rr = sqrt(max(1.0 - z * z, 0.0));
                q = q + vec3<f32>(rr * cos(ph), rr * sin(ph), z) * rad;
            }
            let lv = q - o;
            let d2 = max(dot(lv, lv), 1e-6);
            let wi = lv / sqrt(d2);
            var inten: vec3<f32>;
            if (L0.w > 0.5) {
                let c = dot(-wi, L1.xyz);
                let t = clamp((c - L1.w) / max(L2.w - L1.w, 1e-4), 0.0, 1.0);
                inten = flux / (2.0 * PI * max(1.0 - L1.w, 0.02)) * (t * t * (3.0 - 2.0 * t));
            } else {
                inten = flux / (4.0 * PI);
            }
            if (max(inten.x, max(inten.y, inten.z)) <= 0.0) {
                continue;
            }
            let f = brdf(s, v, wi);
            if (max(f.x, max(f.y, f.z)) <= 0.0) {
                continue;
            }
            if (intersect(o, wi, sqrt(d2) - max(rad, 1e-3), true).ok) {
                continue;
            }
            sum = sum + f * inten / d2;
        }
    }
    if (p.sun.w > 0.0) {
        let tb = basis_t(p.sun.xyz);
        let r = 0.0047 * sqrt(rnd());
        let ph = 2.0 * PI * rnd();
        let wi = normalize(p.sun.xyz + tb[0] * (r * cos(ph)) + tb[1] * (r * sin(ph)));
        let f = brdf(s, v, wi);
        if (max(f.x, max(f.y, f.z)) > 0.0 && !intersect(o, wi, 1e7, true).ok) {
            sum = sum + f * p.sun.w;
        }
    }
    return sum;
}

fn oct(n: vec3<f32>) -> vec2<f32> {
    let m = n / (abs(n.x) + abs(n.y) + abs(n.z));
    if (m.z >= 0.0) {
        return m.xy;
    }
    return (1.0 - abs(m.yx)) * select(vec2<f32>(-1.0), vec2<f32>(1.0), m.xy >= vec2<f32>(0.0));
}

@compute @workgroup_size(8, 8, 1)
fn trace(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= p.chunk.z || gid.y >= p.chunk.w) {
        return;
    }
    let x = p.chunk.x + gid.x;
    let y = p.chunk.y + gid.y;
    if (x >= p.size.x || y >= p.size.y) {
        return;
    }
    let bi = (y - p.tile.y) * p.tile.z + (x - p.tile.x);
    seed = pcg(x * 1973u + y * 9277u + p.size.z * 26699u) ^ pcg(p.size.z + 0x9e3779b9u);
    // camera ray
    let fx = (f32(x) + rnd()) / f32(p.size.x) * 2.0 - 1.0;
    let fy = 1.0 - (f32(y) + rnd()) / f32(p.size.y) * 2.0;
    var o = p.eye.xyz;
    var d: vec3<f32>;
    if (p.cam.z > 0.5) {
        o = p.eye.xyz + p.right.xyz * (fx * p.cam.w * p.cam.y) + p.up.xyz * (fy * p.cam.w) - p.fwd.xyz * 1000.0;
        d = p.fwd.xyz;
    } else {
        d = normalize(p.fwd.xyz + p.right.xyz * (fx * p.cam.x * p.cam.y) + p.up.xyz * (fy * p.cam.x));
    }
    var l = vec3<f32>(0.0);
    var thr = vec3<f32>(1.0);
    var specular = true;
    var alb = vec3<f32>(1.0);
    var nor = vec3<f32>(0.0, 0.0, 1.0);
    var depth = 1e9;
    let clampv = p.sky.w;
    for (var bounce = 0; bounce < 10; bounce = bounce + 1) {
        let h = intersect(o, d, 1e7, false);
        if (!h.ok) {
            var sk = sky(d);
            if (bounce > 1) {
                sk = min(sk, vec3<f32>(clampv));
            }
            l = l + thr * sk;
            if (bounce == 0) {
                alb = select(vec3<f32>(1.0), normalize(sk), dot(sk, sk) > 1e-12);
            }
            break;
        }
        let s = surface(h, d);
        if (bounce == 0) {
            alb = select(max(s.albedo, vec3<f32>(0.02)), vec3<f32>(1.0), s.emissive);
            nor = s.n;
            depth = h.t;
        }
        if (s.emissive) {
            if (specular) {
                l = l + thr * s.albedo * p.flags.z;
            }
            break;
        }
        let wo = -d;
        if (s.alpha < 0.999) {
            let c = abs(dot(wo, s.n));
            let f = 0.04 + 0.96 * pow(1.0 - c, 5.0);
            if (rnd() < f) {
                d = normalize(2.0 * dot(wo, s.n) * s.n - wo);
                o = s.p + s.ng * 2e-4;
            } else {
                thr = thr * mix(vec3<f32>(1.0), s.albedo, clamp(s.alpha, 0.0, 1.0) * 0.6);
                o = s.p + d * 2e-4;
            }
            continue;
        }
        var dl = direct(s, wo);
        if (bounce > 0) {
            dl = min(dl, vec3<f32>(clampv));
        }
        l = l + thr * dl;
        let b = sample_brdf(s, wo);
        if (!b.ok) {
            break;
        }
        thr = thr * b.w;
        specular = b.spec;
        o = s.p + s.ng * 2e-4;
        d = b.dir;
        if (bounce >= 3) {
            let pr = clamp(max(thr.x, max(thr.y, thr.z)), 0.05, 0.95);
            if (rnd() > pr) {
                break;
            }
            thr = thr / pr;
        }
    }
    let ok = l.x == l.x && l.y == l.y && l.z == l.z && abs(l.x) < 1e20 && abs(l.y) < 1e20 && abs(l.z) < 1e20;
    if (p.size.z == 0u) {
        accum[bi] = vec4<f32>(select(vec3<f32>(0.0), l, ok), 0.0);
        aux[bi] = vec4<u32>(pack4x8unorm(vec4<f32>(alb, 0.0)), pack2x16snorm(oct(normalize(nor))), bitcast<u32>(depth), 0u);
    } else {
        let prev = accum[bi];
        // a broken sample is replaced by the running mean
        accum[bi] = vec4<f32>(prev.xyz + select(prev.xyz / f32(p.size.z), l, ok), 0.0);
    }
}
"#;

/// The app's GPU device (shared with the viewport).
#[derive(Clone)]
pub struct GpuCtx {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

/// Material textures as one array (each layer resampled to a square) and the
/// environment panorama, both RGBA8 in sRGB; the shader converts to linear.
fn upload_textures(gpu: &GpuCtx, doc: &crate::doc::Document) -> (wgpu::TextureView, wgpu::TextureView, wgpu::Sampler) {
    let d = &gpu.device;
    let list = &doc.textures[..doc.textures.len().min(crate::tex::MAX_LAYERS)];
    let side: u32 = if list.len() <= 24 { 1024 } else { 512 };
    let layers = list.len().max(1) as u32;
    let mk = |label, w: u32, h: u32, layers: u32| {
        d.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: layers },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    };
    let put = |t: &wgpu::Texture, z: u32, img: &image::RgbaImage| {
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: t, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: 0, z }, aspect: wgpu::TextureAspect::All },
            img.as_raw(),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * img.width()), rows_per_image: Some(img.height()) },
            wgpu::Extent3d { width: img.width(), height: img.height(), depth_or_array_layers: 1 },
        );
    };
    let arr = mk("pt-textures", side, side, layers);
    for (k, t) in list.iter().enumerate() {
        let img = match t.decode() {
            Some(i) if i.width() == side && i.height() == side => i,
            Some(i) => image::imageops::resize(&i, side, side, image::imageops::FilterType::Triangle),
            None => image::RgbaImage::from_pixel(side, side, image::Rgba([255, 255, 255, 255])),
        };
        put(&arr, k as u32, &img);
    }
    let env_img = doc.env.image.and_then(|i| doc.env_images.get(i)).and_then(|t| t.decode()).map(|i| {
        let w = i.width().min(4096);
        image::imageops::resize(&i, w, (w / 2).max(1), image::imageops::FilterType::Triangle)
    });
    let env_img = env_img.unwrap_or_else(|| image::RgbaImage::from_pixel(2, 1, image::Rgba([200, 210, 220, 255])));
    let env = mk("pt-env", env_img.width(), env_img.height(), 1);
    put(&env, 0, &env_img);
    let sampler = d.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("pt-sampler"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    (
        arr.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() }),
        env.create_view(&wgpu::TextureViewDescriptor::default()),
        sampler,
    )
}

fn f4(v: Vec3, w: f32) -> [f32; 4] {
    [v.x, v.y, v.z, w]
}

fn oct_decode(e: [f32; 2]) -> Vec3 {
    let mut n = Vec3::new(e[0], e[1], 1.0 - e[0].abs() - e[1].abs());
    if n.z < 0.0 {
        let (x, y) = (n.x, n.y);
        n.x = (1.0 - y.abs()) * if x >= 0.0 { 1.0 } else { -1.0 };
        n.y = (1.0 - x.abs()) * if y >= 0.0 { 1.0 } else { -1.0 };
    }
    n.normalize_or(Vec3::Z)
}

struct Gpu {
    pipe: wgpu::ComputePipeline,
    bgl: wgpu::BindGroupLayout,
    bufs: [wgpu::Buffer; 6],
}

fn storage(device: &wgpu::Device, label: &str, data: &[u8]) -> wgpu::Buffer {
    let data: &[u8] = if data.is_empty() { &[0u8; 16] } else { data };
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: data, usage: wgpu::BufferUsages::STORAGE })
}

fn upload(gpu: &GpuCtx, sc: &Scene) -> Gpu {
    let d = &gpu.device;
    let tris: Vec<[f32; 4]> = sc.tris.iter().flat_map(|t| [f4(t.p0, 0.0), f4(t.e1, 0.0), f4(t.e2, 0.0)]).collect();
    let attrs: Vec<[f32; 4]> = sc
        .attrs
        .iter()
        .flat_map(|a| {
            [
                f4(a.n[0], a.layer),
                f4(a.n[1], a.col[3]),
                f4(a.n[2], a.tsign),
                [a.uv[0].x, a.uv[0].y, a.uv[1].x, a.uv[1].y],
                [a.uv[2].x, a.uv[2].y, a.q[0], a.q[1]],
                f4(a.tan, a.q[2]),
                [a.col[0], a.col[1], a.col[2], 0.0],
                [a.back[0], a.back[1], a.back[2], 0.0],
            ]
        })
        .collect();
    let nodes: Vec<[f32; 4]> = sc.bvh.nodes.iter().flat_map(|n| [f4(n.min, f32::from_bits(n.first)), f4(n.max, f32::from_bits(n.count))]).collect();
    let mats: Vec<[f32; 4]> = sc.mats.iter().flat_map(|m| [[m[0], m[1], m[2], m[3]], [m[4], m[5], m[6], m[7]], [m[8], m[9], m[10], m[11]], [m[12], m[13], m[14], m[15]]]).collect();
    let lights: Vec<[f32; 4]> = sc
        .lights
        .iter()
        .flat_map(|l| {
            let k = match l.kind {
                crate::lights::LightKind::Point => 0.0,
                crate::lights::LightKind::Spot => 1.0,
                crate::lights::LightKind::Panel => 2.0,
            };
            [f4(l.pos.as_vec3(), k), f4(l.dir.as_vec3(), l.cos_outer), [l.flux[0], l.flux[1], l.flux[2], l.cos_inner], f4(l.u.as_vec3(), l.radius), f4(l.v.as_vec3(), 0.0)]
        })
        .collect();
    let bufs = [
        storage(d, "pt-tris", bytemuck::cast_slice(&tris)),
        storage(d, "pt-attrs", bytemuck::cast_slice(&attrs)),
        storage(d, "pt-nodes", bytemuck::cast_slice(&nodes)),
        storage(d, "pt-idx", bytemuck::cast_slice(&sc.bvh.idx)),
        storage(d, "pt-mats", bytemuck::cast_slice(&mats)),
        storage(d, "pt-lights", bytemuck::cast_slice(&lights)),
    ];
    let module = d.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("homeup-pathtrace"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let ro = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    };
    let rw = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    };
    let bgl = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("pt-bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: None },
                count: None,
            },
            ro(1),
            ro(2),
            ro(3),
            ro(4),
            ro(5),
            ro(6),
            rw(7),
            rw(8),
            wgpu::BindGroupLayoutEntry {
                binding: 9,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2Array, multisampled: false },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 10,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                count: None,
            },
            wgpu::BindGroupLayoutEntry { binding: 11, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
        ],
    });
    let layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("pt-layout"), bind_group_layouts: &[Some(&bgl)], immediate_size: 0 });
    let pipe = d.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some("pt-pipe"), layout: Some(&layout), module: &module, entry_point: Some("trace"), compilation_options: Default::default(), cache: None });
    Gpu { pipe, bgl, bufs }
}

const PARAM_STRIDE: u64 = 256;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    eye: [f32; 4],
    fwd: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
    cam: [f32; 4],
    size: [u32; 4],
    tile: [u32; 4],
    chunk: [u32; 4],
    sun: [f32; 4],
    sky: [f32; 4],
    clip: [f32; 4],
    flags: [f32; 4],
    _pad: [f32; 16],
}

fn read_buffer(gpu: &GpuCtx, src: &wgpu::Buffer, size: u64) -> Option<Vec<u8>> {
    let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("pt-read"), size, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("pt-copy") });
    enc.copy_buffer_to_buffer(src, 0, &staging, 0, size);
    gpu.queue.submit([enc.finish()]);
    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    rx.recv().ok()?.ok()?;
    let v = slice.get_mapped_range().ok()?.to_vec();
    staging.unmap();
    Some(v)
}

/// The GPU render loop (runs on a worker thread; mirrors pathtrace::run).
pub fn run_gpu(job: Arc<Job>, doc: crate::doc::Document, cam: crate::camera::Camera, ctx: egui::Context, gpu: GpuCtx) {
    let (w, h) = (job.size[0] as usize, job.size[1] as usize);
    *job.status.lock().unwrap() = "Preparando a cena para a GPU…".into();
    ctx.request_repaint();
    let sc = crate::pathtrace::build_scene(&doc, &cam, job.size, false);
    let (tex_view, env_view, sampler) = upload_textures(&gpu, &doc);
    drop(doc);
    let g = upload(&gpu, &sc);
    let info = format!("GPU · {} triângulos, {} luzes", sc.tris.len(), sc.lights.len());
    *job.status.lock().unwrap() = info.clone();
    // tiles that fit the storage-buffer limit (16 bytes per pixel and buffer)
    let limits = gpu.device.limits();
    let max_px = ((limits.max_storage_buffer_binding_size as u64).min(limits.max_buffer_size).min(1 << 30) / 16).max(65536) as usize;
    // HOMEUP_PT_TILE_ROWS forces smaller tiles (testing)
    let tile_rows = std::env::var("HOMEUP_PT_TILE_ROWS").ok().and_then(|v| v.parse().ok()).unwrap_or(max_px / w).clamp(1, h);
    let mut b = Buffers { acc: vec![Vec3::ZERO; w * h], alb: vec![Vec3::ONE; w * h], nor: vec![Vec3::Z; w * h], dep: vec![1e9; w * h], spp: 0, auto: 1.0 };
    let base = Params {
        eye: f4(sc.cam.eye, 0.0),
        fwd: f4(sc.cam.fwd, 0.0),
        right: f4(sc.cam.right, 0.0),
        up: f4(sc.cam.up, 0.0),
        cam: [sc.cam.tan_half, w as f32 / h as f32, sc.cam.ortho as u8 as f32, sc.cam.half_h],
        size: [w as u32, h as u32, 0, sc.lights.len() as u32],
        tile: [0; 4],
        chunk: [0; 4],
        sun: sc.sun.map(|(d, e)| f4(d, e)).unwrap_or([0.0; 4]),
        sky: [sc.sky_scale, sc.env_rot, sc.env.is_some() as u8 as f32, f32::MAX],
        clip: sc.clip.map(|(n, d)| f4(n, d)).unwrap_or([0.0; 4]),
        flags: [sc.clip.is_some() as u8 as f32, sc.mats.len() as f32, LAMP_NITS, 0.0],
        _pad: [0.0; 16],
    };
    let p0 = base;
    let tiles: Vec<(usize, usize)> = (0..h).step_by(tile_rows).map(|y0| (y0, tile_rows.min(h - y0))).collect();
    let ntiles = tiles.len();
    let mut last_preview = std::time::Instant::now();
    let mut shown_version = u64::MAX;
    for (ti, (y0, th)) in tiles.into_iter().enumerate() {
        if job.cancel.load(Ordering::Relaxed) {
            return;
        }
        let px = w * th;
        let accum = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("pt-accum"), size: (px * 16) as u64, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
        let aux = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("pt-aux"), size: (px * 16) as u64, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
        // chunks of ≤ 256×256 pixels per dispatch
        let mut chunks = Vec::new();
        for cy in (y0..y0 + th).step_by(256) {
            for cx in (0..w).step_by(256) {
                chunks.push([cx as u32, cy as u32, 256.min(w - cx) as u32, 256.min(y0 + th - cy) as u32]);
            }
        }
        let ubuf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("pt-params"), size: PARAM_STRIDE * chunks.len() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pt-bind"),
            layout: &g.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &ubuf, offset: 0, size: std::num::NonZeroU64::new(std::mem::size_of::<Params>() as u64) }) },
                wgpu::BindGroupEntry { binding: 1, resource: g.bufs[0].as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: g.bufs[1].as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: g.bufs[2].as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: g.bufs[3].as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: g.bufs[4].as_entire_binding() },
                wgpu::BindGroupEntry { binding: 6, resource: g.bufs[5].as_entire_binding() },
                wgpu::BindGroupEntry { binding: 7, resource: accum.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 8, resource: aux.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::TextureView(&tex_view) },
                wgpu::BindGroupEntry { binding: 10, resource: wgpu::BindingResource::TextureView(&env_view) },
                wgpu::BindGroupEntry { binding: 11, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });
        let mut spp = 0u32;
        let read_back = |b: &mut Buffers, spp: u32| {
            if let Some(acc) = read_buffer(&gpu, &accum, (px * 16) as u64) {
                let a: &[[f32; 4]] = bytemuck::cast_slice(&acc);
                for (k, v) in a.iter().enumerate() {
                    // CPU buffers keep sum / spp of the whole image; scale this tile to the image's sample count
                    b.acc[y0 * w + k] = Vec3::new(v[0], v[1], v[2]) * (b.spp.max(1) as f32 / spp.max(1) as f32);
                }
            }
            if spp == 1 {
                if let Some(ax) = read_buffer(&gpu, &aux, (px * 16) as u64) {
                    let a: &[[u32; 4]] = bytemuck::cast_slice(&ax);
                    for (k, v) in a.iter().enumerate() {
                        let c = v[0].to_le_bytes();
                        b.alb[y0 * w + k] = Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32) / 255.0;
                        let nx = (v[1] & 0xffff) as u16 as i16 as f32 / 32767.0;
                        let ny = (v[1] >> 16) as u16 as i16 as f32 / 32767.0;
                        b.nor[y0 * w + k] = oct_decode([nx, ny]);
                        b.dep[y0 * w + k] = f32::from_bits(v[2]);
                    }
                }
            }
        };
        while spp < job.target {
            if job.cancel.load(Ordering::Relaxed) {
                return;
            }
            if job.finished.load(Ordering::Relaxed) {
                break;
            }
            // parameters for every chunk of this pass
            let mut data = vec![0u8; (PARAM_STRIDE as usize) * chunks.len()];
            for (k, c) in chunks.iter().enumerate() {
                let mut pp = p0;
                pp.size[2] = spp;
                pp.tile = [0, y0 as u32, w as u32, th as u32];
                pp.chunk = *c;
                pp.sky[3] = if spp == 0 && b.spp == 0 { f32::MAX } else { 30.0 / b.auto.max(1e-9) };
                let bytes = bytemuck::bytes_of(&pp);
                data[k * PARAM_STRIDE as usize..k * PARAM_STRIDE as usize + bytes.len()].copy_from_slice(bytes);
            }
            gpu.queue.write_buffer(&ubuf, 0, &data);
            // a few chunks per submission keeps every GPU command short
            for group in (0..chunks.len()).collect::<Vec<_>>().chunks(8) {
                let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("pt-pass") });
                {
                    let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("pt"), timestamp_writes: None });
                    pass.set_pipeline(&g.pipe);
                    for &k in group {
                        pass.set_bind_group(0, &bind, &[(k as u64 * PARAM_STRIDE) as u32]);
                        let c = chunks[k];
                        pass.dispatch_workgroups(c[2].div_ceil(8), c[3].div_ceil(8), 1);
                    }
                }
                gpu.queue.submit([enc.finish()]);
                let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            }
            spp += 1;
            // image-wide sample count: the first tile defines progress
            if ti == 0 {
                b.spp = spp;
            }
            let total = (ti as u64 * job.target as u64 + spp as u64) as f64 / (ntiles as u64 * job.target as u64) as f64;
            job.spp.store(((total * job.target as f64).round() as u32).min(job.target), Ordering::Relaxed);
            job.elapsed_ms.store(job.started.elapsed().as_millis() as u64, Ordering::Relaxed);
            let want_preview = spp == 1 || spp == 4 || spp == 16 || spp == job.target || last_preview.elapsed().as_millis() > 1200 || job.version.load(Ordering::Relaxed) != shown_version;
            if want_preview {
                read_back(&mut b, spp);
                if ti == 0 && (spp == 1 || spp == 4 || spp == 16) {
                    auto_exposure(&mut b, px);
                }
                let img = image_from(&job, &b, w, h, Some(1600));
                let v = job.version.load(Ordering::Relaxed);
                *job.preview.lock().unwrap() = Some((img, (ti as u64 * 100000 + spp as u64) * 1000 + v));
                shown_version = v;
                last_preview = std::time::Instant::now();
                ctx.request_repaint();
            }
            if let Some(path) = job.save.lock().unwrap().take() {
                save(&job, &b, w, h, &path, &ctx);
            }
        }
        read_back(&mut b, spp.max(1));
        // exposure over everything rendered so far
        auto_exposure(&mut b, (y0 + th) * w);
    }
    job.spp.store(job.target, Ordering::Relaxed);
    job.finished.store(true, Ordering::Relaxed);
    job.elapsed_ms.store(job.started.elapsed().as_millis() as u64, Ordering::Relaxed);
    *job.status.lock().unwrap() = info;
    // serve exposure / denoise changes and saves until the window closes
    let mut shown = u64::MAX;
    while !job.cancel.load(Ordering::Relaxed) {
        let v = job.version.load(Ordering::Relaxed);
        if v != shown {
            let full = image_from(&job, &b, w, h, None);
            let img = if w > 1600 { image::imageops::thumbnail(&full, 1600, (1600 * h / w) as u32) } else { full };
            *job.preview.lock().unwrap() = Some((img, u64::MAX / 2 + v));
            shown = v;
            ctx.request_repaint();
        }
        if let Some(path) = job.save.lock().unwrap().take() {
            save(&job, &b, w, h, &path, &ctx);
        }
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
}

/// Log-average luminance of the first `px` pixels → exposure scale.
fn auto_exposure(b: &mut Buffers, px: usize) {
    let mut sum = 0.0f64;
    let mut cnt = 0usize;
    for c in b.acc[..px].iter().step_by(7) {
        let lum = (*c / b.spp.max(1) as f32).dot(Vec3::new(0.2126, 0.7152, 0.0722));
        if lum > 1e-6 {
            sum += (lum as f64).ln();
            cnt += 1;
        }
    }
    if cnt > 0 {
        b.auto = 0.18 / ((sum / cnt as f64).exp() as f32).max(1e-9);
    }
}

fn save(job: &Job, b: &Buffers, w: usize, h: usize, path: &std::path::Path, ctx: &egui::Context) {
    *job.status.lock().unwrap() = "Salvando…".into();
    ctx.request_repaint();
    let img = image_from(job, b, w, h, None);
    let res = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg")) {
        image::DynamicImage::ImageRgba8(img).to_rgb8().save(path)
    } else {
        img.save(path)
    };
    *job.status.lock().unwrap() = match res {
        Ok(()) => format!("Salvo: {}", path.display()),
        Err(e) => format!("Erro ao salvar: {e}"),
    };
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    #[test]
    fn gpu_shader_is_valid_wgsl() {
        let m = naga::front::wgsl::parse_str(super::SHADER).expect("parse");
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&m).expect("validate");
    }
}
