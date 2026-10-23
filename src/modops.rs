//! Modeling operations: Follow Me, Intersect Faces and solid booleans (CSG).

use crate::geom::{newell, Edge, Face, Geom, Plane, VId};
use glam::{DMat4, DVec2, DVec3};
use std::collections::{HashMap, HashSet};

// ====================================================================== Follow Me

/// Order the given edges into a chain of points (open or closed).
pub fn chain_edges(g: &Geom, edges: &[usize], near: DVec3) -> Option<(Vec<DVec3>, bool)> {
    let mut adj: HashMap<VId, Vec<VId>> = HashMap::new();
    for e in edges {
        let [a, b] = g.edges[*e].v;
        adj.entry(a).or_default().push(b);
        adj.entry(b).or_default().push(a);
    }
    if adj.is_empty() || adj.values().any(|n| n.len() > 2) {
        return None;
    }
    let ends: Vec<VId> = adj.iter().filter(|(_, n)| n.len() == 1).map(|(v, _)| *v).collect();
    let closed = ends.is_empty();
    let start = if closed {
        *adj.keys().min_by(|a, b| g.p(**a).distance(near).partial_cmp(&g.p(**b).distance(near)).unwrap()).unwrap()
    } else {
        *ends.iter().min_by(|a, b| g.p(**a).distance(near).partial_cmp(&g.p(**b).distance(near)).unwrap()).unwrap()
    };
    let mut pts = vec![start];
    let mut prev = None;
    let mut cur = start;
    loop {
        let next = adj[&cur].iter().copied().find(|n| Some(*n) != prev && (pts.len() < 2 || *n != pts[pts.len() - 2]));
        let Some(n) = next else { break };
        if n == start {
            break;
        }
        if pts.contains(&n) {
            return None;
        }
        pts.push(n);
        prev = Some(cur);
        cur = n;
    }
    if pts.len() != adj.len() {
        return None; // not a single connected chain
    }
    Some((pts.into_iter().map(|v| g.p(v)).collect(), closed))
}

