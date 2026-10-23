//! Online asset library: furniture models (Poly Haven) and materials
//! (ambientCG, Poly Haven textures), all CC0. Network work happens on a
//! background thread; results are cached under %APPDATA%\HomeUp\cache.
//!
//! Poly Haven API terms: requests carry a "HomeUp" User-Agent and the UI names
//! Poly Haven as the source of its content.

use crate::app::{HomeUpApp, Tool};
use crate::chrome::fold;
use eframe::egui::{self, Color32, RichText, Vec2};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

const UA: &str = concat!("HomeUp/", env!("CARGO_PKG_VERSION"), " (modelador 3D desktop)");

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Source {
    PolyModels,
    PolyTextures,
    AmbientCg,
    PolyHdris,
}

/// Downloaded material maps (encoded JPG/PNG bytes).
#[derive(Default)]
pub struct TexSet {
    pub color: Vec<u8>,
    pub normal: Option<Vec<u8>>,
    pub rough: Option<Vec<u8>>,
    pub metal: Option<Vec<u8>>,
    pub ao: Option<Vec<u8>>,
    /// Packed AO/roughness/metal map (Poly Haven "arm": R = AO, G = rough, B = metal).
    pub arm: Option<Vec<u8>>,
}

/// Download progress per job key: (bytes received, bytes expected of the current file).
static PROGRESS: std::sync::LazyLock<std::sync::Mutex<HashMap<String, (u64, u64)>>> = std::sync::LazyLock::new(Default::default);
thread_local! {
    static CURRENT_JOB: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

pub fn progress(key: &str) -> Option<(u64, u64)> {
    PROGRESS.lock().ok()?.get(key).copied()
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::PolyModels => "Móveis · Poly Haven",
            Source::AmbientCg => "Materiais · ambientCG",
            Source::PolyTextures => "Texturas · Poly Haven",
            Source::PolyHdris => "Ambientes · Poly Haven",
        }
    }
    fn credit(self) -> &'static str {
        match self {
            Source::PolyModels | Source::PolyTextures | Source::PolyHdris => "Poly Haven (CC0)",
            Source::AmbientCg => "ambientCG (CC0)",
        }
    }
}

pub const MODEL_CATS: [(&str, &str); 12] = [
    ("furniture", "Móveis (todos)"),
    ("seating", "Assentos"),
    ("table", "Mesas"),
    ("shelves", "Estantes"),
    ("lighting", "Iluminação"),
    ("decorative", "Decoração"),
    ("plants", "Plantas"),
    ("appliances", "Eletrodomésticos"),
    ("office", "Escritório"),
    ("electronics", "Eletrônicos"),
    ("containers", "Recipientes"),
    ("dishes", "Louças"),
];

pub const HDRI_CATS: [(&str, &str); 7] = [
    ("outdoor", "Exteriores"),
    ("skies", "Céus"),
    ("indoor", "Interiores"),
    ("studio", "Estúdio"),
    ("urban", "Urbano"),
    ("nature", "Natureza"),
    ("sunrise-sunset", "Nascer/pôr do sol"),
];

pub const TEXTURE_CATS: [(&str, &str); 10] = [
    ("wood", "Madeira"),
    ("floor", "Pisos"),
    ("brick", "Tijolos"),
    ("concrete", "Concreto"),
    ("plaster", "Reboco"),
    ("tiles", "Azulejos"),
    ("fabric", "Tecidos"),
    ("stone", "Pedra"),
    ("metal", "Metal"),
    ("terrain", "Terreno"),
];

#[derive(Clone, Debug)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub thumb: String,
    pub tags: Vec<String>,
    /// Real-world size of one texture tile (meters).
    pub size_m: [f64; 2],
}

enum Job {
    List { src: Source, cat: String, query: String, offset: usize },
    Thumb { key: String, url: String },
    Model { id: String, name: String },
    Texture { src: Source, id: String, name: String, size_m: [f64; 2] },
    Hdri { id: String, name: String },
}

