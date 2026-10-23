//! Offscreen wgpu renderer for the 3D viewport. The result is registered as an
//! egui texture and painted as an image underneath the 2D overlay.
//!
//! Passes: shadow map → main (background, faces, ground shadow, edges, back
//! edges, glass) → post (screen-space ambient occlusion + profile lines,
//! multiplied onto the resolved image).
//! Faces use a physically based model (GGX specular, Schlick Fresnel,
//! environment reflections) with optional normal/roughness/metal/AO maps.

use eframe::egui;
use eframe::egui_wgpu::RenderState;
use eframe::wgpu;
use eframe::wgpu::util::DeviceExt;
use glam::Mat4;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub nrm: [f32; 3],
    pub col: [f32; 4],
    pub back: [f32; 4],
    pub uv: [f32; 2],
    /// Material slot (-1 = none; +1000 = textured on both sides).
    pub layer: f32,
    /// Tangent (xyz) and bitangent sign (w), for normal maps.
    pub tan: [f32; 4],
}

impl Vertex {
    pub fn plain(pos: [f32; 3], nrm: [f32; 3], col: [f32; 4], back: [f32; 4]) -> Vertex {
        Vertex { pos, nrm, col, back, uv: [0.0; 2], layer: -1.0, tan: [1.0, 0.0, 0.0, 1.0] }
    }
}

/// GPU material record (4 x vec4): layers (color, normal, rough, metal),
/// (ao layer, metal, rough, normal scale), (rough ch, metal ch, ao ch, invert), (ao strength, -, -, -).
pub type GpuMat = [f32; 16];

#[derive(Default)]
pub struct Scene3D {
    pub tris: Vec<Vertex>,
    pub glass: Vec<Vertex>,
    pub lines: Vec<Vertex>,
    /// Opaque triangles that do not cast shadows.
    pub nocast: Vec<Vertex>,
    /// Wide edges (screen-space quads, see `vs_wide`).
    pub wide: Vec<Vertex>,
    /// Free edges that cast shadows (Shadows > From edges).
    pub shadow_lines: Vec<Vertex>,
    pub mats: Vec<GpuMat>,
    /// Imported hatch patterns (see `pats` in the shader).
    pub pats: Vec<[f32; 4]>,
    /// Lights: header (count) + 4 vec4 per light (see lights.rs).
    pub lights: Vec<[f32; 4]>,
}

/// 8-bit RGB to RGBA floats. The offscreen target is non-sRGB because egui
/// samples it as gamma-space color.
pub fn srgb(r: u8, g: u8, b: u8) -> [f32; 4] {
    let f = |c: u8| (c as f32) / 255.0;
    [f(r), f(g), f(b), 1.0]
}

/// Per-frame rendering parameters.
pub struct FrameParams {
    pub vp: Mat4,
    pub inv_vp: Mat4,
    pub clip: Option<[f32; 4]>,
    pub sky: bool,
    pub bg: [f32; 3],
    /// Direction toward the sun when shadows are on.
    pub sun: Option<[f32; 3]>,
    pub light: f32,
    pub dark: f32,
    pub fog: Option<(f32, f32)>,
    pub eye: [f32; 3],
    /// Light view-projection for the shadow map.
    pub light_vp: Mat4,
    /// Ground square (center xy, half size) receiving shadows, when enabled.
    pub ground: Option<([f32; 2], f32)>,
    /// Shadows on faces (otherwise only the ground receives them).
    pub shadows_on_faces: bool,
    pub photoreal: bool,
    pub env_image: bool,
    pub env_rotation: f32,
    pub sky_exposure: f32,
    pub refl_exposure: f32,
    pub skydome: bool,
    pub reflections: bool,
    /// Screen-space ambient occlusion strength (0 = off) and radius (m).
    pub ao: (f32, f32),
    /// Profile line thickness (px, 0 = off).
    pub profiles: f32,
    pub back_edges: bool,
    pub px_scale: f32,
    /// Depth cue max px, endpoints px, jitter px, depth range (m).
    pub edge: [f32; 4],
}

struct BgLayer {
    sig: u64,
    _tex: (wgpu::Texture, wgpu::TextureView),
    ubuf: wgpu::Buffer,
    bind: wgpu::BindGroup,
}

pub(crate) const SHADER: &str = r#"
struct U {
    vp: mat4x4<f32>,
    inv_vp: mat4x4<f32>,
    clip: vec4<f32>,
    flags: vec4<f32>,   // x: clip on, y: shadows on, z: sky on, w: fog on
    bg: vec4<f32>,
    sun: vec4<f32>,     // xyz: sun dir, w: shadows on faces
    light: vec4<f32>,   // x: light, y: dark, z: fog start, w: fog end
    eye: vec4<f32>,
    light_vp: mat4x4<f32>,
    env: vec4<f32>,     // x: rotation (rad), y: sky exposure, z: reflection exposure, w: has env image
    flags2: vec4<f32>,  // x: photoreal, y: skydome, z: reflections, w: back edges
    post: vec4<f32>,    // x: ao strength, y: ao radius, z: profile px, w: px scale
    vpsize: vec4<f32>,  // viewport size (physical px)
    edge: vec4<f32>,    // x: depth cue max px, y: endpoints px, z: jitter px, w: depth range (m)
};
struct Mat {
    layers: vec4<f32>,
    p1: vec4<f32>,
    ch: vec4<f32>,
    p2: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var shadow_tex: texture_depth_2d;
@group(0) @binding(2) var shadow_smp: sampler_comparison;
@group(0) @binding(3) var tex: texture_2d_array<f32>;
@group(0) @binding(4) var tex_smp: sampler;
@group(0) @binding(5) var env_tex: texture_2d<f32>;
@group(0) @binding(6) var<storage, read> mats: array<Mat>;
// imported (.PAT) hatch patterns: per pattern 1 header + 8 line families × 3 vec4
@group(0) @binding(7) var<storage, read> pats: array<vec4<f32>>;
@group(0) @binding(8) var<storage, read> lights: array<vec4<f32>>;

const LIGHT_K: f32 = 0.0035;

// Illuminance (lux-like) from the scene lights at p with normal n; also returns
// the strongest light direction in `ldir` for the specular term.
fn lamp_light(p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    let count = i32(lights[0].x);
    var sum = vec3<f32>(0.0);
    for (var k = 0; k < count; k = k + 1) {
        let a = lights[1 + k * 4];
        let b = lights[2 + k * 4];
        let c = lights[3 + k * 4];
        let L = a.xyz - p;
        let d2 = max(dot(L, L), 0.0004);
        let l = L / sqrt(d2);
        let ndl = max(dot(n, l), 0.0);
        if (ndl <= 0.0) {
            continue;
        }
        var inten = vec3<f32>(0.0);
        let cosd = dot(-l, b.xyz);
        if (a.w < 0.5) {
            inten = c.rgb / (4.0 * PI);
        } else if (a.w < 1.5) {
            let s = smoothstep(b.w, c.w, cosd);
            inten = c.rgb / (2.0 * PI * max(1.0 - b.w, 0.02)) * s;
        } else {
            inten = c.rgb / PI * max(cosd, 0.0);
        }
        sum = sum + inten * ndl / d2;
    }
    return sum;
}

// background layers (watermarks behind the model, matched photo): one draw each
struct WmU {
    a: vec4<f32>,   // opacity, mode (0 stretch, 1 tile, 2 positioned, 3 photo fit), scale, -
    b: vec4<f32>,   // image w, h (px), anchor x, y
};
@group(1) @binding(0) var<uniform> wmu: WmU;
@group(1) @binding(1) var wm_tex: texture_2d<f32>;

const PI: f32 = 3.14159265;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) col: vec4<f32>,
    @location(3) back: vec4<f32>,
    @location(4) uv: vec2<f32>,
    @location(5) layer: f32,
    @location(6) tan: vec4<f32>,
};
struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) nrm: vec3<f32>,
    @location(1) col: vec4<f32>,
    @location(2) back: vec4<f32>,
    @location(3) wpos: vec3<f32>,
    @location(4) uv: vec2<f32>,
    @location(5) @interpolate(flat) layer: f32,
    @location(6) tan: vec4<f32>,
};