/// Sweep the outer loop of `face` along `path` (world-independent, local coords).
pub fn follow_me(g: &mut Geom, face: usize, path: &[DVec3], closed: bool) -> Result<(), String> {
    if path.len() < 2 {
        return Err("O caminho precisa de pelo menos uma aresta.".into());
    }
    let mut path = path.to_vec();
    if closed {
        path.push(path[0]);
    }
    let prof = g.faces[face].clone();
    let ring0: Vec<DVec3> = prof.outer.iter().map(|v| g.p(*v)).collect();
    // profile vertices between two edges of the same smooth curve give soft sweep edges
    let pk = prof.outer.len();
    let pedge = |a: VId, b: VId| g.find_edge(a, b).map(|e| g.edges[e].clone());
    let soft_at: Vec<bool> = (0..pk)
        .map(|j| {
            let (p, c, nx) = (prof.outer[(j + pk - 1) % pk], prof.outer[j], prof.outer[(j + 1) % pk]);
            match (pedge(p, c), pedge(c, nx)) {
                (Some(x), Some(y)) => x.arc && y.arc && x.curve.is_some() && x.curve == y.curve,
                _ => false,
            }
        })
        .collect();
    let n = path.len();
    let dirs: Vec<DVec3> = (0..n - 1).map(|i| (path[i + 1] - path[i]).normalize()).collect();
    if dirs.iter().any(|d| !d.is_finite()) {
        return Err("O caminho tem segmentos degenerados.".into());
    }
    // miter plane normals at each path vertex
    let miter = |i: usize| -> DVec3 {
        if i == 0 {
            if closed { (dirs[n - 2] + dirs[0]).normalize_or(dirs[0]) } else { dirs[0] }
        } else if i == n - 1 {
            if closed { (dirs[n - 2] + dirs[0]).normalize_or(dirs[0]) } else { dirs[n - 2] }
        } else {
            (dirs[i - 1] + dirs[i]).normalize_or(dirs[i])
        }
    };
    // project the profile onto the start miter plane (along the first direction)
    let project = |ring: &[DVec3], d: DVec3, p: DVec3, m: DVec3| -> Option<Vec<DVec3>> {
        let dm = d.dot(m);
        if dm.abs() < 1e-9 {
            return None;
        }
        Some(ring.iter().map(|q| *q + d * ((p - *q).dot(m) / dm)).collect())
    };
    let mut rings: Vec<Vec<DVec3>> = vec![project(&ring0, dirs[0], path[0], miter(0)).ok_or("Perfil paralelo ao caminho.")?];
    for i in 1..n {
        let r = project(rings.last().unwrap(), dirs[i - 1], path[i], miter(i)).ok_or("Ângulo do caminho muito fechado.")?;
        rings.push(r);
    }
    let k = ring0.len();
    let mut ids: Vec<Vec<VId>> = Vec::new();
    for r in &rings {
        ids.push(
            r.iter()
                .map(|p| {
                    g.verts.push(*p);
                    (g.verts.len() - 1) as VId
                })
                .collect(),
        );
    }
    if closed {
        let first = ids[0].clone();
        *ids.last_mut().unwrap() = first;
    }
    let centroid = |r: &[DVec3]| r.iter().copied().sum::<DVec3>() / r.len() as f64;
    let mut new_edges: Vec<Edge> = Vec::new();
    for s in 0..n - 1 {
        let c = (centroid(&rings[s]) + centroid(&rings[s + 1])) * 0.5;
        for j in 0..k {
            let (a, b) = (ids[s][j], ids[s][(j + 1) % k]);
            let (a2, b2) = (ids[s + 1][j], ids[s + 1][(j + 1) % k]);
            let mut quad = vec![a, b, b2, a2];
            let pts: Vec<DVec3> = quad.iter().map(|v| g.p(*v)).collect();
            let mut nrm = newell(&pts);
            if nrm.length() < 1e-14 {
                continue;
            }
            let mid = pts.iter().copied().sum::<DVec3>() / 4.0;
            if nrm.dot(mid - c) < 0.0 {
                quad.reverse();
                nrm = -nrm;
            }
            g.faces.push(Face { outer: quad, holes: vec![], normal: nrm.normalize(), material: prof.material, ..Default::default() });
            new_edges.push(Edge::new(a, b));
            let mut long = Edge::new(a, a2);
            long.soft = soft_at[j];
            new_edges.push(long);
        }
    }
    // ring edges at interior path vertices of a smooth curve are soft
    for j in 0..k {
        let (a, b) = (ids[n - 1][j], ids[n - 1][(j + 1) % k]);
        new_edges.push(Edge::new(a, b));
    }
    for e in new_edges {
        if g.find_edge(e.v[0], e.v[1]).is_none() && e.v[0] != e.v[1] {
            g.edges.push(e);
        }
    }
    // caps
    g.faces.remove(face);
    if !closed {
        let start: Vec<VId> = ids[0].clone();
        let end: Vec<VId> = ids[n - 1].clone();
        for (ring, outward) in [(start, -dirs[0]), (end, dirs[n - 2])] {
            let mut l = ring;
            let pts: Vec<DVec3> = l.iter().map(|v| g.p(*v)).collect();
            let mut nrm = newell(&pts);
            if nrm.dot(outward) < 0.0 {
                l.reverse();
                nrm = -nrm;
            }
            g.faces.push(Face { outer: l, holes: vec![], normal: nrm.normalize(), material: prof.material, ..Default::default() });
        }
    }
    g.weld();
    Ok(())
}

// ====================================================================== Intersect faces

/// Parameter intervals where a 2D line (p + t d) is inside a polygon with holes.
fn line_intervals(p: DVec2, d: DVec2, loops: &[Vec<DVec2>]) -> Vec<(f64, f64)> {
    let nrm = DVec2::new(-d.y, d.x);
    let mut ts: Vec<f64> = Vec::new();
    for l in loops {
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            let (sa, sb) = ((a - p).dot(nrm), (b - p).dot(nrm));
            if (sa > 0.0) != (sb > 0.0) {
                let x = a + (b - a) * (sa / (sa - sb));
                ts.push((x - p).dot(d) / d.length_squared());
            }
        }
    }
    ts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ts.chunks_exact(2).map(|c| (c[0], c[1])).filter(|(a, b)| b - a > 1e-9).collect()
}

fn face_loops_2d(g: &Geom, f: &Face, u: DVec3, v: DVec3) -> Vec<Vec<DVec2>> {
    f.loops().map(|l| l.iter().map(|id| DVec2::new(g.p(*id).dot(u), g.p(*id).dot(v))).collect()).collect()
}