enum Done {
    Hdri { id: String, name: String, png: Vec<u8> },
    List { src: Source, cat: String, items: Vec<Item>, total: usize, append: bool },
    Thumb { key: String, w: u32, h: u32, rgba: Vec<u8> },
    Model { id: String, name: String, path: PathBuf },
    Texture { src: Source, id: String, name: String, set: TexSet, size_m: [f64; 2] },
    Failed { what: String, err: String },
}

pub struct Library {
    tx: Sender<Job>,
    rx: Receiver<Done>,
    pub tab: Source,
    pub model_cat: String,
    pub texture_cat: String,
    pub hdri_cat: String,
    pub search: String,
    pub acg_query: String,
    items: HashMap<(Source, String), Vec<Item>>,
    acg_total: usize,
    thumbs: HashMap<String, egui::TextureHandle>,
    requested: HashSet<String>,
    busy: HashSet<String>,
    pub status: String,
    pending: usize,
}

fn cache_dir() -> PathBuf {
    crate::filemenu::app_dir().join("cache")
}

fn http_get(url: &str) -> Result<Vec<u8>, String> {
    let mut resp = ureq::get(url).header("User-Agent", UA).call().map_err(|e| e.to_string())?;
    let total = resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    let key = CURRENT_JOB.with(|c| c.borrow().clone());
    let mut reader = resp.body_mut().with_config().limit(400 << 20).reader();
    let mut out = Vec::with_capacity(total.min(64 << 20) as usize);
    let mut buf = vec![0u8; 64 << 10];
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        if !key.is_empty() {
            if let Ok(mut p) = PROGRESS.lock() {
                p.insert(key.clone(), (out.len() as u64, total));
            }
        }
    }
    Ok(out)
}

fn http_json(url: &str) -> Result<serde_json::Value, String> {
    let b = http_get(url)?;
    serde_json::from_slice(&b).map_err(|e| e.to_string())
}

/// Download (or reuse) a file in the cache.
fn cached(rel: &str, url: &str) -> Result<PathBuf, String> {
    let p = cache_dir().join(rel);
    if p.exists() {
        return Ok(p);
    }
    let bytes = http_get(url)?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, &bytes).map_err(|e| e.to_string())?;
    Ok(p)
}

fn safe(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' { c } else { '_' }).collect()
}

