//! Floor plans from CAD and PDF: DXF (ASCII, R12–2018: lines, polylines with
//! bulges, circles, arcs, ellipses, splines, blocks/inserts, dimensions' graphics,
//! layers) and vector PDF pages (paths, form XObjects, object streams).
//! The drawing becomes a component (blocks become nested components, layers
//! become tags); closed regions can become faces, ready for Push/Pull.

use crate::app::{HomeUpApp, Tool};
use crate::doc::Definition;
use crate::geom::{Edge, Geom, Instance, Plane, VId};
use crate::settings::Tag;
use eframe::egui::{self, RichText};
use glam::{DMat4, DVec2, DVec3};
use std::collections::{HashMap, HashSet};

/// A polyline in drawing units.
#[derive(Clone, Debug)]
pub struct Poly {
    pub pts: Vec<DVec3>,
    pub closed: bool,
    pub layer: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Block {
    pub name: String,
    pub base: DVec3,
    pub polys: Vec<Poly>,
    /// (block name, transform)
    pub inserts: Vec<(String, DMat4, usize)>,
}

#[derive(Clone, Debug, Default)]
pub struct Drawing {
    pub layers: Vec<(String, [u8; 3])>,
    pub model: Block,
    pub blocks: Vec<Block>,
    /// Meters per drawing unit, when the file says so.
    pub unit_m: Option<f64>,
    pub source: String,
}

impl Drawing {
    fn layer(&mut self, name: &str) -> usize {
        if let Some(k) = self.layers.iter().position(|l| l.0 == name) {
            return k;
        }
        self.layers.push((name.to_string(), [160, 160, 160]));
        self.layers.len() - 1
    }
    pub fn segment_count(&self) -> usize {
        let c = |b: &Block| b.polys.iter().map(|p| p.pts.len().saturating_sub(1) + p.closed as usize).sum::<usize>();
        c(&self.model) + self.blocks.iter().map(c).sum::<usize>()
    }
}

fn arc_pts(c: DVec3, r: f64, a0: f64, a1: f64) -> Vec<DVec3> {
    let mut sweep = a1 - a0;
    while sweep <= 0.0 {
        sweep += std::f64::consts::TAU;
    }
    let n = ((sweep / std::f64::consts::TAU * 48.0).ceil() as usize).clamp(4, 96);
    (0..=n).map(|k| {
        let a = a0 + sweep * k as f64 / n as f64;
        c + DVec3::new(a.cos() * r, a.sin() * r, 0.0)
    }).collect()
}

/// Points of a polyline segment with a DXF bulge (tan of a quarter of the arc angle).
fn bulge_pts(p: DVec3, q: DVec3, b: f64) -> Vec<DVec3> {
    if b.abs() < 1e-9 {
        return vec![q];
    }
    let theta = 4.0 * b.atan();
    let chord = q - p;
    let l = chord.truncate().length();
    if l < 1e-12 {
        return vec![q];
    }
    let r = l / (2.0 * (theta / 2.0).sin());
    let mid = (p + q) * 0.5;
    let nrm = DVec3::new(-chord.y, chord.x, 0.0) / l;
    let h = r * (theta / 2.0).cos();
    let c = mid + nrm * h;
    let a0 = (p.y - c.y).atan2(p.x - c.x);
    let n = ((theta.abs() / std::f64::consts::TAU * 48.0).ceil() as usize).clamp(2, 64);
    (1..=n).map(|k| {
        let a = a0 + theta * k as f64 / n as f64;
        DVec3::new(c.x + r.abs() * a.cos(), c.y + r.abs() * a.sin(), p.z + (q.z - p.z) * k as f64 / n as f64)
    }).collect()
}

fn aci(c: i32) -> [u8; 3] {
    match c.abs() {
        1 => [220, 40, 40],
        2 => [220, 200, 40],
        3 => [40, 180, 60],
        4 => [40, 190, 200],
        5 => [40, 70, 220],
        6 => [200, 50, 200],
        7 => [40, 40, 40],
        8 => [128, 128, 128],
        9 => [190, 190, 190],
        _ => [140, 140, 140],
    }
}

// ------------------------------------------------------------------ DXF

pub fn parse_dxf(text: &str) -> Result<Drawing, String> {
    let lines: Vec<&str> = text.lines().map(|l| l.trim()).collect();
    if lines.len() < 4 {
        return Err("arquivo DXF vazio".into());
    }
    let mut pairs: Vec<(i32, &str)> = Vec::with_capacity(lines.len() / 2);
    let mut k = 0;
    while k + 1 < lines.len() {
        let code = lines[k].parse::<i32>().map_err(|_| format!("DXF inválido perto da linha {} (é um DXF binário?)", k + 1))?;
        pairs.push((code, lines[k + 1]));
        k += 2;
    }
    let mut d = Drawing { source: "DXF".into(), ..Default::default() };
    // drawing units (header variable $INSUNITS, followed by a 70 group)
    if let Some(k) = pairs.iter().position(|p| p.0 == 9 && p.1 == "$INSUNITS") {
        if let Some((70, u)) = pairs.get(k + 1) {
            d.unit_m = match u.parse::<i32>().unwrap_or(0) {
                1 => Some(0.0254),
                2 => Some(0.3048),
                4 => Some(0.001),
                5 => Some(0.01),
                6 => Some(1.0),
                14 => Some(0.1),
                _ => None,
            };
        }
    }
    let mut i = 0;
    let mut section = "";
    let mut cur_block: Option<Block> = None;
    let f = |v: &str| v.parse::<f64>().unwrap_or(0.0);
    while i < pairs.len() {
        let (c, v) = pairs[i];
        if c != 0 {
            i += 1;
            continue;
        }
        // an entity / record starts: gather its pairs
        let start = i + 1;
        let mut end = start;
        while end < pairs.len() && pairs[end].0 != 0 {
            end += 1;
        }
        let rec = &pairs[start..end];
        let get = |code: i32| rec.iter().find(|p| p.0 == code).map(|p| p.1);
        let num = |code: i32| get(code).map(f).unwrap_or(0.0);
        match v {
            "SECTION" => section = get(2).unwrap_or(""),
            "ENDSEC" => section = "",
            "LAYER" if section == "TABLES" => {
                if let Some(n) = get(2) {
                    let k = d.layer(n);
                    d.layers[k].1 = aci(get(62).and_then(|c| c.parse().ok()).unwrap_or(7));
                }
            }
            "BLOCK" => cur_block = Some(Block { name: get(2).unwrap_or("").to_string(), base: DVec3::new(num(10), num(20), num(30)), ..Default::default() }),
            "ENDBLK" => {
                if let Some(b) = cur_block.take() {
                    d.blocks.push(b);
                }
            }
            "EOF" => break,
            _ if section == "ENTITIES" || section == "BLOCKS" => {
                let layer_name = get(8).unwrap_or("0").to_string();
                let layer = d.layer(&layer_name);
                let target = match cur_block.as_mut() {
                    Some(b) => b,
                    None => &mut d.model,
                };
                let p3 = |x: i32| DVec3::new(num(x), num(x + 10), num(x + 20));
                match v {
                    "LINE" => target.polys.push(Poly { pts: vec![p3(10), p3(11)], closed: false, layer }),
                    "LWPOLYLINE" => {
                        let closed = get(70).and_then(|x| x.parse::<i32>().ok()).unwrap_or(0) & 1 != 0;
                        let elev = num(38);
                        let mut verts: Vec<(DVec3, f64)> = Vec::new();
                        for (c2, v2) in rec {
                            match c2 {
                                10 => verts.push((DVec3::new(f(v2), 0.0, elev), 0.0)),
                                20 => {
                                    if let Some(l) = verts.last_mut() {
                                        l.0.y = f(v2);
                                    }
                                }
                                42 => {
                                    if let Some(l) = verts.last_mut() {
                                        l.1 = f(v2);
                                    }
                                }
                                _ => {}
                            }
                        }
                        target.polys.push(poly_from(&verts, closed, layer));
                    }
                    "POLYLINE" => {
                        let closed = get(70).and_then(|x| x.parse::<i32>().ok()).unwrap_or(0) & 1 != 0;
                        let mut verts: Vec<(DVec3, f64)> = Vec::new();
                        let mut j = end;
                        while j < pairs.len() && pairs[j].0 == 0 && pairs[j].1 == "VERTEX" {
                            let s2 = j + 1;
                            let mut e2 = s2;
                            while e2 < pairs.len() && pairs[e2].0 != 0 {
                                e2 += 1;
                            }
                            let r2 = &pairs[s2..e2];
                            let g2 = |code: i32| r2.iter().find(|p| p.0 == code).map(|p| f(p.1)).unwrap_or(0.0);
                            verts.push((DVec3::new(g2(10), g2(20), g2(30)), g2(42)));
                            j = e2;
                        }
                        target.polys.push(poly_from(&verts, closed, layer));
                        i = j;
                        continue;
                    }
                    "CIRCLE" => target.polys.push(Poly { pts: arc_pts(p3(10), num(40), 0.0, std::f64::consts::TAU), closed: true, layer }),
                    "ARC" => target.polys.push(Poly { pts: arc_pts(p3(10), num(40), num(50).to_radians(), num(51).to_radians()), closed: false, layer }),
                    "ELLIPSE" => {
                        let (c, major, ratio) = (p3(10), p3(11), num(40).max(1e-9));
                        let (t0, mut t1) = (num(41), num(42));
                        if t1 <= t0 {
                            t1 += std::f64::consts::TAU;
                        }
                        let minor = DVec3::new(-major.y, major.x, 0.0) * ratio;
                        let n = 48;
                        let pts = (0..=n).map(|k| {
                            let t = t0 + (t1 - t0) * k as f64 / n as f64;
                            c + major * t.cos() + minor * t.sin()
                        }).collect();
                        target.polys.push(Poly { pts, closed: (t1 - t0 - std::f64::consts::TAU).abs() < 1e-6, layer });
                    }
                    "SPLINE" => {
                        // fit points when present, otherwise the control polygon
                        let mut fit = Vec::new();
                        let mut ctrl = Vec::new();
                        let mut cur: Option<(i32, DVec3)> = None;
                        for (c2, v2) in rec {
                            match c2 {
                                10 | 11 => cur = Some((*c2, DVec3::new(f(v2), 0.0, 0.0))),
                                20 | 21 => {
                                    if let Some(c) = cur.as_mut() {
                                        c.1.y = f(v2);
                                    }
                                }
                                30 | 31 => {
                                    if let Some((code, mut p)) = cur.take() {
                                        p.z = f(v2);
                                        if code == 10 { ctrl.push(p) } else { fit.push(p) }
                                    }
                                }
                                _ => {}
                            }
                        }
                        let pts = if fit.len() >= 2 { fit } else { ctrl };
                        if pts.len() >= 2 {
                            target.polys.push(Poly { pts, closed: false, layer });
                        }
                    }
                    "INSERT" | "DIMENSION" => {
                        if let Some(name) = get(2) {
                            let (sx, sy, sz) = if v == "INSERT" { (get(41).map(f).unwrap_or(1.0), get(42).map(f).unwrap_or(1.0), get(43).map(f).unwrap_or(1.0)) } else { (1.0, 1.0, 1.0) };
                            let xf = if v == "INSERT" {
                                DMat4::from_translation(p3(10)) * DMat4::from_rotation_z(num(50).to_radians()) * DMat4::from_scale(DVec3::new(sx, sy, sz))
                            } else {
                                DMat4::IDENTITY
                            };
                            target.inserts.push((name.to_string(), xf, layer));
                        }
                    }
                    "SOLID" | "3DFACE" => {
                        let pts: Vec<DVec3> = [10, 11, 12, 13].iter().map(|c| p3(*c)).collect();
                        target.polys.push(Poly { pts, closed: true, layer });
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        i = end;
    }
    if d.model.polys.is_empty() && d.model.inserts.is_empty() {
        return Err("o DXF não tem linhas no espaço do modelo".into());
    }
    Ok(d)
}

fn poly_from(verts: &[(DVec3, f64)], closed: bool, layer: usize) -> Poly {
    let mut pts = Vec::new();
    if let Some(first) = verts.first() {
        pts.push(first.0);
    }
    let n = verts.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    for k in 0..segs {
        let (p, b) = verts[k];
        let q = verts[(k + 1) % n].0;
        pts.extend(bulge_pts(p, q, b));
    }
    if closed && pts.len() > 1 && pts[0].distance(*pts.last().unwrap()) < 1e-9 {
        pts.pop();
    }
    Poly { pts, closed, layer }
}

// ------------------------------------------------------------------ PDF

#[derive(Clone, Debug)]
enum Obj {
    Null,
    Num(f64),
    Name(String),
    Str(#[allow(dead_code)] Vec<u8>),
    Arr(Vec<Obj>),
    Dict(Vec<(String, Obj)>),
    Ref(u32),
    Op(String),
}

impl Obj {
    fn get(&self, k: &str) -> Option<&Obj> {
        match self {
            Obj::Dict(v) => v.iter().find(|e| e.0 == k).map(|e| &e.1),
            _ => None,
        }
    }
    fn num(&self) -> Option<f64> {
        if let Obj::Num(n) = self { Some(*n) } else { None }
    }
}

struct Lexer<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Lexer<'a> {
    fn ws(&mut self) {
        while self.p < self.b.len() {
            let c = self.b[self.p];
            if c == b'%' {
                while self.p < self.b.len() && self.b[self.p] != b'\n' && self.b[self.p] != b'\r' {
                    self.p += 1;
                }
            } else if c.is_ascii_whitespace() || c == 0 {
                self.p += 1;
            } else {
                break;
            }
        }
    }
    fn delim(c: u8) -> bool {
        c.is_ascii_whitespace() || b"()<>[]{}/%".contains(&c)
    }
    /// Next object (refs "n g R" resolved by the caller via `Ref`).
    fn obj(&mut self) -> Option<Obj> {
        self.ws();
        let c = *self.b.get(self.p)?;
        match c {
            b'/' => {
                self.p += 1;
                let s = self.p;
                while self.p < self.b.len() && !Self::delim(self.b[self.p]) {
                    self.p += 1;
                }
                Some(Obj::Name(String::from_utf8_lossy(&self.b[s..self.p]).to_string()))
            }
            b'[' => {
                self.p += 1;
                let mut v = Vec::new();
                loop {
                    self.ws();
                    if self.b.get(self.p) == Some(&b']') {
                        self.p += 1;
                        break;
                    }
                    let o = self.obj()?;
                    v.push(o);
                    self.fold_refs(&mut v);
                }
                Some(Obj::Arr(v))
            }
            b'<' if self.b.get(self.p + 1) == Some(&b'<') => {
                self.p += 2;
                let mut items = Vec::new();
                loop {
                    self.ws();
                    if self.b.get(self.p) == Some(&b'>') && self.b.get(self.p + 1) == Some(&b'>') {
                        self.p += 2;
                        break;
                    }
                    let o = self.obj()?;
                    items.push(o);
                    self.fold_refs(&mut items);
                }
                let mut kv = Vec::new();
                let mut it = items.into_iter();
                while let (Some(k), Some(v)) = (it.next(), it.next()) {
                    if let Obj::Name(k) = k {
                        kv.push((k, v));
                    }
                }
                Some(Obj::Dict(kv))
            }
            b'<' => {
                self.p += 1;
                let s = self.p;
                while self.p < self.b.len() && self.b[self.p] != b'>' {
                    self.p += 1;
                }
                self.p += 1;
                Some(Obj::Str(self.b[s..self.p - 1].to_vec()))
            }
            b'(' => {
                let mut depth = 0;
                let s = self.p;
                while self.p < self.b.len() {
                    match self.b[self.p] {
                        b'\\' => self.p += 1,
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                self.p += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    self.p += 1;
                }
                Some(Obj::Str(self.b[s..self.p.min(self.b.len())].to_vec()))
            }
            b']' | b'>' | b')' | b'{' | b'}' => {
                self.p += 1;
                Some(Obj::Op(String::from(c as char)))
            }
            _ => {
                let s = self.p;
                while self.p < self.b.len() && !Self::delim(self.b[self.p]) {
                    self.p += 1;
                }
                let t = &self.b[s..self.p];
                if t.is_empty() {
                    self.p += 1;
                    return Some(Obj::Null);
                }
                let txt = String::from_utf8_lossy(t);
                match txt.parse::<f64>() {
                    Ok(n) => Some(Obj::Num(n)),
                    Err(_) => Some(if txt == "null" { Obj::Null } else { Obj::Op(txt.to_string()) }),
                }
            }
        }
    }
    /// "n g R" → Ref(n)
    fn fold_refs(&mut self, v: &mut Vec<Obj>) {
        if v.len() >= 3 {
            if let (Obj::Num(a), Obj::Num(_), Obj::Op(r)) = (&v[v.len() - 3], &v[v.len() - 2], &v[v.len() - 1]) {
                if r == "R" {
                    let id = *a as u32;
                    v.truncate(v.len() - 3);
                    v.push(Obj::Ref(id));
                }
            }
        }
    }
}

struct Pdf {
    objs: HashMap<u32, (Obj, Option<Vec<u8>>)>,
}

fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    if flate2::read::ZlibDecoder::new(data).read_to_end(&mut out).is_ok() {
        return Some(out);
    }
    out.clear();
    flate2::read::DeflateDecoder::new(data).read_to_end(&mut out).ok().map(|_| out)
}

impl Pdf {
    fn parse(b: &[u8]) -> Result<Pdf, String> {
        if !b.starts_with(b"%PDF") {
            return Err("não é um PDF".into());
        }
        let mut objs = HashMap::new();
        // scan "N G obj" markers
        let mut i = 0;
        while let Some(off) = find(&b[i..], b" obj") {
            let at = i + off;
            i = at + 4;
            // read back the "N G"
            let mut s = at;
            let mut spaces = 0;
            while s > 0 && spaces < 2 {
                s -= 1;
                if b[s] == b' ' || b[s] == b'\n' || b[s] == b'\r' {
                    if s + 1 < at && b[s + 1].is_ascii_digit() {
                        spaces += 1;
                    }
                }
                if !(b[s].is_ascii_digit() || b[s] == b' ') {
                    break;
                }
            }
            let head = String::from_utf8_lossy(&b[s..at]);
            let mut it = head.split_whitespace();
            let Some(num) = it.next().and_then(|x| x.parse::<u32>().ok()) else { continue };
            let mut lx = Lexer { b, p: at + 4 };
            let Some(obj) = lx.obj() else { continue };
            lx.ws();
            let mut stream = None;
            if b[lx.p..].starts_with(b"stream") {
                let mut s0 = lx.p + 6;
                if b.get(s0) == Some(&b'\r') {
                    s0 += 1;
                }
                if b.get(s0) == Some(&b'\n') {
                    s0 += 1;
                }
                let len = obj.get("Length").and_then(|l| l.num()).map(|l| l as usize);
                let e = match len {
                    Some(l) if s0 + l <= b.len() && b[s0 + l..].iter().take(20).any(|_| true) && find(&b[s0 + l..(s0 + l + 20).min(b.len())], b"endstream").is_some() => s0 + l,
                    _ => find(&b[s0..], b"endstream").map(|k| s0 + k).unwrap_or(b.len()),
                };
                stream = Some(b[s0..e].to_vec());
                i = e;
            }
            objs.insert(num, (obj, stream));
        }
        let mut pdf = Pdf { objs };
        pdf.expand_object_streams();
        if pdf.objs.is_empty() {
            return Err("PDF sem objetos legíveis (criptografado?)".into());
        }
        Ok(pdf)
    }

    fn decoded(&self, obj: &Obj, raw: &[u8]) -> Option<Vec<u8>> {
        let filt = obj.get("Filter").map(|f| self.resolve(f).clone());
        let names: Vec<String> = match filt {
            Some(Obj::Name(n)) => vec![n],
            Some(Obj::Arr(v)) => v.iter().filter_map(|x| if let Obj::Name(n) = x { Some(n.clone()) } else { None }).collect(),
            _ => vec![],
        };
        let mut data = raw.to_vec();
        for n in names {
            data = match n.as_str() {
                "FlateDecode" | "Fl" => inflate(&data)?,
                "ASCIIHexDecode" | "AHx" => {
                    let h: Vec<u8> = data.iter().copied().filter(|c| c.is_ascii_hexdigit()).collect();
                    h.chunks(2).map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap_or("0"), 16).unwrap_or(0)).collect()
                }
                _ => return None,
            };
        }
        Some(data)
    }

    fn expand_object_streams(&mut self) {
        let streams: Vec<(Obj, Vec<u8>)> = self
            .objs
            .values()
            .filter(|(o, s)| s.is_some() && matches!(o.get("Type"), Some(Obj::Name(n)) if n == "ObjStm"))
            .map(|(o, s)| (o.clone(), s.clone().unwrap()))
            .collect();
        for (o, raw) in streams {
            let Some(data) = self.decoded(&o, &raw) else { continue };
            let n = o.get("N").and_then(|x| x.num()).unwrap_or(0.0) as usize;
            let first = o.get("First").and_then(|x| x.num()).unwrap_or(0.0) as usize;
            let mut lx = Lexer { b: &data, p: 0 };
            let mut heads = Vec::new();
            for _ in 0..n {
                let (Some(Obj::Num(id)), Some(Obj::Num(off))) = (lx.obj(), lx.obj()) else { break };
                heads.push((id as u32, off as usize));
            }
            for (id, off) in heads {
                let mut l2 = Lexer { b: &data, p: first + off };
                if let Some(obj) = l2.obj() {
                    self.objs.entry(id).or_insert((obj, None));
                }
            }
        }
    }

    fn resolve<'s>(&'s self, o: &'s Obj) -> &'s Obj {
        let mut cur = o;
        for _ in 0..8 {
            match cur {
                Obj::Ref(id) => match self.objs.get(id) {
                    Some((x, _)) => cur = x,
                    None => return &Obj::Null,
                },
                _ => return cur,
            }
        }
        cur
    }

    fn stream_of(&self, o: &Obj) -> Option<(Obj, Vec<u8>)> {
        let Obj::Ref(id) = o else { return None };
        let (obj, raw) = self.objs.get(id)?;
        let data = self.decoded(obj, raw.as_ref()?)?;
        Some((obj.clone(), data))
    }

    fn pages(&self) -> Vec<u32> {
        // follow the page tree from the catalog so the order is right
        let mut out = Vec::new();
        let root = self.objs.values().find(|(o, _)| matches!(o.get("Type"), Some(Obj::Name(n)) if n == "Catalog")).and_then(|(o, _)| o.get("Pages").cloned());
        fn walk(pdf: &Pdf, o: &Obj, out: &mut Vec<u32>, depth: usize) {
            if depth > 32 {
                return;
            }
            let Obj::Ref(id) = o else { return };
            let node = pdf.resolve(o);
            match node.get("Type") {
                Some(Obj::Name(n)) if n == "Page" => out.push(*id),
                _ => {
                    if let Some(Obj::Arr(kids)) = node.get("Kids").map(|k| pdf.resolve(k)) {
                        for k in kids {
                            walk(pdf, k, out, depth + 1);
                        }
                    }
                }
            }
        }
        if let Some(r) = root {
            walk(self, &r, &mut out, 0);
        }
        if out.is_empty() {
            let mut ids: Vec<u32> = self.objs.iter().filter(|(_, (o, _))| matches!(o.get("Type"), Some(Obj::Name(n)) if n == "Page")).map(|(k, _)| *k).collect();
            ids.sort();
            out = ids;
        }
        out
    }
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

/// Affine 2D matrix [a b c d e f].
type M2 = [f64; 6];

fn mul(m: M2, n: M2) -> M2 {
    [m[0] * n[0] + m[1] * n[2], m[0] * n[1] + m[1] * n[3], m[2] * n[0] + m[3] * n[2], m[2] * n[1] + m[3] * n[3], m[4] * n[0] + m[5] * n[2] + n[4], m[4] * n[1] + m[5] * n[3] + n[5]]
}

fn apply(m: &M2, x: f64, y: f64) -> DVec2 {
    DVec2::new(m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

fn run_content(pdf: &Pdf, data: &[u8], resources: &Obj, ctm: M2, out: &mut Vec<Poly>, depth: usize) {
    if depth > 12 {
        return;
    }
    let mut lx = Lexer { b: data, p: 0 };
    let mut stack: Vec<Obj> = Vec::new();
    let mut gs = vec![ctm];
    let mut path: Vec<(Vec<DVec2>, bool)> = Vec::new();
    let mut cur = DVec2::ZERO;
    let n = |stack: &[Obj], k: usize| stack.get(stack.len().wrapping_sub(k)).and_then(|o| o.num()).unwrap_or(0.0);
    while let Some(o) = lx.obj() {
        let Obj::Op(op) = &o else {
            stack.push(o);
            continue;
        };
        let m = *gs.last().unwrap();
        match op.as_str() {
            "q" => gs.push(m),
            "Q" => {
                if gs.len() > 1 {
                    gs.pop();
                }
            }
            "cm" => {
                let c = [n(&stack, 6), n(&stack, 5), n(&stack, 4), n(&stack, 3), n(&stack, 2), n(&stack, 1)];
                *gs.last_mut().unwrap() = mul(c, m);
            }
            "m" => {
                cur = DVec2::new(n(&stack, 2), n(&stack, 1));
                path.push((vec![apply(&m, cur.x, cur.y)], false));
            }
            "l" => {
                cur = DVec2::new(n(&stack, 2), n(&stack, 1));
                if let Some(s) = path.last_mut() {
                    s.0.push(apply(&m, cur.x, cur.y));
                }
            }
            "c" | "v" | "y" => {
                let (p1, p2, p3) = match op.as_str() {
                    "c" => (DVec2::new(n(&stack, 6), n(&stack, 5)), DVec2::new(n(&stack, 4), n(&stack, 3)), DVec2::new(n(&stack, 2), n(&stack, 1))),
                    "v" => (cur, DVec2::new(n(&stack, 4), n(&stack, 3)), DVec2::new(n(&stack, 2), n(&stack, 1))),
                    _ => (DVec2::new(n(&stack, 4), n(&stack, 3)), DVec2::new(n(&stack, 2), n(&stack, 1)), DVec2::new(n(&stack, 2), n(&stack, 1))),
                };
                let p0 = cur;
                if let Some(s) = path.last_mut() {
                    for k in 1..=8 {
                        let t = k as f64 / 8.0;
                        let u = 1.0 - t;
                        let p = p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t);
                        s.0.push(apply(&m, p.x, p.y));
                    }
                }
                cur = p3;
            }
            "h" => {
                if let Some(s) = path.last_mut() {
                    s.1 = true;
                }
            }
            "re" => {
                let (x, y, w, h) = (n(&stack, 4), n(&stack, 3), n(&stack, 2), n(&stack, 1));
                path.push((vec![apply(&m, x, y), apply(&m, x + w, y), apply(&m, x + w, y + h), apply(&m, x, y + h)], true));
            }
            "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" => {
                let close = matches!(op.as_str(), "s" | "b" | "b*" | "f" | "F" | "f*" | "B" | "B*");
                for (pts, closed) in path.drain(..) {
                    if pts.len() >= 2 {
                        out.push(Poly { pts: pts.iter().map(|p| DVec3::new(p.x, p.y, 0.0)).collect(), closed: closed || (close && pts.len() > 2), layer: 0 });
                    }
                }
            }
            "n" => path.clear(),
            "Do" => {
                if let Some(Obj::Name(name)) = stack.last() {
                    let xo = resources.get("XObject").map(|x| pdf.resolve(x).clone());
                    if let Some(r) = xo.as_ref().and_then(|x| x.get(name)).cloned() {
                        if let Some((dict, data)) = pdf.stream_of(&r) {
                            if matches!(dict.get("Subtype"), Some(Obj::Name(s)) if s == "Form") {
                                let fm = match dict.get("Matrix").map(|x| pdf.resolve(x)) {
                                    Some(Obj::Arr(v)) if v.len() == 6 => [0, 1, 2, 3, 4, 5].map(|k| v[k].num().unwrap_or(0.0)),
                                    _ => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                                };
                                let res = dict.get("Resources").map(|x| pdf.resolve(x).clone()).unwrap_or_else(|| resources.clone());
                                run_content(pdf, &data, &res, mul(fm, m), out, depth + 1);
                            }
                        }
                    }
                }
            }
            "BI" => {
                // inline image: skip to EI
                if let Some(k) = find(&data[lx.p..], b"EI") {
                    lx.p += k + 2;
                }
            }
            _ => {}
        }
        stack.clear();
    }
}

pub fn pdf_page_count(bytes: &[u8]) -> usize {
    Pdf::parse(bytes).map(|p| p.pages().len()).unwrap_or(0)
}

/// Vector paths of one page (points; y up).
pub fn parse_pdf(bytes: &[u8], page: usize) -> Result<Drawing, String> {
    let pdf = Pdf::parse(bytes)?;
    let pages = pdf.pages();
    let pid = *pages.get(page).ok_or(format!("o PDF tem {} página(s)", pages.len()))?;
    let pg = pdf.resolve(&Obj::Ref(pid)).clone();
    let res = pg.get("Resources").map(|r| pdf.resolve(r).clone()).unwrap_or(Obj::Dict(vec![]));
    let contents: Vec<Obj> = match pg.get("Contents") {
        Some(Obj::Arr(v)) => v.clone(),
        Some(r @ Obj::Ref(_)) => match pdf.resolve(r) {
            Obj::Arr(v) => v.clone(),
            _ => vec![r.clone()],
        },
        _ => vec![],
    };
    let mut data = Vec::new();
    for c in &contents {
        if let Some((_, d)) = pdf.stream_of(c) {
            data.extend(d);
            data.push(b'\n');
        }
    }
    if data.is_empty() {
        return Err("a página não tem conteúdo vetorial legível".into());
    }
    let mut polys = Vec::new();
    run_content(&pdf, &data, &res, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], &mut polys, 0);
    if polys.is_empty() {
        return Err("a página não tem linhas (é uma imagem escaneada? use Importar imagem)".into());
    }
    let mut d = Drawing { source: "PDF".into(), unit_m: Some(0.0254 / 72.0), ..Default::default() };
    d.layer("PDF");
    d.model.polys = polys;
    Ok(d)
}

// ------------------------------------------------------------------ to HomeUp

/// Planar graph of the drawing: vertices merged, crossings split (spatial grid),
/// then faces rebuilt on every elevation plane when requested.
fn to_geom(polys: &[Poly], scale: f64, faces: bool, layer_tag: &[Option<usize>]) -> Geom {
    let mut g = Geom::default();
    let eps = 1e-6;
    let mut grid: HashMap<(i64, i64, i64), VId> = HashMap::new();
    let key = |p: DVec3| ((p.x / eps).round() as i64, (p.y / eps).round() as i64, (p.z / eps).round() as i64);
    let mut vid = |p: DVec3, g: &mut Geom| -> VId {
        *grid.entry(key(p)).or_insert_with(|| {
            g.verts.push(p);
            (g.verts.len() - 1) as VId
        })
    };
    // segments with their tag
    let mut segs: Vec<(DVec3, DVec3, Option<usize>)> = Vec::new();
    for p in polys {
        let pts: Vec<DVec3> = p.pts.iter().map(|q| *q * scale).collect();
        let n = pts.len();
        let m = if p.closed { n } else { n.saturating_sub(1) };
        for k in 0..m {
            let (a, b) = (pts[k], pts[(k + 1) % n]);
            if a.distance(b) > eps * 10.0 {
                segs.push((a, b, layer_tag.get(p.layer).copied().flatten()));
            }
        }
    }
    // split crossings (2D, same elevation) using a uniform grid
    if faces && !segs.is_empty() {
        let (mut lo, mut hi) = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
        for s in &segs {
            lo = lo.min(s.0.truncate()).min(s.1.truncate());
            hi = hi.max(s.0.truncate()).max(s.1.truncate());
        }
        let cells = ((segs.len() as f64).sqrt().ceil() as usize).clamp(1, 1024);
        let cs = ((hi - lo).max_element() / cells as f64).max(1e-6);
        let cell = |p: DVec2| (((p.x - lo.x) / cs) as i64, ((p.y - lo.y) / cs) as i64);
        let mut buckets: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (k, s) in segs.iter().enumerate() {
            let (a, b) = (cell(s.0.truncate()), cell(s.1.truncate()));
            for x in a.0.min(b.0)..=a.0.max(b.0) {
                for y in a.1.min(b.1)..=a.1.max(b.1) {
                    buckets.entry((x, y)).or_default().push(k);
                }
            }
        }
        let mut cuts: Vec<Vec<f64>> = vec![Vec::new(); segs.len()];
        let mut seen: HashSet<(usize, usize)> = HashSet::new();
        for list in buckets.values() {
            for (ii, &i) in list.iter().enumerate() {
                for &j in &list[ii + 1..] {
                    if !seen.insert((i.min(j), i.max(j))) {
                        continue;
                    }
                    let (p, r) = (segs[i].0, segs[i].1 - segs[i].0);
                    let (q, s) = (segs[j].0, segs[j].1 - segs[j].0);
                    if (p.z - q.z).abs() > 1e-6 || r.z.abs() > 1e-9 || s.z.abs() > 1e-9 {
                        continue;
                    }
                    let (r2, s2) = (r.truncate(), s.truncate());
                    let den = r2.perp_dot(s2);
                    let qp = (q - p).truncate();
                    if den.abs() < 1e-12 {
                        // collinear overlaps: cut at the other segment's ends
                        if qp.perp_dot(r2).abs() < 1e-9 * r2.length().max(1.0) {
                            for (o, t_of) in [(i, (q.truncate(), (q + s).truncate())), (j, (p.truncate(), (p + r).truncate()))] {
                                let (base, dir) = (segs[o].0.truncate(), (segs[o].1 - segs[o].0).truncate());
                                for e in [t_of.0, t_of.1] {
                                    let t = (e - base).dot(dir) / dir.length_squared();
                                    if t > 1e-9 && t < 1.0 - 1e-9 {
                                        cuts[o].push(t);
                                    }
                                }
                            }
                        }
                        continue;
                    }
                    let t = qp.perp_dot(s2) / den;
                    let u = qp.perp_dot(r2) / den;
                    if (-1e-9..=1.0 + 1e-9).contains(&t) && (-1e-9..=1.0 + 1e-9).contains(&u) {
                        if t > 1e-9 && t < 1.0 - 1e-9 {
                            cuts[i].push(t);
                        }
                        if u > 1e-9 && u < 1.0 - 1e-9 {
                            cuts[j].push(u);
                        }
                    }
                }
            }
        }
        let mut split = Vec::with_capacity(segs.len());
        for (k, s) in segs.iter().enumerate() {
            let mut ts = cuts[k].clone();
            ts.push(0.0);
            ts.push(1.0);
            ts.sort_by(|a, b| a.partial_cmp(b).unwrap());
            ts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
            for w in ts.windows(2) {
                split.push((s.0.lerp(s.1, w[0]), s.0.lerp(s.1, w[1]), s.2));
            }
        }
        segs = split;
    }
    let mut edge_set: HashSet<(VId, VId)> = HashSet::new();
    let mut zs: Vec<f64> = Vec::new();
    for (a, b, tag) in &segs {
        let (va, vb) = (vid(*a, &mut g), vid(*b, &mut g));
        if va == vb || !edge_set.insert((va.min(vb), va.max(vb))) {
            continue;
        }
        let mut e = Edge::new(va, vb);
        e.tag = *tag;
        g.edges.push(e);
        if (a.z - b.z).abs() < 1e-9 && !zs.iter().any(|z| (z - a.z).abs() < 1e-6) {
            zs.push(a.z);
        }
    }
    if faces {
        let all: HashSet<(VId, VId)> = g.edges.iter().map(|e| (e.v[0], e.v[1])).collect();
        for z in zs {
            g.recompute_plane(&Plane { n: DVec3::Z, d: z }, &all, &[], DVec3::new(0.0, 0.0, z + 1000.0));
        }
    }
    g
}

pub struct PendingCad {
    pub name: String,
    pub drawing: Drawing,
    /// Meters per drawing unit.
    pub unit_m: f64,
    /// PDF: drawing scale 1:N.
    pub pdf_scale: f64,
    pub faces: bool,
    pub bytes: Option<Vec<u8>>,
    pub page: usize,
    pub pages: usize,
}

pub const UNITS: [(&str, f64); 6] = [("milímetros", 0.001), ("centímetros", 0.01), ("decímetros", 0.1), ("metros", 1.0), ("polegadas", 0.0254), ("pés", 0.3048)];

impl HomeUpApp {
    pub(crate) fn start_cad_import(&mut self, path: &std::path::Path) {
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                self.message = format!("Erro ao ler: {e}");
                return;
            }
        };
        if ext == "dwg" {
            self.message = "DWG é um formato fechado da Autodesk: salve como DXF no seu CAD (ou no ODA File Converter, gratuito) e importe o DXF.".into();
            return;
        }
        let res = if ext == "pdf" {
            let pages = pdf_page_count(&bytes);
            parse_pdf(&bytes, 0).map(|d| PendingCad { name, drawing: d, unit_m: 0.0254 / 72.0, pdf_scale: 50.0, faces: false, bytes: Some(bytes), page: 0, pages })
        } else {
            parse_dxf(&String::from_utf8_lossy(&bytes)).map(|d| {
                let unit = d.unit_m.unwrap_or(0.001);
                let faces = d.segment_count() < 30000;
                PendingCad { name, drawing: d, unit_m: unit, pdf_scale: 1.0, faces, bytes: None, page: 0, pages: 1 }
            })
        };
        match res {
            Ok(p) => self.pending_cad = Some(p),
            Err(e) => self.message = format!("Falha ao importar: {e}"),
        }
    }

    pub(crate) fn cad_import_window(&mut self, ctx: &egui::Context) {
        let Some(p) = self.pending_cad.as_mut() else { return };
        let mut open = true;
        let mut go = false;
        let mut reload_page = None;
        egui::Window::new(format!("Importar planta ({})", p.drawing.source)).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(RichText::new(&p.name).strong());
            ui.label(format!("{} linhas, {} blocos, {} camadas", p.drawing.segment_count(), p.drawing.blocks.len(), p.drawing.layers.len()));
            if p.drawing.source == "PDF" {
                if p.pages > 1 {
                    ui.horizontal(|ui| {
                        ui.label("Página:");
                        let mut pg = p.page + 1;
                        if ui.add(egui::DragValue::new(&mut pg).range(1..=p.pages)).changed() {
                            reload_page = Some(pg - 1);
                        }
                        ui.weak(format!("de {}", p.pages));
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Escala do desenho  1 :");
                    ui.add(egui::DragValue::new(&mut p.pdf_scale).range(1.0..=10000.0).speed(1.0));
                });
                ui.weak("Ex.: 1:50 ou 1:100. Se não souber, importe e acerte com a Fita métrica (meça uma parede e digite a medida real).");
            } else {
                ui.horizontal(|ui| {
                    ui.label("Unidade do arquivo:");
                    egui::ComboBox::from_id_salt("cad-unit").selected_text(UNITS.iter().find(|u| (u.1 - p.unit_m).abs() < 1e-12).map(|u| u.0).unwrap_or("?")).show_ui(ui, |ui| {
                        for (n, v) in UNITS {
                            ui.selectable_value(&mut p.unit_m, v, n);
                        }
                    });
                    if p.drawing.unit_m.is_some() {
                        ui.weak("(lida do arquivo)");
                    }
                });
            }
            ui.checkbox(&mut p.faces, "Criar faces nas áreas fechadas (para usar Push/Pull)");
            if p.faces && p.drawing.segment_count() > 30000 {
                ui.weak("Desenho grande: criar faces pode demorar.");
            }
            ui.add_space(4.0);
            if ui.button(RichText::new("Importar").strong()).clicked() {
                go = true;
            }
        });
        if let Some(pg) = reload_page {
            if let Some(b) = p.bytes.clone() {
                match parse_pdf(&b, pg) {
                    Ok(d) => {
                        p.drawing = d;
                        p.page = pg;
                    }
                    Err(e) => self.message = e,
                }
            }
        }
        if !open {
            self.pending_cad = None;
            return;
        }
        if go {
            let p = self.pending_cad.take().unwrap();
            self.finish_cad_import(p);
        }
    }

    fn finish_cad_import(&mut self, p: PendingCad) {
        let scale = if p.drawing.source == "PDF" { p.unit_m * p.pdf_scale } else { p.unit_m };
        self.checkpoint_keep_sel();
        // layers → tags
        let mut layer_tag = Vec::new();
        for (name, color) in &p.drawing.layers {
            if name == "0" || name == "Defpoints" {
                layer_tag.push(None);
                continue;
            }
            let k = match self.doc.tags.iter().position(|t| &t.name == name) {
                Some(k) => k,
                None => {
                    self.doc.tags.push(Tag { name: name.clone(), visible: true, color: *color, ..Default::default() });
                    self.doc.tags.len() - 1
                }
            };
            layer_tag.push(Some(k));
        }
        // blocks → components (instances resolved by name)
        let base = self.doc.defs.len();
        let names: HashMap<String, usize> = p.drawing.blocks.iter().enumerate().map(|(k, b)| (b.name.clone(), base + k)).collect();
        let used: HashSet<&str> = std::iter::once(&p.drawing.model).chain(p.drawing.blocks.iter()).flat_map(|b| b.inserts.iter().map(|i| i.0.as_str())).collect();
        let place = |b: &Block, g: &mut Geom| {
            for (name, xf, layer) in &b.inserts {
                if let Some(&d) = names.get(name) {
                    let blk = &p.drawing.blocks[d - base];
                    let m = DMat4::from_scale(DVec3::splat(scale)) * *xf * DMat4::from_translation(-blk.base) * DMat4::from_scale(DVec3::splat(1.0 / scale));
                    g.instances.push(Instance { def: d, xf: m, tag: layer_tag.get(*layer).copied().flatten(), ..Default::default() });
                }
            }
        };
        for b in &p.drawing.blocks {
            let mut g = if used.contains(b.name.as_str()) { to_geom(&b.polys, scale, false, &layer_tag) } else { Geom::default() };
            place(b, &mut g);
            let name = if b.name.starts_with('*') { format!("Bloco {}", b.name.trim_start_matches('*')) } else { b.name.clone() };
            self.doc.defs.push(Definition { name, geom: g, group: false, face_me: false, procedure: None, ifc: None, author: None, light: None });
        }
        let mut g = to_geom(&p.drawing.model.polys, scale, p.faces, &layer_tag);
        place(&p.drawing.model, &mut g);
        // put the drawing's corner at the origin
        let mut lo = DVec3::splat(f64::MAX);
        for v in &g.verts {
            lo = lo.min(*v);
        }
        if lo.x < f64::MAX {
            let shift = DVec3::new(lo.x, lo.y, 0.0);
            for v in &mut g.verts {
                *v -= shift;
            }
            for i in &mut g.instances {
                i.xf = DMat4::from_translation(-shift) * i.xf;
            }
        }
        let nfaces = g.faces.len();
        self.doc.defs.push(Definition { name: format!("Planta {}", p.name), geom: g, group: false, face_me: false, procedure: None, ifc: None, author: None, light: None });
        let d = self.doc.defs.len() - 1;
        self.cache_dirty = true;
        self.set_tool(Tool::Place(d));
        self.message = format!("Planta {} ({}): {} linhas, {nfaces} faces — clique para posicionar; ajuste a escala com a Fita métrica se precisar.", p.name, p.drawing.source, p.drawing.segment_count());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DXF: &str = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n0\nLAYER\n2\nPAREDES\n62\n1\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nPORTA\n10\n0\n20\n0\n30\n0\n0\nARC\n8\nPAREDES\n10\n0\n20\n0\n30\n0\n40\n800\n50\n0\n51\n90\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n8\nPAREDES\n90\n4\n70\n1\n10\n0\n20\n0\n10\n4000\n20\n0\n10\n4000\n20\n3000\n10\n0\n20\n3000\n0\nLINE\n8\nPAREDES\n10\n2000\n20\n0\n30\n0\n11\n2000\n21\n3000\n31\n0\n0\nINSERT\n2\nPORTA\n10\n1000\n20\n0\n30\n0\n0\nENDSEC\n0\nEOF\n";

    #[test]
    fn dxf_room_becomes_two_faces() {
        let d = parse_dxf(DXF).unwrap();
        assert_eq!(d.unit_m, Some(0.001));
        assert_eq!(d.blocks.len(), 1);
        assert_eq!(d.model.inserts.len(), 1);
        assert!(d.layers.iter().any(|l| l.0 == "PAREDES" && l.1 == [220, 40, 40]));
        let g = to_geom(&d.model.polys, 0.001, true, &[None, None]);
        // the line splits the 4 × 3 m rectangle into two rooms
        assert_eq!(g.faces.len(), 2, "faces {}", g.faces.len());
        let area: f64 = g.faces.iter().map(|f| g.face_area(f)).sum();
        assert!((area - 12.0).abs() < 1e-6, "{area}");
    }

    #[test]
    fn bulges_make_arcs() {
        let p = poly_from(&[(DVec3::ZERO, 1.0), (DVec3::new(2.0, 0.0, 0.0), 0.0)], false, 0);
        // a semicircle of radius 1 through (1, ±1)
        assert!(p.pts.len() > 3);
        assert!(p.pts.iter().any(|q| (q.y.abs() - 1.0).abs() < 0.05 && (q.x - 1.0).abs() < 0.05));
    }

    #[test]
    fn pdf_paths_are_read() {
        // minimal PDF with an uncompressed content stream drawing a rectangle and a line
        let content = b"q 1 0 0 1 10 20 cm 0 0 100 50 re S 0 0 m 100 50 l S Q";
        let mut pdf = Vec::new();
        pdf.extend_from_slice(b"%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n");
        pdf.extend_from_slice(b"3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << >> >> endobj\n");
        pdf.extend_from_slice(format!("4 0 obj << /Length {} >>\nstream\n", content.len()).as_bytes());
        pdf.extend_from_slice(content);
        pdf.extend_from_slice(b"\nendstream endobj\ntrailer << /Root 1 0 R >>\n%%EOF\n");
        assert_eq!(pdf_page_count(&pdf), 1);
        let d = parse_pdf(&pdf, 0).unwrap();
        assert_eq!(d.model.polys.len(), 2);
        let r = &d.model.polys[0];
        assert!(r.closed && r.pts.len() == 4);
        assert!((r.pts[2] - DVec3::new(110.0, 70.0, 0.0)).length() < 1e-9);
        // compressed version
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut z, content).unwrap();
        let zc = z.finish().unwrap();
        let mut pdf2 = Vec::new();
        pdf2.extend_from_slice(b"%PDF-1.5\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /Contents 4 0 R >> endobj\n");
        pdf2.extend_from_slice(format!("4 0 obj << /Length {} /Filter /FlateDecode >>\nstream\n", zc.len()).as_bytes());
        pdf2.extend_from_slice(&zc);
        pdf2.extend_from_slice(b"\nendstream endobj\n%%EOF\n");
        assert_eq!(parse_pdf(&pdf2, 0).unwrap().model.polys.len(), 2);
    }
}