/// Segments where face `a` crosses face `b`.
pub fn face_face_segments(g: &Geom, a: &Face, b: &Face) -> Vec<(DVec3, DVec3)> {
    let (pa, pb) = (g.face_plane(a), g.face_plane(b));
    let dir = pa.n.cross(pb.n);
    let l2 = dir.length_squared();
    if l2 < 1e-12 {
        return vec![];
    }
    let n1n2 = pa.n.dot(pb.n);
    let p0 = (pa.n * (pa.d - pb.d * n1n2) + pb.n * (pb.d - pa.d * n1n2)) / l2;
    let dir = dir / l2.sqrt();
    let ivs = |f: &Face, pl: &Plane| {
        let (u, v) = pl.basis();
        let loops = face_loops_2d(g, f, u, v);
        line_intervals(DVec2::new(p0.dot(u), p0.dot(v)), DVec2::new(dir.dot(u), dir.dot(v)), &loops)
    };
    let (ia, ib) = (ivs(a, &pa), ivs(b, &pb));
    let mut out = Vec::new();
    for (a0, a1) in &ia {
        for (b0, b1) in &ib {
            let (s, e) = (a0.max(*b0), a1.min(*b1));
            if e - s > 1e-7 {
                out.push((p0 + dir * s, p0 + dir * e));
            }
        }
    }
    out
}

/// Add the intersection edges between `set_a` faces and `set_b` faces.
pub fn intersect_faces(g: &mut Geom, set_a: &[usize], set_b: &[usize], eye: DVec3) -> usize {
    let mut segs = Vec::new();
    let mut done: HashSet<(usize, usize)> = HashSet::new();
    for &i in set_a {
        for &j in set_b {
            if i == j || !done.insert((i.min(j), i.max(j))) {
                continue;
            }
            segs.extend(face_face_segments(g, &g.faces[i], &g.faces[j]));
        }
    }
    let n = segs.len();
    if n > 0 {
        g.add_segments(&segs, None, false, eye);
    }
    n
}

// ====================================================================== CSG (BSP)

const CSG_EPS: f64 = 1e-6;

#[derive(Clone, Debug)]
pub struct CPoly {
    pub v: Vec<DVec3>,
    pub n: DVec3,
    pub w: f64,
    pub mat: Option<usize>,
}

impl CPoly {
    fn new(v: Vec<DVec3>, mat: Option<usize>) -> Option<CPoly> {
        let n = newell(&v);
        if n.length() < 1e-14 {
            return None;
        }
        let n = n.normalize();
        Some(CPoly { w: n.dot(v[0]), n, v, mat })
    }
    fn flip(&mut self) {
        self.v.reverse();
        self.n = -self.n;
        self.w = -self.w;
    }
}

#[derive(Default)]
struct Node {
    plane: Option<(DVec3, f64)>,
    front: Option<Box<Node>>,
    back: Option<Box<Node>>,
    polys: Vec<CPoly>,
}

fn split(plane: (DVec3, f64), p: &CPoly, cf: &mut Vec<CPoly>, cb: &mut Vec<CPoly>, f: &mut Vec<CPoly>, b: &mut Vec<CPoly>) {
    const COP: u8 = 0;
    const FRONT: u8 = 1;
    const BACK: u8 = 2;
    let (n, w) = plane;
    let mut ptype = 0u8;
    let types: Vec<u8> = p
        .v
        .iter()
        .map(|v| {
            let t = n.dot(*v) - w;
            let ty = if t < -CSG_EPS { BACK } else if t > CSG_EPS { FRONT } else { COP };
            ptype |= ty;
            ty
        })
        .collect();
    match ptype {
        COP => {
            if n.dot(p.n) > 0.0 { cf.push(p.clone()) } else { cb.push(p.clone()) }
        }
        FRONT => f.push(p.clone()),
        BACK => b.push(p.clone()),
        _ => {
            let (mut fv, mut bv) = (Vec::new(), Vec::new());
            let k = p.v.len();
            for i in 0..k {
                let j = (i + 1) % k;
                let (ti, tj) = (types[i], types[j]);
                let (vi, vj) = (p.v[i], p.v[j]);
                if ti != BACK {
                    fv.push(vi);
                }
                if ti != FRONT {
                    bv.push(vi);
                }
                if (ti | tj) == (FRONT | BACK) {
                    let t = (w - n.dot(vi)) / n.dot(vj - vi);
                    let x = vi.lerp(vj, t);
                    fv.push(x);
                    bv.push(x);
                }
            }
            if fv.len() >= 3 {
                f.push(CPoly { v: fv, ..p.clone() });
            }
            if bv.len() >= 3 {
                b.push(CPoly { v: bv, ..p.clone() });
            }
        }
    }
}