fn list(src: Source, cat: &str, query: &str, offset: usize) -> Result<(Vec<Item>, usize), String> {
    match src {
        Source::PolyModels | Source::PolyTextures | Source::PolyHdris => {
            let t = match src {
                Source::PolyModels => "models",
                Source::PolyHdris => "hdris",
                _ => "textures",
            };
            let j = http_json(&format!("https://api.polyhaven.com/assets?t={t}&c={cat}"))?;
            let obj = j.as_object().ok_or("resposta inesperada do Poly Haven")?;
            let mut items: Vec<Item> = obj
                .iter()
                .map(|(id, v)| {
                    let dims = v.get("dimensions").and_then(|d| d.as_array()).map(|d| d.iter().filter_map(|x| x.as_f64()).collect::<Vec<_>>()).unwrap_or_default();
                    let size_m = if dims.len() >= 2 && dims[0] > 0.0 && dims[1] > 0.0 { [dims[0] / 1000.0, dims[1] / 1000.0] } else { [1.0, 1.0] };
                    Item {
                        id: id.clone(),
                        name: v.get("name").and_then(|n| n.as_str()).unwrap_or(id).to_string(),
                        thumb: format!("https://cdn.polyhaven.com/asset_img/thumbs/{id}.png?width=128&height=128"),
                        tags: v.get("tags").and_then(|t| t.as_array()).map(|t| t.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
                        size_m,
                    }
                })
                .collect();
            items.sort_by(|a, b| a.name.cmp(&b.name));
            let n = items.len();
            Ok((items, n))
        }
        Source::AmbientCg => {
            let q: String = query.split_whitespace().collect::<Vec<_>>().join("+");
            let url = format!(
                "https://ambientcg.com/api/v2/full_json?type=Material&sort=Popular&limit=60&offset={offset}&q={q}&include=previewData,dimensionsData"
            );
            let j = http_json(&url)?;
            let total = j.get("numberOfResults").and_then(|n| n.as_u64()).unwrap_or(0) as usize;
            let items = j
                .get("foundAssets")
                .and_then(|a| a.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| {
                            let id = v.get("assetId")?.as_str()?.to_string();
                            let dx = v.get("dimensionX").and_then(|x| x.as_f64()).filter(|x| *x > 0.0).unwrap_or(100.0);
                            let dy = v.get("dimensionY").and_then(|x| x.as_f64()).filter(|x| *x > 0.0).unwrap_or(dx);
                            let thumb = v
                                .get("previewImage")
                                .and_then(|p| p.get("128-PNG").or(p.get("256-PNG")))
                                .and_then(|u| u.as_str())
                                .map(str::to_string)
                                .unwrap_or_default();
                            Some(Item {
                                name: id.clone(),
                                tags: v.get("tags").and_then(|t| t.as_array()).map(|t| t.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
                                id,
                                thumb,
                                size_m: [dx / 100.0, dy / 100.0],
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            Ok((items, total))
        }
    }
}

pub(crate) fn fetch_model(id: &str) -> Result<PathBuf, String> {
    let j = http_json(&format!("https://api.polyhaven.com/files/{id}"))?;
    let g = j.get("gltf").and_then(|g| g.get("1k").or(g.get("2k"))).and_then(|r| r.get("gltf")).ok_or("modelo sem glTF")?;
    let url = g.get("url").and_then(|u| u.as_str()).ok_or("URL ausente")?;
    let dir = format!("polyhaven/models/{}", safe(id));
    let main = cached(&format!("{dir}/{}", url.rsplit('/').next().unwrap_or("model.gltf")), url)?;
    if let Some(inc) = g.get("include").and_then(|i| i.as_object()) {
        for (rel, v) in inc {
            if let Some(u) = v.get("url").and_then(|u| u.as_str()) {
                // keep the relative layout (textures/...) expected by the .gltf
                let rel: String = rel.split('/').map(safe).collect::<Vec<_>>().join("/");
                cached(&format!("{dir}/{rel}"), u)?;
            }
        }
    }
    Ok(main)
}

pub(crate) fn fetch_texture(src: Source, id: &str) -> Result<TexSet, String> {
    match src {
        Source::PolyTextures => {
            let j = http_json(&format!("https://api.polyhaven.com/files/{id}"))?;
            let map = |key: &str, tag: &str| -> Option<Vec<u8>> {
                let url = j.get(key)?.get("1k")?.get("jpg")?.get("url")?.as_str()?;
                let p = cached(&format!("polyhaven/textures/{}_{tag}_1k.jpg", safe(id)), url).ok()?;
                std::fs::read(p).ok()
            };
            let color = map("Diffuse", "diff").ok_or("textura sem mapa difuso")?;
            Ok(TexSet { color, normal: map("nor_gl", "nor_gl"), arm: map("arm", "arm"), ..Default::default() })
        }
        _ => {
            let zip_path = cached(&format!("ambientcg/{}_1K-JPG.zip", safe(id)), &format!("https://ambientcg.com/get?file={id}_1K-JPG.zip"))?;
            let data = std::fs::read(zip_path).map_err(|e| e.to_string())?;
            let mut z = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| e.to_string())?;
            let mut set = TexSet::default();
            for i in 0..z.len() {
                let mut f = z.by_index(i).map_err(|e| e.to_string())?;
                let n = f.name().map_err(|e| e.to_string())?.to_lowercase();
                if !(n.ends_with(".jpg") || n.ends_with(".png")) {
                    continue;
                }
                let stem = n.rsplit_once('.').map(|s| s.0).unwrap_or(&n).to_string();
                let slot = if stem.ends_with("_color") {
                    &mut set.color
                } else {
                    let o = if stem.ends_with("_normalgl") {
                        &mut set.normal
                    } else if stem.ends_with("_roughness") {
                        &mut set.rough
                    } else if stem.ends_with("_metalness") {
                        &mut set.metal
                    } else if stem.ends_with("_ambientocclusion") {
                        &mut set.ao
                    } else {
                        continue;
                    };
                    o.get_or_insert_with(Vec::new)
                };
                f.read_to_end(slot).map_err(|e| e.to_string())?;
            }
            if set.color.is_empty() {
                return Err("pacote sem mapa de cor".into());
            }
            Ok(set)
        }
    }
}

/// Download a Poly Haven HDRI (2K) and tone-map it to an 8-bit PNG panorama.
pub(crate) fn fetch_hdri(id: &str) -> Result<Vec<u8>, String> {
    let j = http_json(&format!("https://api.polyhaven.com/files/{id}"))?;
    let h = j.get("hdri").ok_or("HDRI ausente")?;
    let url = h.get("2k").or(h.get("1k")).and_then(|r| r.get("hdr")).and_then(|f| f.get("url")).and_then(|u| u.as_str()).ok_or("HDRI sem arquivo .hdr")?;
    let p = cached(&format!("polyhaven/hdris/{}_2k.hdr", safe(id)), url)?;
    let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
    tonemap_hdr(&bytes)
}

/// HDR panorama (Radiance .hdr or OpenEXR) → sRGB PNG (exposure-normalized Reinhard).
pub(crate) fn tonemap_hdr(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?.to_rgb32f();
    let (w, h) = img.dimensions();
    // normalize by the average luminance so dim and bright skies look alike
    let mut sum = 0.0f64;
    for p in img.pixels() {
        sum += (0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]).max(1e-4).ln() as f64;
    }
    let avg = (sum / (w as f64 * h as f64).max(1.0)).exp() as f32;
    let k = 0.30 / avg.max(1e-4);
    let mut out = image::RgbaImage::new(w, h);
    for (x, y, p) in img.enumerate_pixels() {
        let c = |v: f32| {
            let v = v * k;
            let t = v / (1.0 + v);
            (t.powf(1.0 / 2.2) * 255.0).round().clamp(0.0, 255.0) as u8
        };
        out.put_pixel(x, y, image::Rgba([c(p[0]), c(p[1]), c(p[2]), 255]));
    }
    let mut buf = std::io::Cursor::new(Vec::new());
    out.write_to(&mut buf, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(buf.into_inner())
}

fn worker(rx: Receiver<Job>, tx: Sender<Done>, ctx: egui::Context) {
    for job in rx {
        let key = match &job {
            Job::Model { id, .. } | Job::Texture { id, .. } | Job::Hdri { id, .. } => id.clone(),
            _ => String::new(),
        };
        CURRENT_JOB.with(|c| *c.borrow_mut() = key.clone());
        let done = match job {
            Job::Hdri { id, name } => match fetch_hdri(&id) {
                Ok(png) => Done::Hdri { id, name, png },
                Err(err) => Done::Failed { what: name, err },
            },
            Job::List { src, cat, query, offset } => match list(src, &cat, &query, offset) {
                Ok((items, total)) => Done::List { src, cat, items, total, append: offset > 0 },
                Err(err) => Done::Failed { what: "lista".into(), err },
            },
            Job::Thumb { key, url } => {
                let rel = format!("thumbs/{}.png", safe(&key));
                match cached(&rel, &url).and_then(|p| std::fs::read(p).map_err(|e| e.to_string())).and_then(|b| image::load_from_memory(&b).map_err(|e| e.to_string())) {
                    Ok(img) => {
                        let t = img.thumbnail(96, 96).to_rgba8();
                        Done::Thumb { key, w: t.width(), h: t.height(), rgba: t.into_raw() }
                    }
                    Err(err) => Done::Failed { what: format!("miniatura {key}"), err },
                }
            }
            Job::Model { id, name } => match fetch_model(&id) {
                Ok(path) => Done::Model { id, name, path },
                Err(err) => Done::Failed { what: name, err },
            },
            Job::Texture { src, id, name, size_m } => match fetch_texture(src, &id) {
                Ok(set) => Done::Texture { src, id, name, set, size_m },
                Err(err) => Done::Failed { what: name, err },
            },
        };
        if !key.is_empty() {
            if let Ok(mut p) = PROGRESS.lock() {
                p.remove(&key);
            }
        }
        let _ = tx.send(done);
        ctx.request_repaint();
    }
}

impl Library {
    pub fn new(ctx: &egui::Context) -> Library {
        let (jtx, jrx) = channel();
        let (dtx, drx) = channel();
        let c = ctx.clone();
        // a few parallel workers share the job queue
        let jrx = std::sync::Arc::new(std::sync::Mutex::new(jrx));
        for _ in 0..4 {
            let rx = jrx.clone();
            let tx = dtx.clone();
            let c = c.clone();
            std::thread::spawn(move || {
                let (ltx, lrx) = channel();
                std::thread::spawn(move || worker(lrx, tx, c));
                loop {
                    let job = rx.lock().unwrap().recv();
                    match job {
                        Ok(j) => {
                            if ltx.send(j).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }
        Library {
            tx: jtx,
            rx: drx,
            tab: Source::PolyModels,
            model_cat: "furniture".into(),
            texture_cat: "wood".into(),
            hdri_cat: "outdoor".into(),
            search: String::new(),
            acg_query: String::new(),
            items: HashMap::new(),
            acg_total: 0,
            thumbs: HashMap::new(),
            requested: HashSet::new(),
            busy: HashSet::new(),
            status: String::new(),
            pending: 0,
        }
    }

    fn send(&mut self, j: Job) {
        self.pending += 1;
        let _ = self.tx.send(j);
    }

    fn cat_key(&self) -> String {
        match self.tab {
            Source::PolyModels => self.model_cat.clone(),
            Source::PolyTextures => self.texture_cat.clone(),
            Source::PolyHdris => self.hdri_cat.clone(),
            Source::AmbientCg => format!("q:{}", self.acg_query),
        }
    }
}

impl HomeUpApp {
    fn library_mut(&mut self, ctx: &egui::Context) -> &mut Library {
        self.library.get_or_insert_with(|| Library::new(ctx))
    }

    /// Process finished downloads (called every frame).
    pub(crate) fn poll_library(&mut self, ctx: &egui::Context) {
        let Some(lib) = self.library.as_mut() else { return };
        let mut done = Vec::new();
        while let Ok(d) = lib.rx.try_recv() {
            lib.pending = lib.pending.saturating_sub(1);
            done.push(d);
        }
        for d in done {
            match d {
                Done::List { src, cat, items, total, append } => {
                    let lib = self.library.as_mut().unwrap();
                    if src == Source::AmbientCg {
                        lib.acg_total = total;
                    }
                    let e = lib.items.entry((src, cat)).or_default();
                    if append {
                        e.extend(items);
                    } else {
                        *e = items;
                    }
                    lib.status.clear();
                }
                Done::Thumb { key, w, h, rgba } => {
                    let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                    let tex = ctx.load_texture(format!("lib-{key}"), img, egui::TextureOptions::LINEAR);
                    self.library.as_mut().unwrap().thumbs.insert(key, tex);
                }
                Done::Model { id, name, path } => {
                    self.library.as_mut().unwrap().busy.remove(&id);
                    self.insert_library_model(&name, &path);
                }
                Done::Texture { src, id, name, set, size_m } => {
                    self.library.as_mut().unwrap().busy.remove(&id);
                    self.insert_library_material(src, &name, set, size_m);
                }
                Done::Hdri { id, name, png } => {
                    self.library.as_mut().unwrap().busy.remove(&id);
                    self.insert_environment(&name, png, "Poly Haven (CC0)");
                }
                Done::Failed { what, err } => {
                    let lib = self.library.as_mut().unwrap();
                    if !what.starts_with("miniatura") {
                        lib.busy.clear();
                        lib.status = format!("Falha ({what}): {err}");
                    }
                }
            }
        }
        if self.library.as_ref().is_some_and(|l| l.pending > 0) {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
    }

    pub(crate) fn insert_library_model(&mut self, name: &str, path: &std::path::Path) {
        match crate::import::load(path) {
            Ok(imp) => {
                let mut opts = crate::import::default_opts(path, &imp);
                // scanned/sculpted library models: hide curvature edges like SketchUp soft edges
                opts.smooth_deg = 60.0;
                self.checkpoint_keep_sel();
                let mut g = crate::import::to_geom(&imp, &opts, &mut self.doc.materials, &mut self.doc.textures);
                crate::import::recenter(&mut g);
                self.doc.defs.push(crate::doc::Definition { name: name.to_string(), geom: g, group: false, face_me: false, procedure: None, ifc: Some("IfcFurniture".into()), author: Some("Poly Haven (CC0)".into()), light: None });
                let d = self.doc.defs.len() - 1;
                self.set_tool(Tool::Place(d));
                self.message = format!("{name} (Poly Haven, CC0) — clique para posicionar.");
            }
            Err(e) => self.message = format!("Falha ao importar {name}: {e}"),
        }
    }

    pub(crate) fn insert_library_material(&mut self, src: Source, name: &str, set: TexSet, size_m: [f64; 2]) {
        match crate::tex::TextureData::from_bytes(name, set.color) {
            Ok(t) => {
                self.checkpoint_keep_sel();
                let mut m = crate::doc::Material::color(name, t.average(), 1.0);
                self.doc.textures.push(t);
                m.texture = Some(self.doc.textures.len() - 1);
                m.tex_size = size_m;
                m.source = src.credit().to_string();
                let mut add = |bytes: Option<Vec<u8>>, suffix: &str| -> Option<usize> {
                    let t = crate::tex::TextureData::from_bytes(&format!("{name} ({suffix})"), bytes?).ok()?;
                    self.doc.textures.push(t);
                    Some(self.doc.textures.len() - 1)
                };
                m.normal_tex = add(set.normal, "normal");
                if let Some(arm) = add(set.arm, "AO/rugosidade/metal") {
                    (m.ao_tex, m.rough_tex, m.metal_tex) = (Some(arm), Some(arm), Some(arm));
                    (m.ao_ch, m.rough_ch, m.metal_ch) = (0, 1, 2);
                } else {
                    m.rough_tex = add(set.rough, "rugosidade");
                    m.metal_tex = add(set.metal, "metal");
                    m.ao_tex = add(set.ao, "AO");
                }
                if m.rough_tex.is_some() {
                    m.roughness = 1.0;
                }
                if m.metal_tex.is_some() {
                    m.metalness = 1.0;
                }
                self.doc.materials.push(m);
                self.cur_mat = Some(self.doc.materials.len() - 1);
                self.set_tool(Tool::Paint);
                self.message = format!("Material {name} ({}) pronto — clique nas faces para pintar.", src.credit());
            }
            Err(e) => self.message = format!("Falha no material {name}: {e}"),
        }
    }

    pub(crate) fn library_ui(&mut self, ui: &mut egui::Ui) {
        // wrapped grids inside a resizable side panel must not widen it
        let w = ui.available_width();
        ui.set_max_width(w);
        let ctx = ui.ctx().clone();
        let lib = self.library_mut(&ctx);
        ui.horizontal_wrapped(|ui| {
            for s in [Source::PolyModels, Source::AmbientCg, Source::PolyTextures, Source::PolyHdris] {
                ui.selectable_value(&mut lib.tab, s, s.label());
            }
        });
        ui.separator();
        match lib.tab {
            Source::PolyModels | Source::PolyTextures | Source::PolyHdris => {
                let (cats, cur): (&[(&str, &str)], &mut String) = match lib.tab {
                    Source::PolyModels => (&MODEL_CATS, &mut lib.model_cat),
                    Source::PolyHdris => (&HDRI_CATS, &mut lib.hdri_cat),
                    _ => (&TEXTURE_CATS, &mut lib.texture_cat),
                };
                let label = cats.iter().find(|c| c.0 == cur.as_str()).map(|c| c.1).unwrap_or("");
                egui::ComboBox::from_id_salt("lib-cat").selected_text(label).show_ui(ui, |ui| {
                    for (k, n) in cats {
                        ui.selectable_value(cur, k.to_string(), *n);
                    }
                });
                ui.add(egui::TextEdit::singleline(&mut lib.search).hint_text("Filtrar por nome ou tag..."));
            }
            Source::AmbientCg => {
                ui.horizontal(|ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut lib.acg_query).hint_text("Buscar (ex.: wood, tiles, marble)").desired_width(170.0));
                    if (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) | ui.button("Buscar").clicked() {
                        let key = (Source::AmbientCg, format!("q:{}", lib.acg_query));
                        lib.items.remove(&key);
                        lib.requested.remove(&format!("list:{}", key.1));
                    }
                });
            }
        }
        // request the list once per (tab, category/query)
        let key = (lib.tab, lib.cat_key());
        let req = format!("list:{:?}:{}", key.0, key.1);
        if !lib.items.contains_key(&key) && !lib.requested.contains(&req) {
            lib.requested.insert(req);
            let (src, cat) = (lib.tab, if lib.tab == Source::AmbientCg { String::new() } else { key.1.clone() });
            let query = lib.acg_query.clone();
            lib.send(Job::List { src, cat: if src == Source::AmbientCg { key.1.clone() } else { cat }, query, offset: 0 });
            lib.status = "Carregando…".into();
        }
        if !lib.status.is_empty() {
            ui.colored_label(if lib.status.starts_with("Falha") { Color32::from_rgb(180, 30, 30) } else { Color32::from_gray(90) }, &lib.status);
        }
        let q = fold(&lib.search);
        let items: Vec<Item> = lib
            .items
            .get(&key)
            .map(|v| {
                v.iter()
                    .filter(|i| lib.tab == Source::AmbientCg || q.is_empty() || fold(&i.name).contains(&q) || i.tags.iter().any(|t| fold(t).contains(&q)))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let mut clicked: Option<Item> = None;
        egui::ScrollArea::vertical().id_salt("lib-grid").auto_shrink([false, false]).max_height((ui.available_height() - 40.0).max(120.0)).show(ui, |ui| {
            ui.set_max_width(w);
            // explicit rows: horizontal_wrapped does not wrap nested vertical layouts
            let cols = ((w / 92.0).floor() as usize).max(1);
            for row in items.chunks(cols) {
            ui.horizontal(|ui| {
                for it in row {
                    let tkey = format!("{:?}-{}", key.0, it.id);
                    if !lib.thumbs.contains_key(&tkey) && !lib.requested.contains(&tkey) && !it.thumb.is_empty() {
                        lib.requested.insert(tkey.clone());
                        let url = it.thumb.clone();
                        lib.pending += 1;
                        let _ = lib.tx.send(Job::Thumb { key: tkey.clone(), url });
                    }
                    ui.vertical(|ui| {
                        ui.set_width(84.0);
                        let size = Vec2::splat(80.0);
                        let busy = lib.busy.contains(&it.id);
                        let resp = match lib.thumbs.get(&tkey) {
                            Some(t) => ui.add(egui::Image::new((t.id(), size)).sense(egui::Sense::click()).corner_radius(4.0)),
                            None => {
                                let (r, resp) = ui.allocate_exact_size(size, egui::Sense::click());
                                ui.painter().rect_filled(r, 4.0, Color32::from_gray(225));
                                resp
                            }
                        };
                        if busy {
                            let r = resp.rect;
                            ui.painter().rect_filled(r, 4.0, Color32::from_rgba_unmultiplied(255, 255, 255, 170));
                            let (text, frac) = match progress(&it.id) {
                                Some((got, total)) if total > 0 => (format!("{}%", got * 100 / total), Some(got as f32 / total as f32)),
                                Some((got, _)) => (format!("{:.1} MB", got as f64 / 1048576.0), None),
                                None => ("Baixando…".to_string(), None),
                            };
                            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(13.0), Color32::BLACK);
                            if let Some(f) = frac {
                                let bar = egui::Rect::from_min_size(egui::pos2(r.min.x + 6.0, r.max.y - 10.0), Vec2::new(r.width() - 12.0, 4.0));
                                ui.painter().rect_filled(bar, 2.0, Color32::from_gray(200));
                                ui.painter().rect_filled(egui::Rect::from_min_size(bar.min, Vec2::new(bar.width() * f.clamp(0.0, 1.0), 4.0)), 2.0, Color32::from_rgb(40, 110, 220));
                            }
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                        }
                        let tip = format!("{}\n{}", it.name, it.tags.iter().take(6).cloned().collect::<Vec<_>>().join(", "));
                        if resp.on_hover_text(tip).clicked() && !busy {
                            clicked = Some(it.clone());
                        }
                        let short: String = it.name.chars().take(14).collect();
                        ui.label(RichText::new(short).small());
                    });
                }
            });
            }
            if lib.tab == Source::AmbientCg && items.len() < lib.acg_total && !items.is_empty() && ui.button("Carregar mais").clicked() {
                let query = lib.acg_query.clone();
                let n = items.len();
                lib.send(Job::List { src: Source::AmbientCg, cat: key.1.clone(), query, offset: n });
            }
        });
        if let Some(it) = clicked {
            lib.busy.insert(it.id.clone());
            let job = match lib.tab {
                Source::PolyModels => Job::Model { id: it.id.clone(), name: it.name.clone() },
                Source::PolyHdris => Job::Hdri { id: it.id.clone(), name: it.name.clone() },
                src => Job::Texture { src, id: it.id.clone(), name: it.name.clone(), size_m: it.size_m },
            };
            lib.send(job);
            lib.status = format!("Baixando {}…", it.name);
        }
        ui.separator();
        match lib.tab {
            Source::AmbientCg => {
                ui.horizontal(|ui| {
                    ui.weak("Materiais de");
                    ui.hyperlink_to("ambientCG", "https://ambientcg.com");
                    ui.weak("— CC0");
                });
            }
            _ => {
                ui.horizontal(|ui| {
                    ui.weak("Conteúdo de");
                    ui.hyperlink_to("Poly Haven", "https://polyhaven.com");
                    ui.weak("— CC0");
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tonemaps_exr_and_hdr() {
        let img = image::Rgb32FImage::from_fn(16, 8, |x, y| image::Rgb([x as f32 * 0.5, y as f32 * 2.0, 4.0]));
        for fmt in [image::ImageFormat::OpenExr, image::ImageFormat::Hdr] {
            let mut buf = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb32F(img.clone()).write_to(&mut buf, fmt).unwrap();
            let png = tonemap_hdr(buf.get_ref()).unwrap();
            let out = image::load_from_memory(&png).unwrap();
            assert_eq!((out.width(), out.height()), (16, 8), "{fmt:?}");
        }
    }
    #[test]
    fn safe_names() {
        assert_eq!(safe("textures/Arm Chair_01.jpg"), "textures_Arm_Chair_01.jpg");
        assert!(UA.starts_with("HomeUp/"));
    }
}