fn clipped(p: vec3<f32>) -> bool {
    return u.flags.x > 0.5 && dot(u.clip.xyz, p) - u.clip.w > 0.0;
}

// 1 = lit, 0 = in shadow (3x3 PCF)
fn sun_visibility(p: vec3<f32>, bias: f32) -> f32 {
    let lp = u.light_vp * vec4<f32>(p, 1.0);
    let ndc = lp.xyz / lp.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || ndc.z > 1.0) {
        return 1.0;
    }
    let texel = 1.0 / 2048.0;
    var acc = 0.0;
    for (var dx = -1; dx <= 1; dx = dx + 1) {
        for (var dy = -1; dy <= 1; dy = dy + 1) {
            acc = acc + textureSampleCompareLevel(shadow_tex, shadow_smp, uv + vec2<f32>(f32(dx), f32(dy)) * texel, ndc.z - bias);
        }
    }
    return acc / 9.0;
}

fn procedural_sky(d: vec3<f32>) -> vec3<f32> {
    if (d.z >= 0.0) {
        let t = sqrt(clamp(d.z, 0.0, 1.0));
        return mix(vec3<f32>(0.965, 0.970, 0.975), vec3<f32>(0.80, 0.85, 0.90), t);
    }
    let t = sqrt(clamp(-d.z, 0.0, 1.0));
    return mix(vec3<f32>(0.80, 0.80, 0.775), vec3<f32>(0.74, 0.74, 0.715), t);
}

// Environment radiance along a direction (equirectangular image, z up).
fn environment(d: vec3<f32>, lod: f32) -> vec3<f32> {
    if (u.env.w < 0.5) {
        return procedural_sky(d);
    }
    let phi = atan2(d.y, d.x) + u.env.x;
    let theta = acos(clamp(d.z, -1.0, 1.0));
    let uv = vec2<f32>(fract(0.5 - phi / (2.0 * PI)), theta / PI);
    return textureSampleLevel(env_tex, tex_smp, uv, lod).rgb;
}

fn apply_fog(c: vec3<f32>, p: vec3<f32>) -> vec3<f32> {
    if (u.flags.w < 0.5) {
        return c;
    }
    let d = distance(u.eye.xyz, p);
    let t = clamp((d - u.light.z) / max(u.light.w - u.light.z, 0.001), 0.0, 1.0);
    return mix(c, u.bg.rgb, t);
}

fn pick(c: vec4<f32>, ch: f32) -> f32 {
    if (ch < 0.5) { return c.r; }
    if (ch < 1.5) { return c.g; }
    if (ch < 2.5) { return c.b; }
    return c.a;
}

@vertex
fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.clip = u.vp * vec4<f32>(v.pos, 1.0);
    o.nrm = v.nrm;
    o.col = v.col;
    o.back = v.back;
    o.wpos = v.pos;
    o.uv = v.uv;
    o.layer = v.layer;
    o.tan = v.tan;
    return o;
}

fn hline(x: f32, th: f32, aa: f32) -> f32 {
    let d = abs(fract(x + 0.5) - 0.5);
    return 1.0 - smoothstep(th, th + max(aa, 0.0001), d);
}

// Section hatch coverage at pattern coords q (one unit = one repeat); mirrors hatch.rs.
fn hatch(p: f32, q: vec2<f32>, w: f32) -> f32 {
    let s2 = 1.41421356;
    if (p < 0.5) {
        return 0.0;
    }
    if (p < 1.5) {
        return hline((q.x + q.y) * s2, 0.06, w * s2);
    }
    if (p < 2.5) {
        return max(hline((q.x + q.y) * s2, 0.06, w * s2), hline((q.x - q.y) * s2, 0.06, w * s2));
    }
    if (p < 3.5) {
        let row = floor(q.y * 2.0);
        let off = select(0.0, 0.5, fract(row * 0.5) > 0.25);
        return max(hline(q.y * 2.0, 0.05, w * 2.0), hline(q.x + off, 0.025, w));
    }
    if (p < 4.5) {
        let cell = floor(q * 4.0);
        let h = fract(sin(cell.x * 12.9898 + cell.y * 78.233) * 43758.5453);
        let c = fract(q * 4.0) - vec2<f32>(0.5);
        let cx = c.x - (h - 0.5) * 0.5;
        let cy = c.y - fract(h * 7.0) * 0.4 + 0.2;
        let r = sqrt(cx * cx + cy * cy);
        return select(0.0, 1.0 - smoothstep(0.11, 0.11 + w * 4.0, r), h > 0.45);
    }
    if (p < 5.5) {
        let odd = fract((floor(q.x) + floor(q.y)) * 0.5) > 0.25;
        return select(hline(q.y * 3.0, 0.08, w * 3.0), hline(q.x * 3.0, 0.08, w * 3.0), odd);
    }
    return hline(q.y * 2.0 + 0.35 * sin(q.x * 6.2831853), 0.07, w * 2.5);
}

// .PAT line families: angle, origin, offset (dx along, dy across), dashes (+ line, − gap, 0 dot).
fn pat_ink(k: i32, q: vec2<f32>, w: f32) -> f32 {
    let base = k * 25;
    let n = i32(pats[base].x);
    var ink = 0.0;
    for (var f = 0; f < 8; f = f + 1) {
        if (f >= n) {
            break;
        }
        let a = pats[base + 1 + f * 3];
        let b = pats[base + 2 + f * 3];
        let c = pats[base + 3 + f * 3];
        let dy = b.x;
        if (abs(dy) < 1e-6) {
            continue;
        }
        let cs = cos(a.x);
        let sn = sin(a.x);
        let p = q - vec2<f32>(a.y, a.z);
        let s = p.x * cs + p.y * sn;
        let t = -p.x * sn + p.y * cs;
        let kk = round(t / dy);
        let dist = abs(t - kk * dy);
        var on = 1.0;
        let nd = i32(b.y);
        if (nd > 0) {
            var ds = array<f32, 6>(b.z, b.w, c.x, c.y, c.z, c.w);
            var total = 0.0;
            for (var j = 0; j < 6; j = j + 1) {
                if (j < nd) {
                    total = total + max(abs(ds[j]), w);
                }
            }
            let sp = s - kk * a.w;
            let pos = sp - floor(sp / total) * total;
            var acc = 0.0;
            on = 0.0;
            for (var j = 0; j < 6; j = j + 1) {
                if (j < nd) {
                    let len = max(abs(ds[j]), w);
                    if (pos >= acc && pos < acc + len && ds[j] >= 0.0) {
                        on = 1.0;
                    }
                    acc = acc + len;
                }
            }
        }
        ink = max(ink, (1.0 - smoothstep(0.5 * w, 1.5 * w, dist)) * on);
    }
    return ink;
}