impl Node {
    fn new(polys: Vec<CPoly>) -> Node {
        let mut n = Node::default();
        n.build(polys);
        n
    }
    fn invert(&mut self) {
        for p in &mut self.polys {
            p.flip();
        }
        if let Some((n, w)) = self.plane {
            self.plane = Some((-n, -w));
        }
        if let Some(f) = &mut self.front {
            f.invert();
        }
        if let Some(b) = &mut self.back {
            b.invert();
        }
        std::mem::swap(&mut self.front, &mut self.back);
    }
    fn clip_polygons(&self, polys: Vec<CPoly>) -> Vec<CPoly> {
        let Some(plane) = self.plane else { return polys };
        let (mut f, mut b) = (Vec::new(), Vec::new());
        for p in &polys {
            let (mut cf, mut cb) = (Vec::new(), Vec::new());
            split(plane, p, &mut cf, &mut cb, &mut f, &mut b);
            f.extend(cf);
            b.extend(cb);
        }
        let mut f = match &self.front {
            Some(n) => n.clip_polygons(f),
            None => f,
        };
        let b = match &self.back {
            Some(n) => n.clip_polygons(b),
            None => Vec::new(),
        };
        f.extend(b);
        f
    }
    fn clip_to(&mut self, bsp: &Node) {
        self.polys = bsp.clip_polygons(std::mem::take(&mut self.polys));
        if let Some(f) = &mut self.front {
            f.clip_to(bsp);
        }
        if let Some(b) = &mut self.back {
            b.clip_to(bsp);
        }
    }
    fn all(&self) -> Vec<CPoly> {
        let mut v = self.polys.clone();
        if let Some(f) = &self.front {
            v.extend(f.all());
        }
        if let Some(b) = &self.back {
            v.extend(b.all());
        }
        v
    }
    fn build(&mut self, polys: Vec<CPoly>) {
        if polys.is_empty() {
            return;
        }
        let plane = *self.plane.get_or_insert((polys[0].n, polys[0].w));
        let (mut f, mut b) = (Vec::new(), Vec::new());
        for p in &polys {
            let (mut cf, mut cb) = (Vec::new(), Vec::new());
            split(plane, p, &mut cf, &mut cb, &mut f, &mut b);
            self.polys.extend(cf);
            self.polys.extend(cb);
        }
        if !f.is_empty() {
            self.front.get_or_insert_with(Default::default).build(f);
        }
        if !b.is_empty() {
            self.back.get_or_insert_with(Default::default).build(b);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BoolOp {
    Union,
    Subtract,
    Intersect,
}

/// Boolean between two closed polygon sets (runs on a big-stack thread).
pub fn boolean(a: Vec<CPoly>, b: Vec<CPoly>, op: BoolOp) -> Vec<CPoly> {
    let h = std::thread::Builder::new().stack_size(256 << 20).spawn(move || {
        let mut a = Node::new(a);
        let mut b = Node::new(b);
        match op {
            BoolOp::Union => {
                a.clip_to(&b);
                b.clip_to(&a);
                b.invert();
                b.clip_to(&a);
                b.invert();
                a.build(b.all());
            }
            BoolOp::Subtract => {
                a.invert();
                a.clip_to(&b);
                b.clip_to(&a);
                b.invert();
                b.clip_to(&a);
                b.invert();
                a.build(b.all());
                a.invert();
            }
            BoolOp::Intersect => {
                a.invert();
                b.clip_to(&a);
                b.invert();
                a.clip_to(&b);
                b.clip_to(&a);
                a.build(b.all());
                a.invert();
            }
        }
        a.all()
    });
    h.map(|h| h.join().unwrap_or_default()).unwrap_or_default()
}

/// Polygons (convex triangles) of a geometry under `xf`.
pub fn polys_of(g: &Geom, xf: DMat4, inherited: Option<usize>) -> Vec<CPoly> {
    let flip = xf.determinant() < 0.0;
    let mut out = Vec::new();
    for f in &g.faces {
        for t in g.triangulate_face(f) {
            let mut v: Vec<DVec3> = t.iter().map(|i| xf.transform_point3(g.p(*i))).collect();
            if flip {
                v.reverse();
            }
            if let Some(p) = CPoly::new(v, f.material.or(inherited)) {
                out.push(p);
            }
        }
    }
    out
}

/// Turn CSG polygons into clean geometry: welded, T-junctions split, coplanar
/// fragments merged into single faces.
pub fn polys_to_geom(polys: &[CPoly]) -> Geom {
    let mut g = Geom::default();
    let eps = 1e-6;
    let mut grid: HashMap<(i64, i64, i64), VId> = HashMap::new();
    for p in polys {
        let mut ids: Vec<VId> = p
            .v
            .iter()
            .map(|q| {
                let k = ((q.x / eps).round() as i64, (q.y / eps).round() as i64, (q.z / eps).round() as i64);
                *grid.entry(k).or_insert_with(|| {
                    g.verts.push(*q);
                    (g.verts.len() - 1) as VId
                })
            })
            .collect();
        ids.dedup();
        while ids.len() > 1 && ids.first() == ids.last() {
            ids.pop();
        }
        if ids.len() < 3 {
            continue;
        }
        for i in 0..ids.len() {
            let (a, b) = (ids[i], ids[(i + 1) % ids.len()]);
            if g.find_edge(a, b).is_none() {
                g.edges.push(Edge::new(a, b));
            }
        }
        g.faces.push(Face { outer: ids, holes: vec![], normal: p.n, material: p.mat, ..Default::default() });
    }
    // split edges at T-junctions
    loop {
        let mut split = None;
        'outer: for (ei, e) in g.edges.iter().enumerate() {
            let (a, b) = (g.p(e.v[0]), g.p(e.v[1]));
            let ab = b - a;
            let l2 = ab.length_squared();
            for (vi, v) in g.verts.iter().enumerate() {
                let vi = vi as VId;
                if vi == e.v[0] || vi == e.v[1] {
                    continue;
                }
                let t = (*v - a).dot(ab) / l2;
                if t > 1e-7 && t < 1.0 - 1e-7 && (a + ab * t).distance(*v) < 1e-6 {
                    split = Some((ei, vi));
                    break 'outer;
                }
            }
        }
        match split {
            Some((ei, v)) => g.split_edge_pub(ei, v),
            None => break,
        }
    }
    // remove interior edges between coplanar faces with the same material, then rebuild those planes
    let mut by_edge: HashMap<(VId, VId), Vec<usize>> = HashMap::new();
    for (fi, f) in g.faces.iter().enumerate() {
        for (a, b) in f.boundary_edges() {
            by_edge.entry((a.min(b), a.max(b))).or_default().push(fi);
        }
    }
    let mut planes: Vec<Plane> = Vec::new();
    let mut remove: HashSet<(VId, VId)> = HashSet::new();
    for (k, fs) in &by_edge {
        if fs.len() == 2 {
            let (f1, f2) = (&g.faces[fs[0]], &g.faces[fs[1]]);
            if f1.normal.dot(f2.normal) > 1.0 - 1e-9 && f1.material == f2.material {
                remove.insert(*k);
                let pl = g.face_plane(f1);
                if !planes.iter().any(|q| q.same(&pl)) {
                    planes.push(pl);
                }
            }
        }
    }
    g.edges.retain(|e| !remove.contains(&(e.v[0].min(e.v[1]), e.v[0].max(e.v[1]))));
    for pl in planes {
        g.recompute_plane(&pl, &HashSet::new(), &[], DVec3::new(1e3, -1e3, 1e3));
    }
    g.weld();
    g
}

#[cfg(test)]
mod tests {
    use super::*;
    const EYE: DVec3 = DVec3::new(10.0, -10.0, 10.0);

    fn cube(g: &mut Geom, o: DVec3, s: f64) {
        let pts = [o, o + DVec3::X * s, o + DVec3::new(s, s, 0.0), o + DVec3::Y * s];
        g.add_polygon(&pts, None, false, EYE);
        let fi = g.faces.len() - 1;
        g.push_pull(fi, s, false, EYE);
    }

    fn volume(polys: &[CPoly]) -> f64 {
        polys.iter().map(|p| (1..p.v.len() - 1).map(|i| p.v[0].dot(p.v[i].cross(p.v[i + 1])) / 6.0).sum::<f64>()).sum()
    }

    #[test]
    fn csg_volumes() {
        let mut a = Geom::default();
        cube(&mut a, DVec3::ZERO, 2.0);
        let mut b = Geom::default();
        cube(&mut b, DVec3::splat(1.0), 2.0);
        let pa = polys_of(&a, DMat4::IDENTITY, None);
        let pb = polys_of(&b, DMat4::IDENTITY, None);
        assert!((volume(&pa) - 8.0).abs() < 1e-9);
        let u = boolean(pa.clone(), pb.clone(), BoolOp::Union);
        assert!((volume(&u) - 15.0).abs() < 1e-6, "union {}", volume(&u));
        let s = boolean(pa.clone(), pb.clone(), BoolOp::Subtract);
        assert!((volume(&s) - 7.0).abs() < 1e-6, "subtract {}", volume(&s));
        let i = boolean(pa, pb, BoolOp::Intersect);
        assert!((volume(&i) - 1.0).abs() < 1e-6, "intersect {}", volume(&i));
        // the intersection is a unit cube: 6 clean faces after merging
        let g = polys_to_geom(&i);
        assert_eq!(g.faces.len(), 6);
        assert_eq!(g.edges.len(), 12);
    }

    fn area_polys(p: &[CPoly]) -> f64 {
        p.iter().map(|q| newell(&q.v).length() * 0.5).sum()
    }

    #[test]
    fn subtract_keeps_all_surface() {
        let mut a = Geom::default();
        cube(&mut a, DVec3::ZERO, 2.0);
        let mut b = Geom::default();
        cube(&mut b, DVec3::splat(1.0), 2.0);
        let s = boolean(polys_of(&a, DMat4::IDENTITY, None), polys_of(&b, DMat4::IDENTITY, None), BoolOp::Subtract);
        let g = polys_to_geom(&s);
        let area: f64 = g.faces.iter().map(|f| g.face_area(f)).sum();
        // 24 outer minus 3 notch openings plus 3 notch walls = 24
        assert!((area_polys(&s) - 24.0).abs() < 1e-6, "csg area {}", area_polys(&s));
        assert!((area - 24.0).abs() < 1e-6, "geom area {area}, {} faces", g.faces.len());
        assert_eq!(g.faces.len(), 9);
    }

    #[test]
    fn follow_me_square_along_l_path() {
        let mut g = Geom::default();
        // profile: 1x1 square in the YZ plane at the origin
        g.add_polygon(
            &[DVec3::ZERO, DVec3::new(0.0, 1.0, 0.0), DVec3::new(0.0, 1.0, 1.0), DVec3::new(0.0, 0.0, 1.0)],
            None,
            false,
            DVec3::new(-10.0, 0.5, 0.5),
        );
        let path = [DVec3::new(0.0, 0.5, 0.5), DVec3::new(4.0, 0.5, 0.5), DVec3::new(4.0, 4.5, 0.5)];
        follow_me(&mut g, 0, &path, false).unwrap();
        // 4 sides x 2 segments + 2 caps
        assert_eq!(g.faces.len(), 10);
        let hi = g.verts.iter().fold(DVec3::splat(-1e9), |a, b| a.max(*b));
        assert!(hi.y > 4.0);
    }

    #[test]
    fn intersecting_two_faces_adds_an_edge() {
        let mut g = Geom::default();
        g.add_polygon(&[DVec3::new(-1.0, -1.0, 0.0), DVec3::new(1.0, -1.0, 0.0), DVec3::new(1.0, 1.0, 0.0), DVec3::new(-1.0, 1.0, 0.0)], None, false, EYE);
        g.add_polygon(&[DVec3::new(0.0, -2.0, -1.0), DVec3::new(0.0, 2.0, -1.0), DVec3::new(0.0, 2.0, 1.0), DVec3::new(0.0, -2.0, 1.0)], None, false, EYE);
        assert_eq!(g.faces.len(), 2);
        let n = intersect_faces(&mut g, &[0], &[1], EYE);
        assert_eq!(n, 1);
        // the horizontal face is split edge to edge; the vertical one only gains an inner edge
        assert_eq!(g.faces.len(), 3);
        assert!(g.find_edge(g.verts.iter().position(|p| p.distance(DVec3::new(0.0, -1.0, 0.0)) < 1e-9).unwrap() as VId, g.verts.iter().position(|p| p.distance(DVec3::new(0.0, 1.0, 0.0)) < 1e-9).unwrap() as VId).is_some());
    }
}
