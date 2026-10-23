//! Geometric core: vertices, edges, faces (with holes), instances, and the
//! operations that keep faces consistent with the edge graph (SketchUp-like).

use glam::{DMat4, DVec2, DVec3};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Vertex merge tolerance (meters).
pub const MERGE_EPS: f64 = 1e-5;
/// Point-on-plane tolerance (meters).
pub const PLANE_EPS: f64 = 1e-5;

pub type VId = u32;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Edge {
    pub v: [VId; 2],
    /// Soft edges are not drawn and smooth the shading across them.
    #[serde(default)]
    pub soft: bool,
    #[serde(default)]
    pub hidden: bool,
    /// Edges of the same curve (circle, arc, polygon) are selected together.
    #[serde(default)]
    pub curve: Option<u32>,
    /// Smooth curve (circle/arc): extrusions get soft side edges.
    #[serde(default)]
    pub arc: bool,
    /// Tag of the edge itself (SketchUp files tag edges individually).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<usize>,
}

impl Edge {
    pub fn new(a: VId, b: VId) -> Edge {
        Edge { v: [a, b], soft: false, hidden: false, curve: None, arc: false, tag: None }
    }
    pub fn same(&self, a: VId, b: VId) -> bool {
        (self.v[0] == a && self.v[1] == b) || (self.v[0] == b && self.v[1] == a)
    }
    fn key(&self) -> (VId, VId) {
        (self.v[0].min(self.v[1]), self.v[0].max(self.v[1]))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Face {
    /// Outer loop, counter-clockwise when seen from the front (normal side).
    pub outer: Vec<VId>,
    /// Holes, clockwise when seen from the front.
    pub holes: Vec<Vec<VId>>,
    pub normal: DVec3,
    #[serde(default)]
    pub material: Option<usize>,
    #[serde(default)]
    pub back_material: Option<usize>,
    #[serde(default)]
    pub tag: Option<usize>,
    #[serde(default)]
    pub hidden: bool,
    /// Explicit texture coordinates for every loop vertex (outer then holes), from imports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uvs: Option<Vec<[f32; 2]>>,
    /// Projective weight per loop vertex (distorted textures, photo projection):
    /// the texture coordinate is `uvs[k] / uvq[k]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uvq: Option<Vec<f32>>,
}

impl Face {
    /// UV of loop vertex `k` (concatenated loops), if the face carries valid UVs.
    pub fn uv_at(&self, k: usize) -> Option<[f32; 2]> {
        let n: usize = self.loops().map(|l| l.len()).sum();
        self.uvs.as_ref().filter(|u| u.len() == n).map(|u| u[k])
    }

    /// Projective weight of loop vertex `k` (1 when the mapping is affine).
    pub fn uvq_at(&self, k: usize) -> f32 {
        let n: usize = self.loops().map(|l| l.len()).sum();
        self.uvq.as_ref().filter(|u| u.len() == n && self.uv_at(k).is_some()).map(|u| u[k]).unwrap_or(1.0)
    }

    pub fn loops(&self) -> impl Iterator<Item = &Vec<VId>> {
        std::iter::once(&self.outer).chain(self.holes.iter())
    }
    pub fn loops_mut(&mut self) -> impl Iterator<Item = &mut Vec<VId>> {
        std::iter::once(&mut self.outer).chain(self.holes.iter_mut())
    }
    pub fn has_edge(&self, a: VId, b: VId) -> bool {
        self.loops().any(|l| loop_has_edge(l, a, b))
    }
    pub fn boundary_edges(&self) -> Vec<(VId, VId)> {
        let mut out = Vec::new();
        for l in self.loops() {
            for i in 0..l.len() {
                out.push((l[i], l[(i + 1) % l.len()]));
            }
        }
        out
    }
    pub fn reverse(&mut self) {
        for l in self.loops_mut() {
            l.reverse();
        }
        self.normal = -self.normal;
    }
}

fn loop_has_edge(l: &[VId], a: VId, b: VId) -> bool {
    let n = l.len();
    (0..n).any(|i| {
        let (x, y) = (l[i], l[(i + 1) % n]);
        (x == a && y == b) || (x == b && y == a)
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Instance {
    pub def: usize,
    pub xf: DMat4,
    #[serde(default)]
    pub material: Option<usize>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub tag: Option<usize>,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default = "yes")]
    pub cast_shadows: bool,
    /// Locked groups/components cannot be moved, erased or edited.
    #[serde(default)]
    pub locked: bool,
}

fn yes() -> bool {
    true
}

impl Default for Instance {
    fn default() -> Self {
        Instance { def: 0, xf: DMat4::IDENTITY, material: None, name: String::new(), tag: None, hidden: false, cast_shadows: true, locked: false }
    }
}

/// A selection inside one geometry context.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sel {
    pub faces: HashSet<usize>,
    pub edges: HashSet<usize>,
    pub insts: HashSet<usize>,
}

impl Sel {
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty() && self.edges.is_empty() && self.insts.is_empty()
    }
    pub fn clear(&mut self) {
        self.faces.clear();
        self.edges.clear();
        self.insts.clear();
    }
    pub fn remap(&self, em: &[Option<usize>], fm: &[Option<usize>]) -> Sel {
        Sel {
            faces: self.faces.iter().filter_map(|f| fm.get(*f).copied().flatten()).collect(),
            edges: self.edges.iter().filter_map(|e| em.get(*e).copied().flatten()).collect(),
            insts: self.insts.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Plane {
    pub n: DVec3,
    pub d: f64,
}

impl Plane {
    /// Plane with a canonical normal orientation (so equal planes compare equal).
    pub fn new(n: DVec3, p: DVec3) -> Plane {
        let n = canonical(n.normalize());
        Plane { n, d: n.dot(p) }
    }
    pub fn from_points(a: DVec3, b: DVec3, c: DVec3) -> Option<Plane> {
        let n = (b - a).cross(c - a);
        if n.length() < 1e-10 { None } else { Some(Plane::new(n, a)) }
    }
    pub fn dist(&self, p: DVec3) -> f64 {
        self.n.dot(p) - self.d
    }
    pub fn contains(&self, p: DVec3) -> bool {
        self.dist(p).abs() < PLANE_EPS
    }
    pub fn same(&self, o: &Plane) -> bool {
        (self.n - o.n).length() < 1e-6 && (self.d - o.d).abs() < PLANE_EPS
    }
    pub fn project(&self, p: DVec3) -> DVec3 {
        p - self.n * self.dist(p)
    }
    /// Orthonormal basis (u, v) with u × v = n.
    pub fn basis(&self) -> (DVec3, DVec3) {
        basis_for(self.n)
    }
    pub fn intersect_ray(&self, o: DVec3, dir: DVec3) -> Option<DVec3> {
        let denom = self.n.dot(dir);
        if denom.abs() < 1e-12 {
            return None;
        }
        let t = (self.d - self.n.dot(o)) / denom;
        if t < 0.0 { None } else { Some(o + dir * t) }
    }
}

/// Orthonormal in-plane basis for a normal, preferring world axes.
pub fn basis_for(n: DVec3) -> (DVec3, DVec3) {
    let n = n.normalize();
    let u = if n.z.abs() > 0.999 {
        DVec3::X
    } else if n.x.abs() > 0.999 {
        DVec3::Y
    } else if n.y.abs() > 0.999 {
        DVec3::X
    } else if n.z.abs() < 1e-9 {
        DVec3::Z
    } else {
        n.any_orthonormal_vector()
    };
    let u = (u - n * n.dot(u)).normalize();
    (u, n.cross(u))
}

fn canonical(n: DVec3) -> DVec3 {
    let c = if n.z.abs() > 1e-9 {
        n.z
    } else if n.y.abs() > 1e-9 {
        n.y
    } else {
        n.x
    };
    if c < 0.0 { -n } else { n }
}

pub fn signed_area(p: &[DVec2]) -> f64 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (q, r) = (p[i], p[(i + 1) % p.len()]);
        a += q.x * r.y - r.x * q.y;
    }
    a * 0.5
}

pub fn point_in_poly(pt: DVec2, poly: &[DVec2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > pt.y) != (b.y > pt.y) && pt.x < (b.x - a.x) * (pt.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Newell normal of a 3D polygon (not normalized).
pub fn newell(pts: &[DVec3]) -> DVec3 {
    let mut n = DVec3::ZERO;
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        n.x += (a.y - b.y) * (a.z + b.z);
        n.y += (a.z - b.z) * (a.x + b.x);
        n.z += (a.x - b.x) * (a.y + b.y);
    }
    n
}

/// Triangulate a 2D polygon with holes. Returns index triples into the
/// concatenation `outer ++ holes...`.
pub fn triangulate_2d(outer: &[DVec2], holes: &[Vec<DVec2>]) -> Vec<[usize; 3]> {
    let mut data: Vec<f64> = Vec::new();
    let mut hole_idx = Vec::new();
    for p in outer {
        data.extend([p.x, p.y]);
    }
    for h in holes {
        hole_idx.push(data.len() / 2);
        for p in h {
            data.extend([p.x, p.y]);
        }
    }
    match earcutr::earcut(&data, &hole_idx, 2) {
        Ok(ix) => ix.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
        Err(_) => Vec::new(),
    }
}

/// Closest parameters (s, t) between segments p0p1 and q0q1.
pub fn segment_closest(p0: DVec3, p1: DVec3, q0: DVec3, q1: DVec3) -> Option<(f64, f64)> {
    let d1 = p1 - p0;
    let d2 = q1 - q0;
    let r = p0 - q0;
    let (a, e, f) = (d1.dot(d1), d2.dot(d2), d2.dot(r));
    let c = d1.dot(r);
    let b = d1.dot(d2);
    let denom = a * e - b * b;
    if a < 1e-18 || e < 1e-18 || denom.abs() < 1e-14 * a * e {
        return None;
    }
    let s = ((b * f - c * e) / denom).clamp(0.0, 1.0);
    let t = ((b * s + f) / e).clamp(0.0, 1.0);
    let s = ((b * t - c) / a).clamp(0.0, 1.0);
    Some((s, t))
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Geom {
    pub verts: Vec<DVec3>,
    pub edges: Vec<Edge>,
    pub faces: Vec<Face>,
    #[serde(default)]
    pub instances: Vec<Instance>,
    #[serde(default)]
    pub next_curve: u32,
}

pub struct PushPullOutcome {
    pub through_cut: bool,
}

impl Geom {
    pub fn p(&self, v: VId) -> DVec3 {
        self.verts[v as usize]
    }

    pub fn face_plane(&self, f: &Face) -> Plane {
        Plane::new(f.normal, self.p(f.outer[0]))
    }

    pub fn find_edge(&self, a: VId, b: VId) -> Option<usize> {
        self.edges.iter().position(|e| e.same(a, b))
    }

    pub fn new_curve(&mut self) -> u32 {
        self.next_curve += 1;
        self.next_curve
    }

    /// Bounds of this geometry's own vertices (instances excluded).
    #[allow(dead_code)]
    pub fn own_bounds(&self) -> Option<(DVec3, DVec3)> {
        let used: HashSet<VId> = self
            .edges
            .iter()
            .flat_map(|e| e.v)
            .chain(self.faces.iter().flat_map(|f| f.outer.iter().copied()))
            .collect();
        if used.is_empty() {
            return None;
        }
        let (mut lo, mut hi) = (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN));
        for v in used {
            lo = lo.min(self.p(v));
            hi = hi.max(self.p(v));
        }
        Some((lo, hi))
    }

    /// Face triangles as vertex-id triples, wound counter-clockwise around the face normal.
    pub fn triangulate_face(&self, f: &Face) -> Vec<[VId; 3]> {
        if f.outer.len() == 3 && f.holes.is_empty() {
            return vec![[f.outer[0], f.outer[1], f.outer[2]]];
        }
        let plane = Plane::new(f.normal, self.p(f.outer[0]));
        let (u, v) = plane.basis();
        let to2 = |id: &VId| {
            let p = self.p(*id);
            DVec2::new(p.dot(u), p.dot(v))
        };
        let outer: Vec<DVec2> = f.outer.iter().map(to2).collect();
        let holes: Vec<Vec<DVec2>> = f.holes.iter().map(|h| h.iter().map(to2).collect()).collect();
        let ids: Vec<VId> = f.loops().flat_map(|l| l.iter().copied()).collect();
        triangulate_2d(&outer, &holes)
            .into_iter()
            .map(|t| {
                let tri = [ids[t[0]], ids[t[1]], ids[t[2]]];
                let n = (self.p(tri[1]) - self.p(tri[0])).cross(self.p(tri[2]) - self.p(tri[0]));
                if n.dot(f.normal) < 0.0 { [tri[0], tri[2], tri[1]] } else { tri }
            })
            .collect()
    }

    /// A point strictly inside the face (centroid of its largest triangle).
    pub fn face_sample_point(&self, f: &Face) -> DVec3 {
        let mut best = (0.0, self.p(f.outer[0]));
        for t in self.triangulate_face(f) {
            let (a, b, c) = (self.p(t[0]), self.p(t[1]), self.p(t[2]));
            let area = (b - a).cross(c - a).length();
            if area > best.0 {
                best = (area, (a + b + c) / 3.0);
            }
        }
        best.1
    }

    pub fn face_area(&self, f: &Face) -> f64 {
        self.triangulate_face(f)
            .iter()
            .map(|t| (self.p(t[1]) - self.p(t[0])).cross(self.p(t[2]) - self.p(t[0])).length() * 0.5)
            .sum()
    }

    /// Whether `p` (assumed on the face plane) lies inside the face region.
    pub fn face_contains(&self, f: &Face, p: DVec3) -> bool {
        let plane = self.face_plane(f);
        let (u, v) = plane.basis();
        let to2 = |q: DVec3| DVec2::new(q.dot(u), q.dot(v));
        let pt = to2(p);
        let poly = |l: &Vec<VId>| l.iter().map(|id| to2(self.p(*id))).collect::<Vec<_>>();
        point_in_poly(pt, &poly(&f.outer)) && !f.holes.iter().any(|h| point_in_poly(pt, &poly(h)))
    }

    fn used_vertices(&self) -> HashSet<VId> {
        let mut used = HashSet::new();
        for e in &self.edges {
            used.insert(e.v[0]);
            used.insert(e.v[1]);
        }
        used
    }

    // ------------------------------------------------------------------ editing

    /// Find or create a vertex at `p`, splitting an edge if `p` lies on one.
    pub fn add_vertex(&mut self, p: DVec3) -> VId {
        if let Some(v) = self.used_vertices().into_iter().find(|v| self.p(*v).distance(p) < MERGE_EPS) {
            return v;
        }
        let id = self.verts.len() as VId;
        self.verts.push(p);
        for ei in 0..self.edges.len() {
            let [a, b] = self.edges[ei].v;
            let (pa, pb) = (self.p(a), self.p(b));
            let ab = pb - pa;
            let t = (p - pa).dot(ab) / ab.length_squared();
            if t > 1e-9 && t < 1.0 - 1e-9 && (pa + ab * t).distance(p) < MERGE_EPS {
                self.split_edge(ei, id);
                break;
            }
        }
        id
    }

    pub(crate) fn split_edge_pub(&mut self, ei: usize, v: VId) {
        self.split_edge(ei, v);
    }

    fn split_edge(&mut self, ei: usize, v: VId) {
        let [a, b] = self.edges[ei].v;
        let mut second = self.edges[ei].clone();
        self.edges[ei].v = [a, v];
        second.v = [v, b];
        self.edges.push(second);
        for f in &mut self.faces {
            for l in f.loops_mut() {
                let n = l.len();
                if let Some(i) = (0..n).find(|&i| {
                    let (x, y) = (l[i], l[(i + 1) % n]);
                    (x == a && y == b) || (x == b && y == a)
                }) {
                    l.insert(i + 1, v);
                }
            }
        }
    }

    fn push_edge(&mut self, e: Edge) -> bool {
        let [a, b] = e.v;
        if a == b || self.find_edge(a, b).is_some() {
            return false;
        }
        self.edges.push(e);
        true
    }

    fn neighbors(&self, v: VId) -> Vec<VId> {
        self.edges
            .iter()
            .filter_map(|e| {
                if e.v[0] == v {
                    Some(e.v[1])
                } else if e.v[1] == v {
                    Some(e.v[0])
                } else {
                    None
                }
            })
            .collect()
    }

    fn candidate_planes(&self, new_edges: &[(VId, VId)]) -> Vec<Plane> {
        let mut planes: Vec<Plane> = Vec::new();
        let add = |pl: Plane, planes: &mut Vec<Plane>| {
            if !planes.iter().any(|q| q.same(&pl)) {
                planes.push(pl);
            }
        };
        for &(a, b) in new_edges {
            let (pa, pb) = (self.p(a), self.p(b));
            for f in &self.faces {
                let pl = self.face_plane(f);
                if pl.contains(pa) && pl.contains(pb) {
                    add(pl, &mut planes);
                }
            }
            for c in self.neighbors(a).into_iter().chain(self.neighbors(b)) {
                if let Some(pl) = Plane::from_points(pa, pb, self.p(c)) {
                    add(pl, &mut planes);
                }
            }
        }
        planes
    }

    /// Insert segments, splitting at crossings with existing edges/vertices,
    /// and rebuild faces on the affected planes. Returns true if a new face closed.
    pub fn add_segments(&mut self, segs: &[(DVec3, DVec3)], curve: Option<u32>, arc: bool, eye: DVec3) -> bool {
        let mut new_pairs: Vec<(VId, VId)> = Vec::new();
        for &(p, q) in segs {
            let a = self.add_vertex(p);
            let b = self.add_vertex(q);
            if a == b {
                continue;
            }
            let (pa, pb) = (self.p(a), self.p(b));
            let ab = pb - pa;
            let len2 = ab.length_squared();
            let mut pts: Vec<(f64, VId)> = vec![(0.0, a), (1.0, b)];
            for v in self.used_vertices() {
                if v == a || v == b {
                    continue;
                }
                let pv = self.p(v);
                let t = (pv - pa).dot(ab) / len2;
                if t > 1e-9 && t < 1.0 - 1e-9 && (pa + ab * t).distance(pv) < MERGE_EPS {
                    pts.push((t, v));
                }
            }
            let mut crossings: Vec<(f64, DVec3)> = Vec::new();
            for e in &self.edges {
                let [c, d] = e.v;
                if c == a || c == b || d == a || d == b {
                    continue;
                }
                let (pc, pd) = (self.p(c), self.p(d));
                if let Some((s, t)) = segment_closest(pa, pb, pc, pd) {
                    let x = pa + ab * s;
                    let y = pc + (pd - pc) * t;
                    if s > 1e-9 && s < 1.0 - 1e-9 && t > 1e-9 && t < 1.0 - 1e-9 && x.distance(y) < MERGE_EPS {
                        crossings.push((s, x));
                    }
                }
            }
            for (s, x) in crossings {
                let v = self.add_vertex(x);
                pts.push((s, v));
            }
            pts.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
            pts.dedup_by_key(|p| p.1);
            for w in pts.windows(2) {
                let (u, v) = (w[0].1, w[1].1);
                if u == v {
                    continue;
                }
                let mut e = Edge::new(u, v);
                e.curve = curve;
                e.arc = arc;
                if !self.push_edge(e) {
                    // an existing edge became part of this curve? keep it as is
                }
                new_pairs.push((u, v));
            }
        }
        if new_pairs.is_empty() {
            return false;
        }
        let planes = self.candidate_planes(&new_pairs);
        let set: HashSet<(VId, VId)> = new_pairs.iter().copied().collect();
        let mut closed = false;
        for pl in planes {
            closed |= self.recompute_plane(&pl, &set, &[], eye);
        }
        closed
    }

    pub fn add_line(&mut self, p: DVec3, q: DVec3, eye: DVec3) -> bool {
        self.add_segments(&[(p, q)], None, false, eye)
    }

    /// Add a closed planar polygon (rectangle, circle, polygon tools).
    pub fn add_polygon(&mut self, pts: &[DVec3], curve: Option<u32>, arc: bool, eye: DVec3) -> bool {
        let segs: Vec<(DVec3, DVec3)> = (0..pts.len()).map(|i| (pts[i], pts[(i + 1) % pts.len()])).collect();
        self.add_segments(&segs, curve, arc, eye)
    }

    /// Erase an edge; faces bounded by it vanish (or merge, when coplanar).
    pub fn erase_edge(&mut self, ei: usize, eye: DVec3) {
        let [a, b] = self.edges[ei].v;
        let mut planes: Vec<Plane> = Vec::new();
        for f in self.faces.iter().filter(|f| f.has_edge(a, b)) {
            let pl = self.face_plane(f);
            if !planes.iter().any(|q| q.same(&pl)) {
                planes.push(pl);
            }
        }
        self.edges.remove(ei);
        for pl in planes {
            self.recompute_plane(&pl, &HashSet::new(), &[], eye);
        }
        self.faces.retain(|f| !f.has_edge(a, b));
    }

    /// Rebuild the faces lying on `plane` from the edge graph.
    /// Returns true if a region bounded by one of `new_edges` became a face.
    pub fn recompute_plane(&mut self, plane: &Plane, new_edges: &HashSet<(VId, VId)>, forced_empty: &[DVec3], eye: DVec3) -> bool {
        let is_new = |a: VId, b: VId| new_edges.contains(&(a, b)) || new_edges.contains(&(b, a));
        let (u, v) = plane.basis();
        let to2 = |p: DVec3| DVec2::new(p.dot(u), p.dot(v));

        let mut old: Vec<Face> = Vec::new();
        let mut kept: Vec<Face> = Vec::new();
        for f in self.faces.drain(..) {
            let fp = Plane::new(f.normal, self.verts[f.outer[0] as usize]);
            if fp.same(plane) {
                old.push(f);
            } else {
                kept.push(f);
            }
        }
        self.faces = kept;
        let old2: Vec<(Vec<DVec2>, Vec<Vec<DVec2>>, DVec3, Face)> = old
            .iter()
            .map(|f| {
                (
                    f.outer.iter().map(|i| to2(self.p(*i))).collect(),
                    f.holes.iter().map(|h| h.iter().map(|i| to2(self.p(*i))).collect()).collect(),
                    f.normal,
                    f.clone(),
                )
            })
            .collect();

        let mut edges: Vec<(VId, VId)> = self
            .edges
            .iter()
            .filter(|e| plane.contains(self.p(e.v[0])) && plane.contains(self.p(e.v[1])))
            .map(|e| (e.v[0], e.v[1]))
            .collect();

        let mut cycles: Vec<Vec<VId>>;
        let mut guard = 0;
        loop {
            guard += 1;
            let mut adj: HashMap<VId, Vec<VId>> = HashMap::new();
            for &(a, b) in &edges {
                adj.entry(a).or_default().push(b);
                adj.entry(b).or_default().push(a);
            }
            for (k, nb) in adj.iter_mut() {
                let c = to2(self.p(*k));
                nb.sort_by(|x, y| {
                    let ax = (to2(self.p(*x)) - c).to_angle();
                    let ay = (to2(self.p(*y)) - c).to_angle();
                    ax.partial_cmp(&ay).unwrap()
                });
            }
            let mut visited: HashSet<(VId, VId)> = HashSet::new();
            cycles = Vec::new();
            let mut bad: HashSet<(VId, VId)> = HashSet::new();
            for &(a, b) in &edges {
                for start in [(a, b), (b, a)] {
                    if visited.contains(&start) {
                        continue;
                    }
                    let mut cyc = Vec::new();
                    let mut hes: HashSet<(VId, VId)> = HashSet::new();
                    let mut cur = start;
                    for _ in 0..100_000 {
                        visited.insert(cur);
                        hes.insert(cur);
                        cyc.push(cur.0);
                        let nb = &adj[&cur.1];
                        let i = nb.iter().position(|x| *x == cur.0).unwrap();
                        let next = nb[(i + nb.len() - 1) % nb.len()];
                        cur = (cur.1, next);
                        if cur == start {
                            break;
                        }
                    }
                    for &(x, y) in &hes {
                        if hes.contains(&(y, x)) {
                            bad.insert((x.min(y), x.max(y)));
                        }
                    }
                    cycles.push(cyc);
                }
            }
            if bad.is_empty() || guard > 64 {
                break;
            }
            edges.retain(|&(a, b)| !bad.contains(&(a.min(b), a.max(b))));
        }

        let mut parent: HashMap<VId, VId> = HashMap::new();
        fn find(p: &mut HashMap<VId, VId>, x: VId) -> VId {
            let px = *p.get(&x).unwrap_or(&x);
            if px == x {
                x
            } else {
                let r = find(p, px);
                p.insert(x, r);
                r
            }
        }
        for &(a, b) in &edges {
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            if ra != rb {
                parent.insert(ra, rb);
            }
        }

        struct Cyc {
            ids: Vec<VId>,
            pts: Vec<DVec2>,
            area: f64,
            comp: VId,
        }
        let cycs: Vec<Cyc> = cycles
            .into_iter()
            .map(|ids| {
                let pts: Vec<DVec2> = ids.iter().map(|i| to2(self.p(*i))).collect();
                let area = signed_area(&pts);
                let comp = find(&mut parent, ids[0]);
                Cyc { ids, pts, area, comp }
            })
            .filter(|c| c.area.abs() > 1e-12)
            .collect();

        let outers: Vec<usize> = (0..cycs.len()).filter(|&i| cycs[i].area > 0.0).collect();
        let mut holes_of: HashMap<usize, Vec<usize>> = HashMap::new();
        for h in (0..cycs.len()).filter(|&i| cycs[i].area < 0.0) {
            let probe = cycs[h].pts[0];
            let host = outers
                .iter()
                .copied()
                .filter(|&o| cycs[o].comp != cycs[h].comp && point_in_poly(probe, &cycs[o].pts))
                .min_by(|&x, &y| cycs[x].area.partial_cmp(&cycs[y].area).unwrap());
            if let Some(o) = host {
                holes_of.entry(o).or_default().push(h);
            }
        }

        let forced2: Vec<DVec2> = forced_empty.iter().map(|p| to2(*p)).collect();
        let mut closed_new = false;
        for o in outers {
            let hs = holes_of.remove(&o).unwrap_or_default();
            let hole_pts: Vec<Vec<DVec2>> = hs.iter().map(|h| cycs[*h].pts.clone()).collect();
            let inside = |pt: DVec2| point_in_poly(pt, &cycs[o].pts) && !hole_pts.iter().any(|h| point_in_poly(pt, h));
            let tris = triangulate_2d(&cycs[o].pts, &hole_pts);
            let all: Vec<DVec2> = cycs[o].pts.iter().chain(hole_pts.iter().flatten()).copied().collect();
            let mut sample = None;
            let mut best = 0.0;
            for t in &tris {
                let (a, b, c) = (all[t[0]], all[t[1]], all[t[2]]);
                let ar = ((b - a).perp_dot(c - a)).abs();
                if ar > best {
                    best = ar;
                    sample = Some((a + b + c) / 3.0);
                }
            }
            let Some(sample) = sample else { continue };
            if forced2.iter().any(|p| inside(*p)) {
                continue;
            }
            let covering = old2
                .iter()
                .find(|(ou, oh, _, _)| point_in_poly(sample, ou) && !oh.iter().any(|h| point_in_poly(sample, h)));
            let mut loops: Vec<&Vec<VId>> = vec![&cycs[o].ids];
            loops.extend(hs.iter().map(|h| &cycs[*h].ids));
            let touches_new = loops.iter().any(|l| (0..l.len()).any(|i| is_new(l[i], l[(i + 1) % l.len()])));
            let (normal, attrs) = match covering {
                Some((_, _, n, m)) => (*n, Some(m.clone())),
                None if touches_new => {
                    closed_new = true;
                    let n = if plane.n.z.abs() > 0.999 {
                        DVec3::Z
                    } else {
                        let p0 = self.p(cycs[o].ids[0]);
                        if plane.n.dot(eye - p0) >= 0.0 { plane.n } else { -plane.n }
                    };
                    (n, None)
                }
                None => continue,
            };
            let flip = normal.dot(plane.n) < 0.0;
            let fix = |l: &Vec<VId>| {
                let mut l = l.clone();
                if flip {
                    l.reverse();
                }
                l
            };
            self.faces.push(Face {
                outer: fix(&cycs[o].ids),
                holes: hs.iter().map(|h| fix(&cycs[*h].ids)).collect(),
                normal: if flip { -plane.n } else { plane.n },
                material: attrs.as_ref().and_then(|a| a.material),
                back_material: attrs.as_ref().and_then(|a| a.back_material),
                tag: attrs.as_ref().and_then(|a| a.tag),
                hidden: attrs.as_ref().is_some_and(|a| a.hidden),
                uvs: None,
                uvq: None,
            });
        }
        closed_new
    }

    /// Push/Pull face `fi` by `d` meters along its normal.
    pub fn push_pull(&mut self, fi: usize, d: f64, force_new: bool, eye: DVec3) -> PushPullOutcome {
        let mut out = PushPullOutcome { through_cut: false };
        if d.abs() < 1e-9 || fi >= self.faces.len() {
            return out;
        }
        let f = self.faces[fi].clone();
        let n = f.normal;

        // Per loop: "stretch" when every edge only borders perpendicular faces
        // (those faces stretch with the moved vertices), else "wall".
        let mut any_neighbor = false;
        let mut stretch: Vec<bool> = Vec::new();
        for l in f.loops() {
            let mut loop_stretch = !force_new;
            for i in 0..l.len() {
                let (a, b) = (l[i], l[(i + 1) % l.len()]);
                let mut has = false;
                for (j, g) in self.faces.iter().enumerate() {
                    if j != fi && g.has_edge(a, b) {
                        has = true;
                        if g.normal.dot(n).abs() > 1e-6 {
                            loop_stretch = false;
                        }
                    }
                }
                any_neighbor |= has;
                if !has {
                    loop_stretch = false;
                }
            }
            stretch.push(loop_stretch);
        }
        let free = !any_neighbor;

        if stretch.iter().all(|s| *s) {
            let ids: HashSet<VId> = f.loops().flatten().copied().collect();
            for v in ids {
                self.verts[v as usize] += n * d;
            }
            return out;
        }

        // edge flags of the face boundary (before mutation)
        let flags: HashMap<(VId, VId), Edge> = f
            .boundary_edges()
            .iter()
            .filter_map(|&(a, b)| self.find_edge(a, b).map(|ei| ((a.min(b), a.max(b)), self.edges[ei].clone())))
            .collect();
        let flag = |a: VId, b: VId| flags.get(&(a.min(b), a.max(b))).cloned();
        let mut curve_map: HashMap<u32, u32> = HashMap::new();

        let mut map: HashMap<VId, VId> = HashMap::new();
        for (l, s) in f.loops().zip(&stretch) {
            if *s {
                for &v in l {
                    map.insert(v, v);
                }
            }
        }
        let moved: Vec<VId> = map.keys().copied().collect();
        for v in moved {
            self.verts[v as usize] += n * d;
        }
        for (l, s) in f.loops().zip(&stretch) {
            if !*s {
                for &v in l {
                    map.entry(v).or_insert_with(|| {
                        self.verts.push(self.verts[v as usize] + n * d);
                        (self.verts.len() - 1) as VId
                    });
                }
            }
        }
        let mut new_faces = Vec::new();
        let mut new_edges: Vec<Edge> = Vec::new();
        for (l, s) in f.loops().zip(&stretch) {
            if *s {
                continue;
            }
            let len = l.len();
            for i in 0..len {
                let (a, b) = (l[i], l[(i + 1) % len]);
                let prev = l[(i + len - 1) % len];
                let (a2, b2) = (map[&a], map[&b]);
                let outward = (self.p(b) - self.p(a)).cross(n);
                let desired = if !free && d < 0.0 { -outward } else { outward };
                let mut quad = vec![a, b, b2, a2];
                let pts: Vec<DVec3> = quad.iter().map(|v| self.p(*v)).collect();
                if newell(&pts).dot(desired) < 0.0 {
                    quad.reverse();
                }
                new_faces.push(Face { outer: quad, holes: vec![], normal: desired.normalize(), material: None, ..Default::default() });
                // cap edge copies the curve membership
                let fe = flag(a, b);
                let mut cap_e = Edge::new(a2, b2);
                if let Some(fe) = &fe {
                    cap_e.arc = fe.arc;
                    cap_e.curve = fe.curve.map(|c| *curve_map.entry(c).or_insert_with(|| {
                        self.next_curve += 1;
                        self.next_curve
                    }));
                }
                new_edges.push(cap_e);
                // vertical edge at a: soft when both adjacent loop edges are the same smooth curve
                let fp = flag(prev, a);
                let mut vert_e = Edge::new(a, a2);
                if let (Some(x), Some(y)) = (&fp, &fe) {
                    if x.arc && y.arc && x.curve.is_some() && x.curve == y.curve {
                        vert_e.soft = true;
                    }
                }
                new_edges.push(vert_e);
            }
        }
        let cap_normal = if free { n * d.signum() } else { n };
        let remap = |l: &Vec<VId>| {
            let mut m: Vec<VId> = l.iter().map(|v| map[v]).collect();
            if cap_normal.dot(n) < 0.0 {
                m.reverse();
            }
            m
        };
        let cap = Face { outer: remap(&f.outer), holes: f.holes.iter().map(remap).collect(), normal: cap_normal, ..f.clone() };
        for e in new_edges {
            self.push_edge(e);
        }
        self.faces.remove(fi);
        if free {
            let base_n = -n * d.signum();
            let rev = |l: &Vec<VId>| {
                let mut m = l.clone();
                if base_n.dot(n) < 0.0 {
                    m.reverse();
                }
                m
            };
            self.faces.push(Face { outer: rev(&f.outer), holes: f.holes.iter().map(rev).collect(), normal: base_n, ..f.clone() });
        }
        self.faces.extend(new_faces);

        if !free && d < 0.0 {
            let cap_plane = self.face_plane(&cap);
            let sample = self.face_sample_point(&cap);
            let hit = self.faces.iter().any(|g| {
                g.normal.dot(cap.normal) < -0.999 && self.face_plane(g).same(&cap_plane) && self.face_contains(g, sample)
            });
            if hit {
                out.through_cut = true;
                self.recompute_plane(&cap_plane, &HashSet::new(), &[sample], eye);
                return out;
            }
        }
        self.faces.push(cap);
        out
    }

    /// Distances (along the face normal) at which the face would meet parallel faces.
    pub fn parallel_face_offsets(&self, fi: usize) -> Vec<f64> {
        let f = &self.faces[fi];
        let p0 = self.p(f.outer[0]);
        self.faces
            .iter()
            .enumerate()
            .filter(|(j, g)| *j != fi && g.normal.dot(f.normal).abs() > 0.9999)
            .map(|(_, g)| (self.p(g.outer[0]) - p0).dot(f.normal))
            .filter(|t| t.abs() > 1e-6)
            .collect()
    }

    pub fn delete_sel(&mut self, sel: &Sel, eye: DVec3) {
        let pairs: Vec<[VId; 2]> = sel.edges.iter().filter(|e| **e < self.edges.len()).map(|e| self.edges[*e].v).collect();
        let mut fi = 0;
        self.faces.retain(|_| {
            fi += 1;
            !sel.faces.contains(&(fi - 1))
        });
        let mut ii = 0;
        self.instances.retain(|_| {
            ii += 1;
            !sel.insts.contains(&(ii - 1))
        });
        for [a, b] in pairs {
            if let Some(ei) = self.find_edge(a, b) {
                self.erase_edge(ei, eye);
            }
        }
    }

    /// Edges of the selection plus the boundary edges of selected faces.
    pub fn sel_edges_full(&self, sel: &Sel) -> HashSet<usize> {
        let mut es = sel.edges.clone();
        for fi in &sel.faces {
            for (a, b) in self.faces[*fi].boundary_edges() {
                if let Some(e) = self.find_edge(a, b) {
                    es.insert(e);
                }
            }
        }
        es
    }

    pub fn sel_vertices(&self, sel: &Sel) -> HashSet<VId> {
        let mut vs = HashSet::new();
        for e in &sel.edges {
            vs.extend(self.edges[*e].v);
        }
        for f in &sel.faces {
            vs.extend(self.faces[*f].loops().flatten().copied());
        }
        vs
    }

    /// Apply `m` to a vertex subset; connected geometry stretches.
    pub fn transform_vertices(&mut self, vs: &HashSet<VId>, m: DMat4) {
        if m.determinant() < 0.0 {
            // mirrored faces keep their front side outward
            for f in &mut self.faces {
                if f.loops().flatten().all(|v| vs.contains(v)) {
                    for l in f.loops_mut() {
                        l.reverse();
                    }
                }
            }
        }
        for v in vs {
            self.verts[*v as usize] = m.transform_point3(self.verts[*v as usize]);
        }
        let affected: Vec<usize> = (0..self.faces.len())
            .filter(|fi| self.faces[*fi].loops().flatten().any(|v| vs.contains(v)))
            .collect();
        for fi in &affected {
            let pts: Vec<DVec3> = self.faces[*fi].outer.iter().map(|v| self.p(*v)).collect();
            let n = newell(&pts);
            if n.length() > 1e-12 {
                self.faces[*fi].normal = n.normalize();
            }
        }
        self.fold_nonplanar(&affected);
    }

    /// Split non-planar faces into triangles joined by soft, hidden edges.
    fn fold_nonplanar(&mut self, idx: &[usize]) {
        let mut replace: Vec<usize> = Vec::new();
        for &fi in idx {
            let f = &self.faces[fi];
            let c = self.p(f.outer[0]);
            let size = f.outer.iter().map(|v| self.p(*v).distance(c)).fold(0.0, f64::max).max(1e-9);
            if f.loops().flatten().any(|v| (self.p(*v) - c).dot(f.normal).abs() > 1e-7 * size.max(1.0)) {
                replace.push(fi);
            }
        }
        if replace.is_empty() {
            return;
        }
        let mut new_faces = Vec::new();
        let mut diag = Vec::new();
        for &fi in &replace {
            let f = self.faces[fi].clone();
            for t in self.triangulate_face(&f) {
                let pts: Vec<DVec3> = t.iter().map(|v| self.p(*v)).collect();
                let n = newell(&pts);
                if n.length() < 1e-14 {
                    continue;
                }
                new_faces.push(Face { outer: t.to_vec(), holes: vec![], normal: n.normalize(), ..f.clone() });
                for i in 0..3 {
                    diag.push((t[i], t[(i + 1) % 3]));
                }
            }
        }
        let set: HashSet<usize> = replace.into_iter().collect();
        let mut i = 0;
        self.faces.retain(|_| {
            i += 1;
            !set.contains(&(i - 1))
        });
        self.faces.extend(new_faces);
        for (a, b) in diag {
            if self.find_edge(a, b).is_none() {
                let mut e = Edge::new(a, b);
                e.soft = true;
                e.hidden = true;
                self.edges.push(e);
            }
        }
    }

    /// Transform the selection in place, then weld. Returns the remapped selection.
    pub fn transform_sel(&mut self, sel: &Sel, m: DMat4, weld: bool) -> Sel {
        let vs = self.sel_vertices(sel);
        self.transform_vertices(&vs, m);
        for i in &sel.insts {
            self.instances[*i].xf = m * self.instances[*i].xf;
        }
        if !weld {
            return sel.clone();
        }
        let (em, fm) = self.weld();
        sel.remap(&em, &fm)
    }

    /// Copy the selection transformed by `m`; returns the selection of the copy.
    pub fn copy_sel(&mut self, sel: &Sel, m: DMat4, weld: bool) -> Sel {
        let mut part = self.subset(sel);
        part.transform_all(m);
        let new = self.append(&part, DMat4::IDENTITY, None);
        if !weld {
            return new;
        }
        let (em, fm) = self.weld();
        new.remap(&em, &fm)
    }

    /// A standalone copy of the selected entities.
    pub fn subset(&self, sel: &Sel) -> Geom {
        let es = self.sel_edges_full(sel);
        let mut g = Geom { next_curve: self.next_curve, ..Default::default() };
        let mut map: HashMap<VId, VId> = HashMap::new();
        let mut mv = |v: VId, g: &mut Geom| {
            *map.entry(v).or_insert_with(|| {
                g.verts.push(self.p(v));
                (g.verts.len() - 1) as VId
            })
        };
        let mut es: Vec<usize> = es.into_iter().collect();
        es.sort_unstable();
        for e in es {
            let mut ne = self.edges[e].clone();
            ne.v = [mv(ne.v[0], &mut g), mv(ne.v[1], &mut g)];
            g.edges.push(ne);
        }
        let mut fs: Vec<usize> = sel.faces.iter().copied().collect();
        fs.sort_unstable();
        for fi in fs {
            let mut f = self.faces[fi].clone();
            for l in f.loops_mut() {
                for v in l.iter_mut() {
                    *v = mv(*v, &mut g);
                }
            }
            g.faces.push(f);
        }
        let mut is: Vec<usize> = sel.insts.iter().copied().collect();
        is.sort_unstable();
        for i in is {
            g.instances.push(self.instances[i].clone());
        }
        g
    }

    /// Remove the selection (edges still bounding remaining faces stay).
    pub fn remove_sel(&mut self, sel: &Sel) {
        let es = self.sel_edges_full(sel);
        let mut i = 0;
        self.faces.retain(|_| {
            i += 1;
            !sel.faces.contains(&(i - 1))
        });
        let mut i = 0;
        self.instances.retain(|_| {
            i += 1;
            !sel.insts.contains(&(i - 1))
        });
        let still: HashSet<(VId, VId)> = self
            .faces
            .iter()
            .flat_map(|f| f.boundary_edges())
            .map(|(a, b)| (a.min(b), a.max(b)))
            .collect();
        let mut i = 0;
        self.edges.retain(|e| {
            i += 1;
            !es.contains(&(i - 1)) || still.contains(&e.key())
        });
    }

    /// Transform every vertex and instance.
    pub fn transform_all(&mut self, m: DMat4) {
        for v in &mut self.verts {
            *v = m.transform_point3(*v);
        }
        let flip = m.determinant() < 0.0;
        for fi in 0..self.faces.len() {
            if flip {
                for l in self.faces[fi].loops_mut() {
                    l.reverse();
                }
            }
            let pts: Vec<DVec3> = self.faces[fi].outer.iter().map(|v| self.verts[*v as usize]).collect();
            let n = newell(&pts);
            if n.length() > 1e-14 {
                self.faces[fi].normal = n.normalize();
            }
        }
        for inst in &mut self.instances {
            inst.xf = m * inst.xf;
        }
    }

    /// Append another geometry transformed by `m`. Faces without material get `mat`.
    pub fn append(&mut self, other: &Geom, m: DMat4, mat: Option<usize>) -> Sel {
        let mut g = other.clone();
        g.transform_all(m);
        let base = self.verts.len() as VId;
        self.verts.extend(g.verts.iter().copied());
        let mut curve_map: HashMap<u32, u32> = HashMap::new();
        let mut sel = Sel::default();
        for mut e in g.edges {
            e.v = [e.v[0] + base, e.v[1] + base];
            e.curve = e.curve.map(|c| *curve_map.entry(c).or_insert_with(|| {
                self.next_curve += 1;
                self.next_curve
            }));
            sel.edges.insert(self.edges.len());
            self.edges.push(e);
        }
        for mut f in g.faces {
            for l in f.loops_mut() {
                for v in l.iter_mut() {
                    *v += base;
                }
            }
            if f.material.is_none() {
                f.material = mat;
            }
            sel.faces.insert(self.faces.len());
            self.faces.push(f);
        }
        for inst in g.instances {
            sel.insts.insert(self.instances.len());
            self.instances.push(inst);
        }
        sel
    }

    /// Merge coincident vertices, drop degenerate/duplicate edges and faces.
    /// Returns (edge index map, face index map).
    pub fn weld(&mut self) -> (Vec<Option<usize>>, Vec<Option<usize>>) {
        let q = |p: DVec3| {
            ((p.x / MERGE_EPS).round() as i64, (p.y / MERGE_EPS).round() as i64, (p.z / MERGE_EPS).round() as i64)
        };
        let mut grid: HashMap<(i64, i64, i64), VId> = HashMap::new();
        let mut remap: Vec<VId> = (0..self.verts.len() as VId).collect();
        let mut used: Vec<VId> = self
            .edges
            .iter()
            .flat_map(|e| e.v)
            .chain(self.faces.iter().flat_map(|f| f.loops().flatten().copied().collect::<Vec<_>>()))
            .collect();
        used.sort_unstable();
        used.dedup();
        for v in used {
            let p = self.p(v);
            let k = q(p);
            let mut found = None;
            'outer: for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        if let Some(&w) = grid.get(&(k.0 + dx, k.1 + dy, k.2 + dz)) {
                            if self.p(w).distance(p) < MERGE_EPS {
                                found = Some(w);
                                break 'outer;
                            }
                        }
                    }
                }
            }
            match found {
                Some(w) => remap[v as usize] = w,
                None => {
                    grid.insert(k, v);
                }
            }
        }
        let mut seen: HashMap<(VId, VId), usize> = HashMap::new();
        let mut edges = Vec::new();
        let mut em = Vec::new();
        for e in self.edges.drain(..) {
            let (a, b) = (remap[e.v[0] as usize], remap[e.v[1] as usize]);
            if a == b {
                em.push(None);
                continue;
            }
            let k = (a.min(b), a.max(b));
            if let Some(&i) = seen.get(&k) {
                em.push(Some(i));
                continue;
            }
            seen.insert(k, edges.len());
            em.push(Some(edges.len()));
            let mut e = e;
            e.v = [a, b];
            edges.push(e);
        }
        self.edges = edges;
        let mut fm = Vec::new();
        let mut faces = Vec::new();
        let mut fseen: HashSet<Vec<VId>> = HashSet::new();
        let clean = |l: &Vec<VId>| {
            let mut out: Vec<VId> = l.iter().map(|v| remap[*v as usize]).collect();
            out.dedup();
            while out.len() > 1 && out.first() == out.last() {
                out.pop();
            }
            out
        };
        for f in self.faces.drain(..) {
            let outer = clean(&f.outer);
            if outer.len() < 3 {
                fm.push(None);
                continue;
            }
            let mut key = outer.clone();
            key.sort_unstable();
            if !fseen.insert(key) {
                fm.push(None);
                continue;
            }
            let holes = f.holes.iter().map(clean).filter(|h| h.len() >= 3).collect();
            fm.push(Some(faces.len()));
            faces.push(Face { outer, holes, normal: f.normal, ..f.clone() });
        }
        self.faces = faces;
        (em, fm)
    }

    /// Every entity connected to the given vertices.
    pub fn connected(&self, seed: &HashSet<VId>) -> Sel {
        let mut seen: HashSet<VId> = seed.clone();
        let mut stack: Vec<VId> = seed.iter().copied().collect();
        let mut adj: HashMap<VId, Vec<VId>> = HashMap::new();
        for e in &self.edges {
            adj.entry(e.v[0]).or_default().push(e.v[1]);
            adj.entry(e.v[1]).or_default().push(e.v[0]);
        }
        for f in &self.faces {
            let vs: Vec<VId> = f.loops().flatten().copied().collect();
            for w in vs.windows(2) {
                adj.entry(w[0]).or_default().push(w[1]);
                adj.entry(w[1]).or_default().push(w[0]);
            }
        }
        while let Some(v) = stack.pop() {
            for w in adj.get(&v).into_iter().flatten() {
                if seen.insert(*w) {
                    stack.push(*w);
                }
            }
        }
        Sel {
            faces: (0..self.faces.len()).filter(|f| self.faces[*f].outer.iter().all(|v| seen.contains(v))).collect(),
            edges: (0..self.edges.len()).filter(|e| self.edges[*e].v.iter().all(|v| seen.contains(v))).collect(),
            insts: HashSet::new(),
        }
    }

    /// All edges of the same curve as `ei`.
    pub fn curve_edges(&self, ei: usize) -> Vec<usize> {
        match self.edges[ei].curve {
            Some(c) => (0..self.edges.len()).filter(|e| self.edges[*e].curve == Some(c)).collect(),
            None => vec![ei],
        }
    }

    /// Outer loop of face `fi` offset inward by `d` (outward when negative).
    pub fn offset_loop(&self, fi: usize, d: f64) -> Vec<DVec3> {
        let f = &self.faces[fi];
        let pts: Vec<DVec3> = f.outer.iter().map(|v| self.p(*v)).collect();
        let n = f.normal;
        let k = pts.len();
        let left: Vec<DVec3> = (0..k).map(|i| n.cross((pts[(i + 1) % k] - pts[i]).normalize())).collect();
        (0..k)
            .map(|i| {
                let (m1, m2) = (left[(i + k - 1) % k], left[i]);
                let denom = 1.0 + m1.dot(m2);
                let off = if denom < 1e-6 { m2 } else { (m1 + m2) / denom };
                pts[i] + off * d
            })
            .collect()
    }

    pub fn reverse_faces(&mut self, faces: &HashSet<usize>) {
        for fi in faces {
            self.faces[*fi].reverse();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const EYE: DVec3 = DVec3::new(10.0, -10.0, 10.0);

    fn rect(m: &mut Geom, x0: f64, y0: f64, x1: f64, y1: f64) {
        m.add_polygon(
            &[DVec3::new(x0, y0, 0.0), DVec3::new(x1, y0, 0.0), DVec3::new(x1, y1, 0.0), DVec3::new(x0, y1, 0.0)],
            None,
            false,
            EYE,
        );
    }

    fn top_rect(m: &mut Geom, z: f64) {
        m.add_polygon(
            &[DVec3::new(1.0, 1.0, z), DVec3::new(2.0, 1.0, z), DVec3::new(2.0, 2.0, z), DVec3::new(1.0, 2.0, z)],
            None,
            false,
            EYE,
        );
    }

    #[test]
    fn rectangle_makes_face() {
        let mut m = Geom::default();
        rect(&mut m, 0.0, 0.0, 2.0, 1.0);
        assert_eq!(m.faces.len(), 1);
        assert_eq!(m.edges.len(), 4);
    }

    #[test]
    fn lines_close_face() {
        let mut m = Geom::default();
        let p = [DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0), DVec3::Y];
        assert!(!m.add_line(p[0], p[1], EYE));
        assert!(!m.add_line(p[1], p[2], EYE));
        assert!(!m.add_line(p[2], p[3], EYE));
        assert!(m.add_line(p[3], p[0], EYE));
        assert_eq!(m.faces.len(), 1);
    }

    #[test]
    fn chord_splits_face() {
        let mut m = Geom::default();
        rect(&mut m, 0.0, 0.0, 2.0, 1.0);
        m.add_line(DVec3::new(1.0, 0.0, 0.0), DVec3::new(1.0, 1.0, 0.0), EYE);
        assert_eq!(m.faces.len(), 2);
    }

    #[test]
    fn crossing_lines_split_each_other() {
        let mut m = Geom::default();
        rect(&mut m, 0.0, 0.0, 2.0, 2.0);
        // a line crossing the whole square, beyond its borders
        m.add_line(DVec3::new(-1.0, 1.0, 0.0), DVec3::new(3.0, 1.0, 0.0), EYE);
        assert_eq!(m.faces.len(), 2, "square split by a crossing line");
        m.add_line(DVec3::new(1.0, -1.0, 0.0), DVec3::new(1.0, 3.0, 0.0), EYE);
        assert_eq!(m.faces.len(), 4, "second crossing line splits again");
    }

    #[test]
    fn connected_crosses_face_holes() {
        let mut m = Geom::default();
        rect(&mut m, 0.0, 0.0, 4.0, 4.0);
        rect(&mut m, 1.0, 1.0, 2.0, 2.0);
        let seed: HashSet<VId> = [m.faces.iter().find(|f| !f.holes.is_empty()).unwrap().outer[0]].into();
        let c = m.connected(&seed);
        assert_eq!(c.faces.len(), 2);
        assert_eq!(c.edges.len(), 8);
    }

    #[test]
    fn inner_rect_makes_hole() {
        let mut m = Geom::default();
        rect(&mut m, 0.0, 0.0, 4.0, 4.0);
        rect(&mut m, 1.0, 1.0, 2.0, 2.0);
        assert_eq!(m.faces.len(), 2);
        assert_eq!(m.faces.iter().map(|f| f.holes.len()).sum::<usize>(), 1);
    }

    #[test]
    fn box_and_move_top() {
        let mut m = Geom::default();
        rect(&mut m, 0.0, 0.0, 2.0, 1.0);
        m.push_pull(0, 1.0, false, EYE);
        assert_eq!(m.faces.len(), 6);
        assert_eq!(m.edges.len(), 12);
        let top = m.faces.iter().position(|f| f.normal.z > 0.9).unwrap();
        m.push_pull(top, 0.5, false, EYE);
        assert_eq!(m.faces.len(), 6);
        let (_, hi) = m.own_bounds().unwrap();
        assert!((hi.z - 1.5).abs() < 1e-9);
    }

    #[test]
    fn erase_edge_merges_coplanar() {
        let mut m = Geom::default();
        rect(&mut m, 0.0, 0.0, 2.0, 1.0);
        m.add_line(DVec3::new(1.0, 0.0, 0.0), DVec3::new(1.0, 1.0, 0.0), EYE);
        let a = m.add_vertex(DVec3::new(1.0, 0.0, 0.0));
        let b = m.add_vertex(DVec3::new(1.0, 1.0, 0.0));
        let ei = m.find_edge(a, b).unwrap();
        m.erase_edge(ei, EYE);
        assert_eq!(m.faces.len(), 1);
    }

    #[test]
    fn erase_box_edge_removes_two_faces() {
        let mut m = Geom::default();
        rect(&mut m, 0.0, 0.0, 1.0, 1.0);
        m.push_pull(0, 1.0, false, EYE);
        let a = m.add_vertex(DVec3::new(0.0, 0.0, 1.0));
        let b = m.add_vertex(DVec3::new(1.0, 0.0, 1.0));
        let ei = m.find_edge(a, b).unwrap();
        m.erase_edge(ei, EYE);
        assert_eq!(m.faces.len(), 4);
    }

    #[test]
    fn push_holed_top_stretches_outer_walls() {
        let mut m = Geom::default();
        rect(&mut m, 0.0, 0.0, 4.0, 4.0);
        m.push_pull(0, 1.0, false, EYE);
        top_rect(&mut m, 1.0);
        let holed = m.faces.iter().position(|f| f.holes.len() == 1).unwrap();
        m.push_pull(holed, -0.5, false, EYE);
        assert_eq!(m.faces.len(), 11);
    }

    #[test]
    fn pocket_through_cut() {
        let mut m = Geom::default();
        rect(&mut m, 0.0, 0.0, 4.0, 4.0);
        m.push_pull(0, 1.0, false, EYE);
        top_rect(&mut m, 1.0);
        let inner = m
            .faces
            .iter()
            .position(|f| f.normal.z > 0.9 && f.holes.is_empty() && {
                let c = m.face_sample_point(f);
                c.x > 1.0 && c.x < 2.0
            })
            .unwrap();
        let r = m.push_pull(inner, -1.0, false, EYE);
        assert!(r.through_cut);
        assert_eq!(m.faces.len(), 10);
    }

    #[test]
    fn circle_extrusion_has_soft_sides() {
        let mut m = Geom::default();
        let c = m.new_curve();
        let pts: Vec<DVec3> = (0..24)
            .map(|k| {
                let a = k as f64 / 24.0 * std::f64::consts::TAU;
                DVec3::new(a.cos(), a.sin(), 0.0)
            })
            .collect();
        m.add_polygon(&pts, Some(c), true, EYE);
        assert_eq!(m.faces.len(), 1);
        m.push_pull(0, 1.0, false, EYE);
        assert_eq!(m.faces.len(), 26);
        assert_eq!(m.edges.iter().filter(|e| e.soft).count(), 24);
        assert_eq!(m.curve_edges(0).len(), 24);
    }

    #[test]
    fn move_and_copy_selection() {
        let mut m = Geom::default();
        rect(&mut m, 0.0, 0.0, 1.0, 1.0);
        let sel = Sel { faces: [0].into(), ..Default::default() };
        let copy = m.copy_sel(&sel, DMat4::from_translation(DVec3::new(1.0, 0.0, 0.0)), true);
        assert_eq!(m.faces.len(), 2);
        // copies share the edge x=1 after welding
        assert_eq!(m.edges.len(), 7);
        assert_eq!(copy.faces.len(), 1);
        let moved = m.transform_sel(&copy, DMat4::from_translation(DVec3::new(0.0, 0.0, 1.0)), true);
        assert_eq!(moved.faces.len(), 1);
        let (_, hi) = m.own_bounds().unwrap();
        assert!((hi.z - 1.0).abs() < 1e-9);
    }

    #[test]
    fn mirroring_keeps_faces_outward() {
        let mut m = Geom::default();
        rect(&mut m, 0.0, 0.0, 1.0, 2.0);
        m.push_pull(0, 1.0, false, EYE);
        let vol = |m: &Geom| -> f64 {
            m.faces.iter().flat_map(|f| m.triangulate_face(f)).map(|t| m.p(t[0]).dot(m.p(t[1]).cross(m.p(t[2]))) / 6.0).sum()
        };
        assert!((vol(&m) - 2.0).abs() < 1e-9);
        let all = Sel { faces: (0..m.faces.len()).collect(), edges: (0..m.edges.len()).collect(), insts: HashSet::new() };
        m.transform_sel(&all, DMat4::from_scale(DVec3::new(-1.0, 1.0, 1.0)), true);
        assert!((vol(&m) - 2.0).abs() < 1e-9, "mirrored volume {}", vol(&m));
    }

    #[test]
    fn offset_splits_face_into_ring() {
        let mut m = Geom::default();
        rect(&mut m, 0.0, 0.0, 4.0, 4.0);
        let pts = m.offset_loop(0, 0.5);
        assert!((pts[0] - DVec3::new(0.5, 0.5, 0.0)).length() < 1e-9);
        m.add_polygon(&pts, None, false, EYE);
        assert_eq!(m.faces.len(), 2);
    }

    #[test]
    fn extract_and_append_roundtrip() {
        let mut m = Geom::default();
        rect(&mut m, 0.0, 0.0, 1.0, 1.0);
        m.push_pull(0, 1.0, false, EYE);
        let all = Sel { faces: (0..6).collect(), edges: (0..12).collect(), insts: HashSet::new() };
        let part = m.subset(&all);
        m.remove_sel(&all);
        assert!(m.faces.is_empty() && m.edges.is_empty());
        m.append(&part, DMat4::IDENTITY, None);
        m.weld();
        assert_eq!(m.faces.len(), 6);
        assert_eq!(m.edges.len(), 12);
    }
}