@fragment
fn fs_face(i: VOut, @builtin(front_facing) ff: bool) -> @location(0) vec4<f32> {
    // derivatives first, in uniform control flow
    // projective texture coordinates: uv / |tan.w|
    let uvp = i.uv / max(abs(i.tan.w), 1e-6);
    let ddx = dpdx(uvp);
    let ddy = dpdy(uvp);
    let hn = normalize(i.nrm + vec3<f32>(0.0, 0.0, 1e-7));
    let hu = normalize(i.tan.xyz + vec3<f32>(1e-7, 0.0, 0.0));
    let hq = vec2<f32>(dot(i.wpos, hu), dot(i.wpos, cross(hn, hu))) / max(i.tan.w, 0.001);
    let hw = fwidth(hq);
    let hdx = dpdx(hq);
    let hdy = dpdy(hq);
    if (clipped(i.wpos)) {
        discard;
    }
    if (i.layer < -999.5) {
        // section fill with an image hatch (tiled texture layer)
        let t = textureSampleGrad(tex, tex_smp, hq, i32(-1000.0 - i.layer + 0.5), hdx, hdy);
        return vec4<f32>(t.rgb, 1.0);
    }
    if (i.layer < -99.5) {
        // imported .PAT pattern
        let k = pat_ink(i32(-100.0 - i.layer + 0.5), hq, max(hw.x, hw.y));
        return vec4<f32>(mix(i.col.rgb, i.back.rgb, k), 1.0);
    }
    if (i.layer < -19.5) {
        // section fill with a hatch pattern (unlit, like a drawing)
        let k = hatch(-20.0 - i.layer, hq, max(hw.x, hw.y));
        return vec4<f32>(mix(i.col.rgb, i.back.rgb, k), 1.0);
    }
    var base = select(i.back, i.col, ff);
    let both = i.layer >= 999.5;
    let slot = select(i.layer, i.layer - 1000.0, both);
    var n = normalize(i.nrm);
    if (!ff) {
        n = -n;
    }
    var rough = 0.85;
    var metal = 0.0;
    var occ = 1.0;
    let textured = slot > -0.5 && (ff || both);
    if (textured) {
        let m = mats[i32(slot + 0.5)];
        rough = m.p1.z;
        metal = m.p1.y;
        if (m.layers.x > -0.5) {
            let t = textureSampleGrad(tex, tex_smp, uvp, i32(m.layers.x + 0.5), ddx, ddy);
            base = vec4<f32>(base.rgb * t.rgb, base.a);
        }
        if (u.flags2.x > 0.5) {
            if (m.layers.z > -0.5) {
                var r = pick(textureSampleGrad(tex, tex_smp, uvp, i32(m.layers.z + 0.5), ddx, ddy), m.ch.x);
                if (m.ch.w > 0.5) {
                    r = 1.0 - r;
                }
                rough = rough * r;
            }
            if (m.layers.w > -0.5) {
                metal = metal * pick(textureSampleGrad(tex, tex_smp, uvp, i32(m.layers.w + 0.5), ddx, ddy), m.ch.y);
            }
            if (m.p1.x > -0.5) {
                let a = pick(textureSampleGrad(tex, tex_smp, uvp, i32(m.p1.x + 0.5), ddx, ddy), m.ch.z);
                occ = mix(1.0, a, clamp(m.p2.x, 0.0, 1.0));
            }
            if (m.layers.y > -0.5) {
                let tn = textureSampleGrad(tex, tex_smp, uvp, i32(m.layers.y + 0.5), ddx, ddy).xyz * 2.0 - 1.0;
                let t = normalize(i.tan.xyz - n * dot(n, i.tan.xyz));
                let b = cross(n, t) * sign(i.tan.w);
                // OpenGL-style maps: green points toward -v (image up)
                let nn = normalize(t * tn.x * m.p1.w - b * tn.y * m.p1.w + n * tn.z);
                n = nn;
            }
        }
    }
    let albedo = base.rgb;
    var emissive = 0.0;
    if (textured) {
        emissive = mats[i32(slot + 0.5)].p2.y;
    }
    if (emissive > 0.0) {
        return vec4<f32>(apply_fog(min(albedo * 1.6, vec3<f32>(1.0)), i.wpos), base.a);
    }
    let lamp = lamp_light(i.wpos, n) * LIGHT_K;
    var color: vec3<f32>;
    let shadows = u.flags.y > 0.5;
    if (u.flags2.x > 0.5) {
        // physically based shading
        let v = normalize(u.eye.xyz - i.wpos);
        var l = normalize(vec3<f32>(0.35, 0.55, 0.75));
        var light = 0.75;
        var amb = 0.55;
        if (shadows) {
            l = normalize(u.sun.xyz);
            light = 1.15 * u.light.x;
            amb = 0.25 + 0.6 * u.light.y;
        }
        let ndl = max(dot(n, l), 0.0);
        let ndv = max(dot(n, v), 0.001);
        var vis = 1.0;
        if (shadows && u.sun.w > 0.5 && ndl > 0.0) {
            vis = sun_visibility(i.wpos, 0.0015);
        }
        let h = normalize(l + v);
        let ndh = max(dot(n, h), 0.0);
        let vdh = max(dot(v, h), 0.0);
        let a = max(rough * rough, 0.002);
        let a2 = a * a;
        let dd = ndh * ndh * (a2 - 1.0) + 1.0;
        let D = a2 / (PI * dd * dd);
        let k = (rough + 1.0) * (rough + 1.0) / 8.0;
        let G = (ndv / (ndv * (1.0 - k) + k)) * (ndl / (ndl * (1.0 - k) + k));
        let f0 = mix(vec3<f32>(0.04), albedo, metal);
        let F = f0 + (1.0 - f0) * pow(1.0 - vdh, 5.0);
        let spec = D * G * F / max(4.0 * ndv * ndl, 0.001);
        // hemispherical ambient: brighter from above
        let hemi = 0.75 + 0.25 * n.z;
        let diffuse = albedo * (1.0 - metal) * (amb * hemi * occ + light * ndl * vis);
        var refl = vec3<f32>(0.0);
        if (u.flags2.z > 0.5) {
            let r = reflect(-v, n);
            let fr = f0 + (max(vec3<f32>(1.0 - rough), f0) - f0) * pow(1.0 - ndv, 5.0);
            refl = environment(r, rough * 6.0) * fr * u.env.z * (1.0 - 0.7 * rough) * occ;
        }
        color = diffuse + spec * light * ndl * vis + refl + albedo * (1.0 - metal) * lamp * occ;
    } else {
        var k: f32;
        if (shadows) {
            let l = normalize(u.sun.xyz);
            let ndl = max(dot(n, l), 0.0);
            var vis = 0.0;
            if (ndl > 0.0) {
                vis = select(1.0, sun_visibility(i.wpos, 0.0015), u.sun.w > 0.5);
            }
            k = 0.25 + 0.65 * u.light.y + 0.75 * u.light.x * ndl * vis;
        } else {
            let l = normalize(vec3<f32>(0.35, 0.55, 0.75));
            k = 0.62 + 0.38 * abs(dot(n, l));
        }
        color = albedo * k + albedo * lamp;
    }
    return vec4<f32>(apply_fog(color, i.wpos), base.a);
}

// Edges; vertices marked layer = -2 belong to extension segments: the moving
// end is pushed uv.x pixels beyond the edge end, away from the other end (back.xyz).
@vertex
fn vs_line(v: VIn) -> VOut {
    var o: VOut;
    o.clip = u.vp * vec4<f32>(v.pos, 1.0);
    o.nrm = v.nrm;
    o.col = v.col;
    o.back = v.col;
    o.wpos = v.pos;
    o.uv = v.uv;
    o.layer = v.layer;
    o.tan = v.tan;
    if (v.layer < -9.5) {
        // dashed edge (tag dash pattern): screen position of the edge start for the fragment shader
        let s = u.vp * vec4<f32>(v.back.xyz, 1.0);
        if (s.w > 0.0001) {
            let n = s.xy / s.w;
            o.tan = vec4<f32>((n.x * 0.5 + 0.5) * u.vpsize.x, (0.5 - n.y * 0.5) * u.vpsize.y, -10.0 - v.layer, 0.0);
        }
    } else if (v.layer < -1.5 && v.uv.y > 0.5) {
        let b = u.vp * vec4<f32>(v.back.xyz, 1.0);
        if (o.clip.w > 0.0001 && b.w > 0.0001) {
            let sa = o.clip.xy / o.clip.w;
            let sb = b.xy / b.w;
            let dp = (sa - sb) * u.vpsize.xy;
            if (length(dp) > 0.001) {
                let off = normalize(dp) * v.uv.x * u.post.w * 2.0 / u.vpsize.xy;
                o.clip = vec4<f32>((sa + off) * o.clip.w, o.clip.z, o.clip.w);
            }
        }
    }
    return o;
}

fn jitter_hash(p: vec3<f32>, s: f32) -> vec2<f32> {
    let k = dot(p, vec3<f32>(12.9898, 78.233, 37.719)) + s * 0.6180339;
    return vec2<f32>(fract(sin(k) * 43758.5453), fract(sin(k * 1.37 + 0.71) * 24634.6345)) * 2.0 - 1.0;
}

// Wide edges (depth cue, endpoints, jitter): each segment is a screen-space quad.
// back.xyz = other end, back.w = side (-1/+1); uv.x = edge seed;
// uv.y = kind (0 body, 1 cap near end, 2 cap far end, 3 extension); tan.x = extension px.
@vertex
fn vs_wide(v: VIn) -> VOut {
    var o: VOut;
    o.nrm = v.nrm;
    o.col = v.col;
    o.back = v.col;
    o.wpos = v.pos;
    o.uv = v.uv;
    o.layer = v.layer;
    o.tan = v.tan;
    var a = u.vp * vec4<f32>(v.pos, 1.0);
    var b = u.vp * vec4<f32>(v.back.xyz, 1.0);
    let near = 0.001;
    if (a.w < near && b.w < near) {
        o.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        return o;
    }
    if (a.w < near) {
        a = mix(a, b, (near - a.w) / (b.w - a.w));
    }
    if (b.w < near) {
        b = mix(b, a, (near - b.w) / (a.w - b.w));
    }
    let half = u.vpsize.xy * 0.5;
    let px = u.post.w;
    var sa = a.xy / a.w * half;
    var sb = b.xy / b.w * half;
    if (u.edge.z > 0.0) {
        sa = sa + jitter_hash(v.pos, v.uv.x) * u.edge.z * px;
        sb = sb + jitter_hash(v.back.xyz, v.uv.x) * u.edge.z * px;
    }
    let d = sb - sa;
    let len = length(d);
    var dir = vec2<f32>(1.0, 0.0);
    if (len > 0.0001) {
        dir = d / len;
    }
    let perp = vec2<f32>(-dir.y, dir.x);
    var w = 1.0;
    if (u.edge.x > 0.0) {
        let dist = distance(v.pos, u.eye.xyz);
        w = mix(u.edge.x, 1.0, clamp(dist / max(u.edge.w, 0.001), 0.0, 1.0));
    }
    var p = sa;
    let kind = v.uv.y;
    if (kind > 0.5 && kind < 2.5) {
        w = w + 1.5;
    }
    if (kind > 1.5 && kind < 2.5) {
        p = sa + dir * min(u.edge.y * px, len * 0.5);
    }
    if (kind > 2.5) {
        p = sa - dir * v.tan.x * px;
    }
    let off = perp * v.back.w * 0.5 * max(w, 1.0) * px;
    o.clip = vec4<f32>((p + off) / half, a.z / a.w, 1.0);
    if (v.tan.y > 0.5) {
        // tag dash pattern: measure from the same edge end on every vertex of the edge
        var first = v.back.xyz;
        let pa = v.pos;
        let pb = v.back.xyz;
        if (pa.x < pb.x || (pa.x == pb.x && (pa.y < pb.y || (pa.y == pb.y && pa.z < pb.z)))) {
            first = pa;
        }
        let s = u.vp * vec4<f32>(first, 1.0);
        if (s.w > 0.0001) {
            let n = s.xy / s.w;
            o.tan = vec4<f32>((n.x * 0.5 + 0.5) * u.vpsize.x, (0.5 - n.y * 0.5) * u.vpsize.y, v.tan.y, 0.0);
            o.layer = -10.0 - v.tan.y;
        }
    }
    return o;
}

fn dash_off(p: f32, d: f32) -> bool {
    if (p < 1.5) {
        return fract(d / 13.0) * 13.0 > 8.0;
    }
    if (p < 2.5) {
        return fract(d / 6.0) * 6.0 > 2.0;
    }
    if (p < 3.5) {
        let t = fract(d / 20.0) * 20.0;
        return !(t < 10.0 || (t > 14.0 && t < 16.0));
    }
    return fract(d / 24.0) * 24.0 > 18.0;
}

@fragment
fn fs_line(i: VOut) -> @location(0) vec4<f32> {
    if (clipped(i.wpos)) {
        discard;
    }
    if (i.layer < -9.5 && i.layer > -19.5) {
        if (dash_off(i.tan.z, distance(i.clip.xy, i.tan.xy) / u.post.w)) {
            discard;
        }
    }
    return vec4<f32>(apply_fog(i.col.rgb, i.wpos), i.col.a);
}

// Edges hidden behind faces, dashed (Back Edges style).
@fragment
fn fs_line_back(i: VOut) -> @location(0) vec4<f32> {
    if (clipped(i.wpos)) {
        discard;
    }
    let p = i.clip.xy;
    if (fract((p.x + p.y) / (8.0 * u.post.w)) > 0.5) {
        discard;
    }
    return vec4<f32>(mix(i.col.rgb, u.bg.rgb, 0.35), i.col.a);
}

// Shadow on the ground plane: only the shadow itself is drawn.
@fragment
fn fs_ground(i: VOut) -> @location(0) vec4<f32> {
    let vis = sun_visibility(i.wpos, 0.0025);
    let a = (1.0 - vis) * 0.45 * u.light.x;
    if (a < 0.003) {
        discard;
    }
    return vec4<f32>(0.0, 0.0, 0.0, a);
}

@vertex
fn vs_shadow(v: VIn) -> @builtin(position) vec4<f32> {
    return u.light_vp * vec4<f32>(v.pos, 1.0);
}

struct BgOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_bg(@builtin(vertex_index) i: u32) -> BgOut {
    var o: BgOut;
    let x = f32((i << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(i & 2u) * 2.0 - 1.0;
    o.clip = vec4<f32>(x, y, 1.0, 1.0);
    o.ndc = vec2<f32>(x, y);
    return o;
}

fn background(ndc: vec2<f32>) -> vec3<f32> {
    if (u.flags.z < 0.5) {
        return u.bg.rgb;
    }
    let a = u.inv_vp * vec4<f32>(ndc, 0.0, 1.0);
    let b = u.inv_vp * vec4<f32>(ndc, 1.0, 1.0);
    let d = normalize(b.xyz / b.w - a.xyz / a.w);
    if (u.env.w > 0.5 && u.flags2.y > 0.5) {
        return environment(d, 0.0) * u.env.y;
    }
    return procedural_sky(d);
}

// Sky/ground gradient, environment panorama, or a solid color.
@fragment
fn fs_bg(i: BgOut) -> @location(0) vec4<f32> {
    return vec4<f32>(background(i.ndc), 1.0);
}

// One background layer (watermark or matched photo), alpha-blended over the sky.
@fragment
fn fs_wm(i: BgOut) -> @location(0) vec4<f32> {
    let px = vec2<f32>(i.ndc.x * 0.5 + 0.5, 0.5 - i.ndc.y * 0.5) * u.vpsize.xy;
    var size = wmu.b.xy * wmu.a.z * u.post.w;
    if (wmu.a.y > 2.5) {
        size = vec2<f32>(u.vpsize.y * wmu.b.x / max(wmu.b.y, 1.0), u.vpsize.y);
    }
    var uv = px / u.vpsize.xy;
    var inside = true;
    if (wmu.a.y > 0.5 && wmu.a.y < 1.5) {
        uv = fract(px / max(size, vec2<f32>(1.0)));
    } else if (wmu.a.y > 1.5) {
        let origin = (u.vpsize.xy - size) * wmu.b.zw;
        uv = (px - origin) / max(size, vec2<f32>(1.0));
        inside = all(uv >= vec2<f32>(0.0)) && all(uv <= vec2<f32>(1.0));
    }
    let t = textureSampleLevel(wm_tex, tex_smp, uv, 0.0);
    return vec4<f32>(t.rgb, select(0.0, t.a * wmu.a.x, inside));
}
"#;

/// Post-processing shader (SSAO + profiles); DEPTH_TYPE is filled at runtime.
pub(crate) const POST_SHADER: &str = r#"
struct U {
    vp: mat4x4<f32>,
    inv_vp: mat4x4<f32>,
    clip: vec4<f32>,
    flags: vec4<f32>,
    bg: vec4<f32>,
    sun: vec4<f32>,
    light: vec4<f32>,
    eye: vec4<f32>,
    light_vp: mat4x4<f32>,
    env: vec4<f32>,
    flags2: vec4<f32>,
    post: vec4<f32>,
    vpsize: vec4<f32>,
    edge: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var depth_tex: DEPTH_TYPE;

struct PostOut {
    @builtin(position) clip: vec4<f32>,
};

@vertex
fn vs_post(@builtin(vertex_index) i: u32) -> PostOut {
    var o: PostOut;
    let x = f32((i << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(i & 2u) * 2.0 - 1.0;
    o.clip = vec4<f32>(x, y, 0.0, 1.0);
    return o;
}

fn depth_at(p: vec2<i32>) -> f32 {
    let s = vec2<i32>(u.vpsize.xy) - vec2<i32>(1, 1);
    return textureLoad(depth_tex, clamp(p, vec2<i32>(0, 0), s), 0);
}

fn world_at(p: vec2<i32>, z: f32) -> vec3<f32> {
    let ndc = vec2<f32>((f32(p.x) + 0.5) / u.vpsize.x * 2.0 - 1.0, 1.0 - (f32(p.y) + 0.5) / u.vpsize.y * 2.0);
    let w = u.inv_vp * vec4<f32>(ndc, z, 1.0);
    return w.xyz / w.w;
}

// Output is multiplied onto the frame (blend: src * dst).
@fragment
fn fs_post(o: PostOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(o.clip.xy);
    let z = depth_at(p);
    var shade = 1.0;
    // profiles: strong depth discontinuities become thick dark lines
    let t = i32(round(u.post.z * u.post.w));
    if (t > 0) {
        let pc = world_at(p, z);
        let dc = select(distance(pc, u.eye.xyz), 1e9, z >= 1.0);
        var edge = false;
        for (var k = 0; k < 4; k = k + 1) {
            var off = vec2<i32>(t, 0);
            if (k == 1) { off = vec2<i32>(-t, 0); }
            if (k == 2) { off = vec2<i32>(0, t); }
            if (k == 3) { off = vec2<i32>(0, -t); }
            let zs = depth_at(p + off);
            let ds = select(distance(world_at(p + off, zs), u.eye.xyz), 1e9, zs >= 1.0);
            if (ds - dc > max(dc * 0.08, 0.05) && z < 1.0) {
                edge = true;
            }
        }
        if (edge) {
            shade = 0.12;
        }
    }
    // screen-space ambient occlusion
    if (u.post.x > 0.0 && z < 1.0) {
        let pc = world_at(p, z);
        let px = world_at(p + vec2<i32>(1, 0), depth_at(p + vec2<i32>(1, 0)));
        let py = world_at(p + vec2<i32>(0, 1), depth_at(p + vec2<i32>(0, 1)));
        var n = normalize(cross(px - pc, py - pc));
        let toeye = normalize(u.eye.xyz - pc);
        if (dot(n, toeye) < 0.0) {
            n = -n;
        }
        let dist = distance(pc, u.eye.xyz);
        let radius = u.post.y;
        // world radius projected to pixels (approx.)
        let rpx = clamp(radius / max(dist, 0.01) * u.vpsize.y * 0.9, 3.0, 64.0);
        var occ = 0.0;
        let samples = 16;
        for (var k = 0; k < samples; k = k + 1) {
            let fk = f32(k);
            let ang = fk * 2.39996 + f32((p.x * 7 + p.y * 13) % 17) * 0.37;
            let rr = rpx * sqrt((fk + 0.5) / f32(samples));
            let q = p + vec2<i32>(vec2<f32>(cos(ang), sin(ang)) * rr);
            let zq = depth_at(q);
            if (zq >= 1.0) {
                continue;
            }
            let v = world_at(q, zq) - pc;
            let len = length(v);
            if (len < 0.0001 || len > radius) {
                continue;
            }
            let w = max(dot(n, v / len) - 0.15, 0.0) * (1.0 - len / radius);
            occ = occ + w;
        }
        occ = occ / f32(samples);
        shade = shade * clamp(1.0 - occ * 2.2 * u.post.x, 0.0, 1.0);
    }
    return vec4<f32>(shade, shade, shade, 1.0);
}
"#;

const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SHADOW_SIZE: u32 = 2048;
const UNIFORM_FLOATS: usize = 16 + 16 + 4 * 6 + 16 + 4 * 5;

struct Targets {
    size: [u32; 2],
    msaa_view: Option<wgpu::TextureView>,
    depth_view: wgpu::TextureView,
    resolve_tex: wgpu::Texture,
    resolve_view: wgpu::TextureView,
    post_bind: wgpu::BindGroup,
}

pub struct Renderer3D {
    face_pipe: wgpu::RenderPipeline,
    line_pipe: wgpu::RenderPipeline,
    line_back_pipe: wgpu::RenderPipeline,
    wide_pipe: wgpu::RenderPipeline,
    wide_back_pipe: wgpu::RenderPipeline,
    shadow_line_pipe: wgpu::RenderPipeline,
    wide_buf: Option<(wgpu::Buffer, u32)>,
    shadow_line_buf: Option<(wgpu::Buffer, u32)>,
    glass_pipe: wgpu::RenderPipeline,
    bg_pipe: wgpu::RenderPipeline,
    ground_pipe: wgpu::RenderPipeline,
    shadow_pipe: wgpu::RenderPipeline,
    post_pipe: wgpu::RenderPipeline,
    post_bgl: wgpu::BindGroupLayout,
    ubuf: wgpu::Buffer,
    bind: wgpu::BindGroup,
    shadow_bind: wgpu::BindGroup,
    shadow_view: wgpu::TextureView,
    samples: u32,
    targets: Option<Targets>,
    texture_id: Option<egui::TextureId>,
    tri_buf: Option<(wgpu::Buffer, u32)>,
    line_buf: Option<(wgpu::Buffer, u32)>,
    glass_buf: Option<(wgpu::Buffer, u32)>,
    nocast_buf: Option<(wgpu::Buffer, u32)>,
    bgl: wgpu::BindGroupLayout,
    shadow_smp: wgpu::Sampler,
    tex_smp: wgpu::Sampler,
    tex: (wgpu::Texture, wgpu::TextureView, u32),
    tex_sig: u64,
    env: (wgpu::Texture, wgpu::TextureView),
    env_sig: u64,
    wm_bgl: wgpu::BindGroupLayout,
    wm_pipe: wgpu::RenderPipeline,
    bg_layers: Vec<BgLayer>,
    pat_buf: wgpu::Buffer,
    pat_cap: usize,
    light_buf: wgpu::Buffer,
    light_cap: usize,
    mat_buf: wgpu::Buffer,
    mat_cap: usize,
    pub clear: wgpu::Color,
}

fn make_tex_array(device: &wgpu::Device, layers: u32) -> (wgpu::Texture, wgpu::TextureView, u32) {
    let t = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("homeup-textures"),
        size: wgpu::Extent3d { width: crate::tex::LAYER, height: crate::tex::LAYER, depth_or_array_layers: layers.max(1) },
        mip_level_count: crate::tex::mip_levels(),
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let v = t.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
    (t, v, layers.max(1))
}

/// Equirectangular environment texture with mips (2:1).
fn make_env(device: &wgpu::Device, queue: &wgpu::Queue, img: Option<&image::RgbaImage>) -> (wgpu::Texture, wgpu::TextureView) {
    let (w, h) = img.map(|i| (i.width(), i.height())).unwrap_or((2, 1));
    let levels = (w.max(h) as f32).log2().floor() as u32 + 1;
    let t = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("homeup-env"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut cur = img.cloned().unwrap_or_else(|| image::RgbaImage::from_pixel(2, 1, image::Rgba([200, 210, 220, 255])));
    for level in 0..levels {
        let (cw, ch) = (cur.width(), cur.height());
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &t, mip_level: level, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            cur.as_raw(),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * cw), rows_per_image: Some(ch) },
            wgpu::Extent3d { width: cw, height: ch, depth_or_array_layers: 1 },
        );
        if level + 1 < levels {
            cur = image::imageops::resize(&cur, (cw / 2).max(1), (ch / 2).max(1), image::imageops::FilterType::Triangle);
        }
    }
    let v = t.create_view(&wgpu::TextureViewDescriptor::default());
    (t, v)
}

impl Renderer3D {
    pub fn new(rs: &RenderState) -> Self {
        let device = &rs.device;
        let samples = if rs.adapter.get_texture_format_features(COLOR_FORMAT).flags.sample_count_supported(4)
            && rs.adapter.get_texture_format_features(DEPTH_FORMAT).flags.sample_count_supported(4)
        {
            4
        } else {
            1
        };
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("homeup-shader"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let post_src = POST_SHADER.replace("DEPTH_TYPE", if samples > 1 { "texture_depth_multisampled_2d" } else { "texture_depth_2d" });
        let post_module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("homeup-post"), source: wgpu::ShaderSource::Wgsl(post_src.into()) });
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("homeup-uniforms"),
            size: (UNIFORM_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shadow_view = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("homeup-shadow"),
                size: wgpu::Extent3d { width: SHADOW_SIZE, height: SHADOW_SIZE, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        let shadow_smp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("homeup-shadow-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let tex_smp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("homeup-tex-sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let uniform_entry = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let frag_tex = |binding, sample_type, view_dimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type, view_dimension, multisampled: false },
            count: None,
        };
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("homeup-bgl"),
            entries: &[
                uniform_entry,
                frag_tex(1, wgpu::TextureSampleType::Depth, wgpu::TextureViewDimension::D2),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                frag_tex(3, wgpu::TextureSampleType::Float { filterable: true }, wgpu::TextureViewDimension::D2Array),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                frag_tex(5, wgpu::TextureSampleType::Float { filterable: true }, wgpu::TextureViewDimension::D2),
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
            ],
        });
        let shadow_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("homeup-shadow-bgl"), entries: &[uniform_entry] });
        let post_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("homeup-post-bgl"),
            entries: &[
                uniform_entry,
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: samples > 1,
                    },
                    count: None,
                },
            ],
        });
        let tex_view = make_tex_array(device, 1);
        let env = make_env(device, &rs.queue, None);
        let pat_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("homeup-pats"),
            size: 25 * 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let light_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("homeup-lights"),
            contents: bytemuck::cast_slice(&[[0f32; 4]]),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });
        let wm_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("homeup-wm-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                frag_tex(1, wgpu::TextureSampleType::Float { filterable: true }, wgpu::TextureViewDimension::D2),
            ],
        });
        let mat_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("homeup-mats"),
            size: 64 * 64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shadow_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("homeup-shadow-bind"),
            layout: &shadow_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("homeup-layout"), bind_group_layouts: &[Some(&bgl)], immediate_size: 0 });
        let shadow_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("homeup-shadow-layout"), bind_group_layouts: &[Some(&shadow_bgl)], immediate_size: 0 });
        let post_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("homeup-post-layout"), bind_group_layouts: &[Some(&post_bgl)], immediate_size: 0 });
        let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x4, 4 => Float32x2, 5 => Float32, 6 => Float32x4];
        let vbl = wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs };
        let primitive = |topology| wgpu::PrimitiveState {
            topology,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        };
        let make_vs = |vs: &str, topology, fs: &str, bias: wgpu::DepthBiasState, write: bool, compare: wgpu::CompareFunction| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fs),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &module, entry_point: Some(vs), compilation_options: Default::default(), buffers: &[Some(vbl.clone())] },
                primitive: primitive(topology),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(write),
                    depth_compare: Some(compare),
                    stencil: Default::default(),
                    bias,
                }),
                multisample: wgpu::MultisampleState { count: samples, mask: !0, alpha_to_coverage_enabled: false },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let make = |topology, fs: &str, bias: wgpu::DepthBiasState, write: bool, compare: wgpu::CompareFunction| {
            let vs = if topology == wgpu::PrimitiveTopology::LineList { "vs_line" } else { "vs" };
            make_vs(vs, topology, fs, bias, write, compare)
        };
        use wgpu::CompareFunction::{Greater, LessEqual};
        let wide_pipe = make_vs("vs_wide", wgpu::PrimitiveTopology::TriangleList, "fs_line", Default::default(), true, LessEqual);
        let wide_back_pipe = make_vs("vs_wide", wgpu::PrimitiveTopology::TriangleList, "fs_line_back", Default::default(), false, Greater);
        let face_bias = wgpu::DepthBiasState { constant: 4, slope_scale: 2.0, clamp: 0.0 };
        let face_pipe = make(wgpu::PrimitiveTopology::TriangleList, "fs_face", face_bias, true, LessEqual);
        let glass_pipe = make(wgpu::PrimitiveTopology::TriangleList, "fs_face", face_bias, false, LessEqual);
        let ground_pipe = make(wgpu::PrimitiveTopology::TriangleList, "fs_ground", wgpu::DepthBiasState { constant: 8, slope_scale: 4.0, clamp: 0.0 }, false, LessEqual);
        let line_pipe = make(wgpu::PrimitiveTopology::LineList, "fs_line", Default::default(), true, LessEqual);
        let line_back_pipe = make(wgpu::PrimitiveTopology::LineList, "fs_line_back", Default::default(), false, Greater);
        let bg_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("homeup-bg"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_bg"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: samples, mask: !0, alpha_to_coverage_enabled: false },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_bg"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let mk_shadow = |topology: wgpu::PrimitiveTopology| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("homeup-shadow"),
            layout: Some(&shadow_layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_shadow"), compilation_options: Default::default(), buffers: &[Some(vbl.clone())] },
            primitive: primitive(topology),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(LessEqual),
                stencil: Default::default(),
                bias: if topology == wgpu::PrimitiveTopology::LineList { Default::default() } else { wgpu::DepthBiasState { constant: 2, slope_scale: 2.5, clamp: 0.0 } },
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        let shadow_pipe = mk_shadow(wgpu::PrimitiveTopology::TriangleList);
        let shadow_line_pipe = mk_shadow(wgpu::PrimitiveTopology::LineList);
        // multiply blend: result = src * dst
        let multiply = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Dst, dst_factor: wgpu::BlendFactor::Zero, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let wm_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("homeup-wm-layout"), bind_group_layouts: &[Some(&bgl), Some(&wm_bgl)], immediate_size: 0 });
        let wm_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("homeup-wm"),
            layout: Some(&wm_layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_bg"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: samples, mask: !0, alpha_to_coverage_enabled: false },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_wm"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let post_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("homeup-post"),
            layout: Some(&post_layout),
            vertex: wgpu::VertexState { module: &post_module, entry_point: Some("vs_post"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &post_module,
                entry_point: Some("fs_post"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend: Some(multiply), write_mask: wgpu::ColorWrites::COLOR })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let c = srgb(214, 226, 236);
        let mut r = Renderer3D {
            face_pipe,
            line_pipe,
            line_back_pipe,
            wide_pipe,
            wide_back_pipe,
            shadow_line_pipe,
            wide_buf: None,
            shadow_line_buf: None,
            glass_pipe,
            bg_pipe,
            ground_pipe,
            shadow_pipe,
            post_pipe,
            post_bgl,
            bind: shadow_bind.clone(),
            ubuf,
            shadow_bind,
            shadow_view,
            samples,
            targets: None,
            texture_id: None,
            tri_buf: None,
            line_buf: None,
            glass_buf: None,
            nocast_buf: None,
            bgl,
            shadow_smp,
            tex_smp,
            tex: tex_view,
            tex_sig: 0,
            env,
            wm_bgl,
            wm_pipe,
            bg_layers: Vec::new(),
            pat_buf,
            pat_cap: 25,
            light_buf,
            light_cap: 1,
            env_sig: 0,
            mat_buf,
            mat_cap: 64,
            clear: wgpu::Color { r: c[0] as f64, g: c[1] as f64, b: c[2] as f64, a: 1.0 },
        };
        r.rebind(device);
        r
    }

    fn rebind(&mut self, device: &wgpu::Device) {
        self.bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("homeup-bind"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.shadow_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.shadow_smp) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.tex.1) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.tex_smp) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&self.env.1) },
                wgpu::BindGroupEntry { binding: 6, resource: self.mat_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 7, resource: self.pat_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 8, resource: self.light_buf.as_entire_binding() },
            ],
        });
    }

    /// Upload the document textures (only when they changed).
    pub fn set_textures(&mut self, rs: &RenderState, list: &[crate::tex::TextureData]) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        list.len().hash(&mut h);
        for t in list {
            t.bytes.len().hash(&mut h);
            t.name.hash(&mut h);
            t.bytes.get(..64).hash(&mut h);
        }
        let sig = h.finish() | 1;
        if sig == self.tex_sig {
            return;
        }
        self.tex_sig = sig;
        let n = list.len().clamp(1, crate::tex::MAX_LAYERS) as u32;
        if n > self.tex.2 || self.tex.2 > n * 2 {
            self.tex = make_tex_array(&rs.device, n.next_power_of_two());
        }
        for (layer, t) in list.iter().take(crate::tex::MAX_LAYERS).enumerate() {
            let mut s = crate::tex::LAYER;
            for (mip, data) in t.mip_chain().iter().enumerate() {
                rs.queue.write_texture(
                    wgpu::TexelCopyTextureInfo { texture: &self.tex.0, mip_level: mip as u32, origin: wgpu::Origin3d { x: 0, y: 0, z: layer as u32 }, aspect: wgpu::TextureAspect::All },
                    data,
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * s), rows_per_image: Some(s) },
                    wgpu::Extent3d { width: s, height: s, depth_or_array_layers: 1 },
                );
                s = (s / 2).max(1);
            }
        }
        self.rebind(&rs.device);
    }

    /// Background layers in drawing order: (signature, parameters). `make(k)`
    /// decodes layer k's image only when its signature changed.
    pub fn set_bg_layers(&mut self, rs: &RenderState, layers: &[(u64, [f32; 8])], make: impl Fn(usize) -> Option<image::RgbaImage>) {
        self.bg_layers.truncate(layers.len());
        for (k, (sig, params)) in layers.iter().enumerate() {
            if self.bg_layers.get(k).is_some_and(|l| l.sig == *sig) {
                rs.queue.write_buffer(&self.bg_layers[k].ubuf, 0, bytemuck::cast_slice(params));
                continue;
            }
            let img = make(k).map(|i| {
                let w = i.width().min(2048);
                let h = ((i.height() as u64 * w as u64) / i.width().max(1) as u64).max(1) as u32;
                image::imageops::resize(&i, w, h, image::imageops::FilterType::Triangle)
            });
            let tex = make_env(&rs.device, &rs.queue, img.as_ref());
            let ubuf = rs.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("homeup-wm-u"),
                contents: bytemuck::cast_slice(params),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
            let bind = rs.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("homeup-wm-bind"),
                layout: &self.wm_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&tex.1) },
                ],
            });
            let layer = BgLayer { sig: *sig, _tex: tex, ubuf, bind };
            if k < self.bg_layers.len() {
                self.bg_layers[k] = layer;
            } else {
                self.bg_layers.push(layer);
            }
        }
    }

    /// Upload the environment panorama (None = procedural sky).
    pub fn set_environment(&mut self, rs: &RenderState, img: Option<&crate::tex::TextureData>) {
        let sig = img.map(|t| (t.bytes.len() as u64) << 1 | 1).unwrap_or(2);
        if sig == self.env_sig {
            return;
        }
        self.env_sig = sig;
        let decoded = img.and_then(|t| t.decode()).map(|i| {
            // keep a 2:1 panorama up to 2048 px wide
            let w = i.width().min(2048);
            image::imageops::resize(&i, w, (w / 2).max(1), image::imageops::FilterType::Triangle)
        });
        self.env = make_env(&rs.device, &rs.queue, decoded.as_ref());
        self.rebind(&rs.device);
    }

    pub fn upload(&mut self, rs: &RenderState, scene: &Scene3D) {
        let mk = |data: &[Vertex], label| {
            if data.is_empty() {
                None
            } else {
                Some((
                    rs.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: bytemuck::cast_slice(data), usage: wgpu::BufferUsages::VERTEX }),
                    data.len() as u32,
                ))
            }
        };
        self.tri_buf = mk(&scene.tris, "homeup-tris");
        self.line_buf = mk(&scene.lines, "homeup-lines");
        self.glass_buf = mk(&scene.glass, "homeup-glass");
        self.nocast_buf = mk(&scene.nocast, "homeup-nocast");
        self.wide_buf = mk(&scene.wide, "homeup-wide");
        self.shadow_line_buf = mk(&scene.shadow_lines, "homeup-shadow-lines");
        let nl = scene.lights.len().max(1);
        if nl > self.light_cap {
            self.light_cap = nl.next_power_of_two();
            self.light_buf = rs.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("homeup-lights"),
                size: (self.light_cap * 16) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.rebind(&rs.device);
        }
        let header: Vec<[f32; 4]> = if scene.lights.is_empty() { vec![[0.0; 4]] } else { scene.lights.clone() };
        rs.queue.write_buffer(&self.light_buf, 0, bytemuck::cast_slice(&header));
        let np = scene.pats.len().max(25);
        if np > self.pat_cap {
            self.pat_cap = np.next_power_of_two();
            self.pat_buf = rs.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("homeup-pats"),
                size: (self.pat_cap * 16) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.rebind(&rs.device);
        }
        if !scene.pats.is_empty() {
            rs.queue.write_buffer(&self.pat_buf, 0, bytemuck::cast_slice(&scene.pats));
        }
        let n = scene.mats.len().max(1);
        if n > self.mat_cap {
            self.mat_cap = n.next_power_of_two();
            self.mat_buf = rs.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("homeup-mats"),
                size: (self.mat_cap * 64) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.rebind(&rs.device);
        }
        if !scene.mats.is_empty() {
            rs.queue.write_buffer(&self.mat_buf, 0, bytemuck::cast_slice(&scene.mats));
        }
    }

    fn make_targets(&self, rs: &RenderState, size: [u32; 2]) -> Targets {
        let ext = wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 };
        let tex = |label, format, samples, usage| {
            rs.device.create_texture(&wgpu::TextureDescriptor { label: Some(label), size: ext, mip_level_count: 1, sample_count: samples, dimension: wgpu::TextureDimension::D2, format, usage, view_formats: &[] })
        };
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let resolve_tex = tex("homeup-color", COLOR_FORMAT, 1, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC);
        let resolve_view = view(&resolve_tex);
        let msaa_view = (self.samples > 1).then(|| view(&tex("homeup-msaa", COLOR_FORMAT, self.samples, wgpu::TextureUsages::RENDER_ATTACHMENT)));
        let depth_view = view(&tex("homeup-depth", DEPTH_FORMAT, self.samples, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING));
        let post_bind = rs.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("homeup-post-bind"),
            layout: &self.post_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&depth_view) },
            ],
        });
        Targets { size, msaa_view, depth_view, resolve_tex, resolve_view, post_bind }
    }

    fn ensure_targets(&mut self, rs: &RenderState, size: [u32; 2]) {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let t = self.make_targets(rs, size);
        let mut r = rs.renderer.write();
        match self.texture_id {
            Some(id) => r.update_egui_texture_from_wgpu_texture(&rs.device, &t.resolve_view, wgpu::FilterMode::Linear, id),
            None => self.texture_id = Some(r.register_native_texture(&rs.device, &t.resolve_view, wgpu::FilterMode::Linear)),
        }
        self.targets = Some(t);
    }

    /// Largest image the GPU can render in one piece.
    pub fn max_size(rs: &RenderState) -> u32 {
        rs.device.limits().max_texture_dimension_2d
    }

    /// Render the uploaded scene off screen at any size and read the pixels back.
    pub fn render_offscreen(&mut self, rs: &RenderState, size: [u32; 2], fp: &FrameParams) -> Result<image::RgbaImage, String> {
        let max = Self::max_size(rs);
        if size[0] > max || size[1] > max {
            return Err(format!("a placa de vídeo aceita no máximo {max} px por lado"));
        }
        let t = self.make_targets(rs, size);
        self.draw(rs, &t, fp);
        let row = (size[0] * 4).div_ceil(256) * 256;
        let buf = rs.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("homeup-readback"),
            size: row as u64 * size[1] as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = rs.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("homeup-readback") });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &t.resolve_tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(size[1]) } },
            wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        );
        rs.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = rs.device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv().map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
        let data = slice.get_mapped_range().map_err(|e| format!("{e:?}"))?;
        let mut img = image::RgbaImage::new(size[0], size[1]);
        for y in 0..size[1] as usize {
            let src = &data[y * row as usize..y * row as usize + size[0] as usize * 4];
            img.as_mut()[y * size[0] as usize * 4..(y + 1) * size[0] as usize * 4].copy_from_slice(src);
        }
        drop(data);
        buf.unmap();
        for p in img.pixels_mut() {
            p[3] = 255;
        }
        Ok(img)
    }

    /// Render the uploaded scene; returns the egui texture holding the frame.
    pub fn render(&mut self, rs: &RenderState, size: [u32; 2], fp: &FrameParams) -> egui::TextureId {
        let size = [size[0].max(1), size[1].max(1)];
        self.ensure_targets(rs, size);
        let t = self.targets.take().unwrap();
        self.draw(rs, &t, fp);
        self.targets = Some(t);
        self.texture_id.unwrap()
    }

    fn draw(&mut self, rs: &RenderState, t: &Targets, fp: &FrameParams) {
        let size = t.size;
        let b = |x: bool| x as u8 as f32;
        let mut m = [0f32; UNIFORM_FLOATS];
        m[..16].copy_from_slice(&fp.vp.to_cols_array());
        m[16..32].copy_from_slice(&fp.inv_vp.to_cols_array());
        if let Some(c) = fp.clip {
            m[32..36].copy_from_slice(&c);
        }
        m[36..40].copy_from_slice(&[b(fp.clip.is_some()), b(fp.sun.is_some()), b(fp.sky), b(fp.fog.is_some())]);
        m[40..43].copy_from_slice(&fp.bg);
        m[43] = 1.0;
        if let Some(s) = fp.sun {
            m[44..47].copy_from_slice(&s);
        }
        m[47] = b(fp.shadows_on_faces);
        let (fs, fe) = fp.fog.unwrap_or((0.0, 1.0));
        m[48..52].copy_from_slice(&[fp.light, fp.dark, fs, fe]);
        m[52..55].copy_from_slice(&fp.eye);
        m[56..72].copy_from_slice(&fp.light_vp.to_cols_array());
        m[72..76].copy_from_slice(&[fp.env_rotation, fp.sky_exposure, fp.refl_exposure, b(fp.env_image)]);
        m[76..80].copy_from_slice(&[b(fp.photoreal), b(fp.skydome), b(fp.reflections), b(fp.back_edges)]);
        m[80..84].copy_from_slice(&[fp.ao.0, fp.ao.1, fp.profiles, fp.px_scale]);
        m[84..88].copy_from_slice(&[size[0] as f32, size[1] as f32, 0.0, 0.0]);
        m[88..92].copy_from_slice(&fp.edge);

        rs.queue.write_buffer(&self.ubuf, 0, bytemuck::cast_slice(&m));

        let ground = fp.ground.filter(|_| fp.sun.is_some()).map(|(c, h)| {
            let v = |x: f32, y: f32| Vertex::plain([x, y, 0.0], [0.0, 0.0, 1.0], [0.0; 4], [0.0; 4]);
            let (x0, x1, y0, y1) = (c[0] - h, c[0] + h, c[1] - h, c[1] + h);
            let verts = [v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y0), v(x1, y1), v(x0, y1)];
            rs.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("homeup-ground"), contents: bytemuck::cast_slice(&verts), usage: wgpu::BufferUsages::VERTEX })
        });


        let mut enc = rs.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("homeup-enc") });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("homeup-shadow-pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if fp.sun.is_some() {
                if let Some((b, n)) = &self.tri_buf {
                    pass.set_pipeline(&self.shadow_pipe);
                    pass.set_bind_group(0, &self.shadow_bind, &[]);
                    pass.set_vertex_buffer(0, b.slice(..));
                    pass.draw(0..*n, 0..1);
                }
                if let Some((b, n)) = &self.shadow_line_buf {
                    pass.set_pipeline(&self.shadow_line_pipe);
                    pass.set_bind_group(0, &self.shadow_bind, &[]);
                    pass.set_vertex_buffer(0, b.slice(..));
                    pass.draw(0..*n, 0..1);
                }
            }
        }
        {
            let (view, resolve) = match &t.msaa_view {
                Some(m) => (m, Some(&t.resolve_view)),
                None => (&t.resolve_view, None),
            };
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("homeup-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(self.clear), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.bind, &[]);
            pass.set_pipeline(&self.bg_pipe);
            pass.draw(0..3, 0..1);
            for l in &self.bg_layers {
                pass.set_pipeline(&self.wm_pipe);
                pass.set_bind_group(1, &l.bind, &[]);
                pass.draw(0..3, 0..1);
            }
            for buf in [&self.tri_buf, &self.nocast_buf] {
                if let Some((b, n)) = buf {
                    pass.set_pipeline(&self.face_pipe);
                    pass.set_vertex_buffer(0, b.slice(..));
                    pass.draw(0..*n, 0..1);
                }
            }
            if let Some(g) = &ground {
                pass.set_pipeline(&self.ground_pipe);
                pass.set_vertex_buffer(0, g.slice(..));
                pass.draw(0..6, 0..1);
            }
            if let Some((b, n)) = &self.line_buf {
                pass.set_pipeline(&self.line_pipe);
                pass.set_vertex_buffer(0, b.slice(..));
                pass.draw(0..*n, 0..1);
                if fp.back_edges {
                    pass.set_pipeline(&self.line_back_pipe);
                    pass.draw(0..*n, 0..1);
                }
            }
            if let Some((b, n)) = &self.wide_buf {
                pass.set_pipeline(&self.wide_pipe);
                pass.set_vertex_buffer(0, b.slice(..));
                pass.draw(0..*n, 0..1);
                if fp.back_edges {
                    pass.set_pipeline(&self.wide_back_pipe);
                    pass.draw(0..*n, 0..1);
                }
            }
            if let Some((b, n)) = &self.glass_buf {
                pass.set_pipeline(&self.glass_pipe);
                pass.set_vertex_buffer(0, b.slice(..));
                pass.draw(0..*n, 0..1);
            }
        }
        if fp.ao.0 > 0.0 || fp.profiles > 0.0 {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("homeup-post-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.resolve_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.post_pipe);
            pass.set_bind_group(0, &t.post_bind, &[]);
            pass.draw(0..3, 0..1);
        }
        rs.queue.submit([enc.finish()]);
    }
}

#[cfg(test)]
mod tests {
    fn validate(src: &str) -> naga::Module {
        let module = naga::front::wgsl::parse_str(src).expect("WGSL parse");
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&module).expect("WGSL validation");
        module
    }

    #[test]
    fn shaders_are_valid_wgsl() {
        let module = validate(super::SHADER);
        for ep in ["vs", "vs_line", "vs_wide", "fs_wm", "fs_face", "fs_line", "fs_line_back", "fs_ground", "vs_shadow", "vs_bg", "fs_bg"] {
            assert!(module.entry_points.iter().any(|e| e.name == ep), "missing entry point {ep}");
        }
        for t in ["texture_depth_multisampled_2d", "texture_depth_2d"] {
            let m = validate(&super::POST_SHADER.replace("DEPTH_TYPE", t));
            assert!(m.entry_points.iter().any(|e| e.name == "fs_post"));
        }
    }
}
