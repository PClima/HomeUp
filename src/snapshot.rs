//! Offscreen self-test: `HOMEUP_SNAPSHOT=out.png` renders the window off-screen
//! (without focus or user input), saves a PNG with egui's own screenshot and exits.
//! `HOMEUP_SNAPSHOT_SETUP` takes comma-separated flags to prepare the scene:
//! `demo`, `more`, `flyout`, `search=<text>`, `panel=<entity|components|materials|scenes>`,
//! `select=group`, `edit-group`, `ortho`, `top`.

use crate::app::{HomeUpApp, Tool};
use crate::chrome::PanelKind;
use crate::geom::Sel;
use crate::tools::circle_points;
use eframe::egui;
use glam::DVec3;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct Snapshot {
    pub path: String,
    pub setup: Vec<String>,
    pub frame: u32,
    pub done: Arc<AtomicBool>,
    pub requested: bool,
}

impl Snapshot {
    pub fn from_env() -> Option<Snapshot> {
        let path = std::env::var("HOMEUP_SNAPSHOT").ok()?;
        let setup = std::env::var("HOMEUP_SNAPSHOT_SETUP")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Some(Snapshot { path, setup, frame: 0, done: Arc::new(AtomicBool::new(false)), requested: false })
    }
}

fn save_png(path: &str, img: &egui::ColorImage) -> Result<(), String> {
    let f = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), img.size[0] as u32, img.size[1] as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
    w.write_image_data(&bytes).map_err(|e| e.to_string())
}

impl HomeUpApp {
    /// Build a small demo model through the document API.
    pub(crate) fn build_demo(&mut self) {
        let eye = DVec3::new(10.0, -12.0, 8.0);
        let g = &mut self.doc.root;
        g.add_polygon(
            &[DVec3::new(0.0, 0.0, 0.0), DVec3::new(4.0, 0.0, 0.0), DVec3::new(4.0, 3.0, 0.0), DVec3::new(0.0, 3.0, 0.0)],
            None,
            false,
            eye,
        );
        g.push_pull(0, 2.5, false, eye);
        let c = g.new_curve();
        g.add_polygon(&circle_points(DVec3::new(2.0, 1.5, 2.5), DVec3::Z, DVec3::X, 0.6, 24), Some(c), true, eye);
        if let Some(fi) = g.faces.iter().position(|f| f.normal.z > 0.9 && f.holes.is_empty() && f.outer.len() == 24) {
            g.push_pull(fi, 1.0, false, eye);
        }
        let brick = self.doc.materials.iter().position(|m| m.name == "Tijolo");
        let glass = self.doc.materials.iter().position(|m| m.name == "Vidro");
        let g = &mut self.doc.root;
        for f in &mut g.faces {
            if f.normal.z.abs() < 0.1 && f.outer.len() == 4 {
                f.material = brick;
            }
        }
        // a glass pane next to the house
        g.add_polygon(
            &[DVec3::new(5.0, 0.0, 0.0), DVec3::new(6.5, 0.0, 0.0), DVec3::new(6.5, 0.0, 1.8), DVec3::new(5.0, 0.0, 1.8)],
            None,
            false,
            eye,
        );
        if let Some(f) = g.faces.last_mut() {
            f.material = glass;
        }
        self.cache_dirty = true;
        self.dirty = true;
    }

    pub(crate) fn snapshot_frame(&mut self, ctx: &egui::Context) {
        let Some(s) = self.snapshot.as_mut() else { return };
        s.frame += 1;
        let frame = s.frame;
        if frame == 2 {
            let setup = s.setup.clone();
            for flag in setup {
                let (k, v) = flag.split_once('=').unwrap_or((flag.as_str(), ""));
                match k {
                    "demo" => {
                        self.build_demo();
                        self.zoom_extents();
                    }
                    "shapes" => {
                        let z = DVec3::Z;
                        self.commit_center_arc(DVec3::new(-3.0, -3.0, 0.0), z, DVec3::new(-2.0, -3.0, 0.0), 270f64.to_radians(), true);
                        self.commit_center_arc(DVec3::new(-3.0, 0.0, 0.0), z, DVec3::new(-2.0, 0.0, 0.0), 120f64.to_radians(), false);
                        self.commit_rot_rect(DVec3::new(-6.0, -4.0, 0.0), DVec3::new(-4.5, -2.5, 0.0), z, 1.0);
                        if let Some((c, n, sw)) = crate::tools2::arc3(DVec3::new(-7.0, 0.0, 0.0), DVec3::new(-6.0, 1.0, 0.0), DVec3::new(-5.0, 0.0, 0.0)) {
                            let pts = crate::tools2::sweep_points(c, n, DVec3::new(-7.0, 0.0, 0.0), sw, 12);
                            let local = pts.clone();
                            let g = self.active_mut();
                            let cid = g.new_curve();
                            let segs: Vec<(DVec3, DVec3)> = local.windows(2).map(|w| (w[0], w[1])).collect();
                            g.add_segments(&segs, Some(cid), true, DVec3::new(0.0, -10.0, 10.0));
                        }
                        self.zoom_extents();
                    }
                    "modeling" => {
                        let eye = DVec3::new(10.0, -12.0, 8.0);
                        for (o, s) in [(DVec3::ZERO, 2.0), (DVec3::splat(1.0), 2.0)] {
                            let mut g = crate::geom::Geom::default();
                            g.add_polygon(&[o, o + DVec3::X * s, o + DVec3::new(s, s, 0.0), o + DVec3::Y * s], None, false, eye);
                            g.push_pull(0, s, false, eye);
                            let n = g.faces.len();
                            let base = self.doc.root.faces.len();
                            let sel = self.doc.root.append(&g, glam::DMat4::IDENTITY, None);
                            let _ = (n, base);
                            self.doc.make_def(&[], &sel, true);
                        }
                        let k = self.doc.root.instances.len();
                        self.run_solid(crate::app::Tool::Subtract, k - 1, k - 2);
                        // Follow Me: circular profile swept along an L path
                        let mut g = crate::geom::Geom::default();
                        let c = g.new_curve();
                        g.add_polygon(&circle_points(DVec3::new(5.0, 0.0, 0.5), DVec3::X, DVec3::Y, 0.3, 16), Some(c), true, DVec3::new(-10.0, 0.0, 0.5));
                        let path = [DVec3::new(5.0, 0.0, 0.5), DVec3::new(8.0, 0.0, 0.5), DVec3::new(8.0, 3.0, 0.5), DVec3::new(8.0, 3.0, 2.5)];
                        if let Err(e) = crate::modops::follow_me(&mut g, 0, &path, false) {
                            eprintln!("follow me: {e}");
                        }
                        self.doc.root.append(&g, glam::DMat4::IDENTITY, Some(6));
                        self.cache_dirty = true;
                        self.zoom_extents();
                    }
                    "annot" => {
                        self.build_demo();
                        self.doc.dims.push(crate::doc::Dimension { a: DVec3::ZERO, b: DVec3::new(4.0, 0.0, 0.0), off: DVec3::new(0.0, -0.8, 0.0) });
                        self.doc.labels.push(crate::doc::Label { anchor: DVec3::new(2.0, 0.0, 1.2), pos: DVec3::new(2.5, -1.5, 3.2), text: "Parede de tijolo
10 m²".into() });
                        if let Ok(g) = crate::annot::text_geometry(&crate::annot::Text3dDialog { text: "HomeUp".into(), height: 0.6, depth: 0.15, extrude: true }) {
                            self.doc.defs.push(crate::doc::Definition { name: "Texto 3D".into(), geom: g, group: false, face_me: false, procedure: None, ifc: None, author: None, light: None });
                            let d = self.doc.defs.len() - 1;
                            self.doc.root.instances.push(crate::geom::Instance { def: d, xf: glam::DMat4::from_translation(DVec3::new(0.0, -3.5, 0.0)), material: Some(6), name: String::new(), ..Default::default() });
                        }
                        self.doc.sections.push(crate::doc::SectionPlane { p: DVec3::new(0.0, 1.5, 0.0), n: DVec3::new(0.0, -1.0, 0.0), active: true, ..Default::default() });
                        self.cache_dirty = true;
                        self.zoom_extents();
                    }
                    "more" => self.show_more = true,
                    "home" => self.show_home = true,
                    "libtab" => {
                        let c = ctx.clone();
                        let lib = self.library.get_or_insert_with(|| crate::library::Library::new(&c));
                        lib.tab = match v {
                            "acg" => crate::library::Source::AmbientCg,
                            "phtex" => crate::library::Source::PolyTextures,
                            _ => crate::library::Source::PolyModels,
                        };
                    }
                    "libmodel" => match crate::library::fetch_model(v) {
                        Ok(p) => {
                            self.insert_library_model(v, &p);
                            if let Tool::Place(d) = self.tool {
                                self.doc.root.instances.push(crate::geom::Instance { def: d, xf: glam::DMat4::from_translation(DVec3::new(-2.0, -1.5, 0.0)), ..Default::default() });
                                self.set_tool(Tool::Select);
                                self.cache_dirty = true;
                            }
                        }
                        Err(e) => eprintln!("libmodel: {e}"),
                    },
                    "libmat" => match crate::library::fetch_texture(crate::library::Source::AmbientCg, v) {
                        Ok(b) => {
                            self.insert_library_material(crate::library::Source::AmbientCg, v, b, [0.8, 0.8]);
                            let m = self.doc.materials.len() - 1;
                            for f in &mut self.doc.root.faces {
                                if f.normal.z.abs() < 0.1 && f.outer.len() == 4 {
                                    f.material = Some(m);
                                }
                            }
                            self.set_tool(Tool::Select);
                        }
                        Err(e) => eprintln!("libmat: {e}"),
                    },
                    "textures" => {
                        // procedural checker + plank textures to exercise the texture pipeline
                        let checker: Vec<u8> = (0..64 * 64).flat_map(|i| { let (x, y) = (i % 64, i / 64); if (x / 8 + y / 8) % 2 == 0 { [235u8, 235, 235, 255] } else { [40, 40, 40, 255] } }).collect();
                        let planks: Vec<u8> = (0..128 * 128).flat_map(|i| { let (x, y) = (i % 128, i / 128); let g = ((x as f32 * 0.4).sin() * 18.0) as i32 + if y % 32 == 0 { -60 } else { 0 }; [(150 + g).clamp(0, 255) as u8, (100 + g).clamp(0, 255) as u8, (60 + g / 2).clamp(0, 255) as u8, 255] }).collect();
                        for (name, w, data, size) in [("Xadrez", 64u32, checker, [1.0, 1.0]), ("Tábuas", 128u32, planks, [0.6, 0.6])] {
                            let t = crate::tex::TextureData::from_rgba(name, w, w, data).unwrap();
                            let mut m = crate::doc::Material::color(name, t.average(), 1.0);
                            self.doc.textures.push(t);
                            m.texture = Some(self.doc.textures.len() - 1);
                            m.tex_size = size;
                            self.doc.materials.push(m);
                        }
                        let (ck, pl) = (self.doc.materials.len() - 2, self.doc.materials.len() - 1);
                        for f in &mut self.doc.root.faces {
                            if f.normal.z.abs() < 0.1 && f.outer.len() == 4 {
                                f.material = Some(pl);
                            } else if f.normal.z > 0.9 && f.outer.len() == 4 {
                                f.material = Some(ck);
                            }
                        }
                    }
                    "pbr" => {
                        // normal map with raised tiles on the planks, polished metal on the checker
                        let n = 128u32;
                        let nm: Vec<u8> = (0..n * n)
                            .flat_map(|i| {
                                let (x, y) = ((i % n) as f32, (i / n) as f32);
                                let dx = ((x / n as f32) * std::f32::consts::TAU * 4.0).cos() * 0.5;
                                let dy = ((y / n as f32) * std::f32::consts::TAU * 4.0).cos() * 0.5;
                                let v = glam::Vec3::new(-dx, dy, 1.0).normalize();
                                [((v.x * 0.5 + 0.5) * 255.0) as u8, ((v.y * 0.5 + 0.5) * 255.0) as u8, ((v.z * 0.5 + 0.5) * 255.0) as u8, 255]
                            })
                            .collect();
                        self.doc.textures.push(crate::tex::TextureData::from_rgba("Normal", n, n, nm).unwrap());
                        let t = self.doc.textures.len() - 1;
                        let k = self.doc.materials.len();
                        if k >= 2 {
                            self.doc.materials[k - 1].normal_tex = Some(t);
                            self.doc.materials[k - 1].roughness = 0.6;
                            self.doc.materials[k - 2].metalness = 1.0;
                            self.doc.materials[k - 2].roughness = 0.25;
                        }
                    }
                    "web" => self.prefs.desktop_ui = false,
                    "exportui" => self.show_export = true,
                    "ptui" => {
                        let ctx = eframe::egui::Context::default();
                        self.start_path_trace(&ctx, [640, 360], 16);
                        if let Some(v) = &self.render_view {
                            while !v.job.finished.load(std::sync::atomic::Ordering::Relaxed) {
                                std::thread::sleep(std::time::Duration::from_millis(50));
                            }
                            std::thread::sleep(std::time::Duration::from_millis(600));
                        }
                    }
                    "rexport" => {
                        let (dims, out) = v.split_once(':').unwrap_or(("1920x1080", "export.png"));
                        let n: Vec<u32> = dims.split('x').filter_map(|s| s.parse().ok()).collect();
                        self.pending_raster = Some(crate::export::PendingRaster { path: std::path::PathBuf::from(out), size: [n.first().copied().unwrap_or(1920), n.get(1).copied().unwrap_or(1080)] });
                    }
                    "room" => {
                        // a 4 × 5 m room with a window, wooden floor, lights and an armchair
                        let p = |x: f64, y: f64, z: f64| DVec3::new(x, y, z);
                        let (w, l, h) = (4.0, 5.0, 2.7);
                        self.doc = crate::doc::Document::new();
                        let inside = p(2.0, 2.5, 1.3);
                        let g = &mut self.doc.root;
                        for f in [
                            [p(0., 0., 0.), p(w, 0., 0.), p(w, l, 0.), p(0., l, 0.)],
                            [p(0., 0., h), p(0., l, h), p(w, l, h), p(w, 0., h)],
                            [p(0., 0., 0.), p(0., l, 0.), p(0., l, h), p(0., 0., h)],
                            [p(0., 0., 0.), p(0., 0., h), p(w, 0., h), p(w, 0., 0.)],
                            [p(0., l, 0.), p(w, l, 0.), p(w, l, h), p(0., l, h)],
                            [p(w, 0., 0.), p(w, 0., h), p(w, l, h), p(w, l, 0.)],
                        ] {
                            g.add_polygon(&f, None, false, inside);
                        }
                        // window in the x = w wall
                        g.add_polygon(&[p(w, 1.2, 0.9), p(w, 1.2, 2.2), p(w, 3.6, 2.2), p(w, 3.6, 0.9)], None, false, inside);
                        let win = g.faces.iter().position(|f| f.normal.x.abs() > 0.9 && f.outer.len() == 4 && g.p(f.outer[0]).z > 0.5 && g.p(f.outer[0]).z < 2.5 && (g.p(f.outer[0]).x - w).abs() < 1e-9);
                        if let Some(k) = win {
                            g.faces.remove(k);
                        }
                        // materials: wood floor (procedural planks), warm white walls
                        let planks: Vec<u8> = (0..256 * 256).flat_map(|i| {
                            let (x, y) = (i % 256, i / 256);
                            let g = ((x as f32 * 0.3).sin() * 10.0 + (y as f32 * 0.05).sin() * 6.0) as i32 + if y % 64 == 0 || (x + (y / 64) * 97) % 256 == 0 { -50 } else { 0 };
                            [(150 + g).clamp(0, 255) as u8, (104 + g).clamp(0, 255) as u8, (66 + g / 2).clamp(0, 255) as u8, 255]
                        }).collect();
                        let t = crate::tex::TextureData::from_rgba("Tábuas", 256, 256, planks).unwrap();
                        let mut floor = crate::doc::Material::color("Piso de madeira", t.average(), 1.0);
                        self.doc.textures.push(t);
                        floor.texture = Some(self.doc.textures.len() - 1);
                        floor.tex_size = [1.2, 1.2];
                        floor.roughness = 0.45;
                        self.doc.materials.push(floor);
                        let fm = self.doc.materials.len() - 1;
                        self.doc.materials.push(crate::doc::Material::color("Parede", [236, 230, 220], 1.0));
                        let wm = self.doc.materials.len() - 1;
                        for f in &mut self.doc.root.faces {
                            let m = Some(if f.normal.z.abs() > 0.9 && self.doc.root.verts[f.outer[0] as usize].z < 0.1 { fm } else { wm });
                            f.material = m;
                            f.back_material = m;
                        }
                        // lights: two spots and a pendant
                        for (k, at) in [(crate::lights::LightKind::Spot, p(1.0, 1.5, h)), (crate::lights::LightKind::Spot, p(1.0, 3.5, h)), (crate::lights::LightKind::Point, p(2.2, 2.5, h))] {
                            self.new_light(k);
                            if let Tool::Place(d) = self.tool {
                                let mut xf = glam::DMat4::from_translation(at);
                                if k == crate::lights::LightKind::Point {
                                    xf *= glam::DMat4::from_translation(p(0.0, 0.0, -0.6));
                                }
                                self.doc.root.instances.push(crate::geom::Instance { def: d, xf, ..Default::default() });
                            }
                        }
                        self.set_tool(Tool::Select);
                        if let Ok(path) = crate::library::fetch_model("ArmChair_01") {
                            self.insert_library_model("ArmChair_01", &path);
                            if let Tool::Place(d) = self.tool {
                                let xf = glam::DMat4::from_translation(p(2.6, 2.2, 0.0)) * glam::DMat4::from_rotation_z(-2.2);
                                self.doc.root.instances.push(crate::geom::Instance { def: d, xf, ..Default::default() });
                            }
                            self.set_tool(Tool::Select);
                        }
                        self.doc.shadows.on = true;
                        self.doc.shadows.minutes = 15 * 60;
                        self.cam.target = p(3.0, 2.6, 1.0);
                        self.cam.yaw = (200f64).to_radians();
                        self.cam.pitch = 8f64.to_radians();
                        self.cam.dist = 2.6;
                        self.cam.fov_y = 60f64.to_radians();
                        self.cache_dirty = true;
                        self.dirty = true;
                    }
                    "ptrace" => {
                        // ptrace=WIDTHxHEIGHTxSPP:path.png
                        let (dims, out) = v.split_once(':').unwrap_or(("640x360x64", "pathtrace.png"));
                        let n: Vec<u32> = dims.split('x').filter_map(|s| s.parse().ok()).collect();
                        let (w, h, s) = (n.first().copied().unwrap_or(640), n.get(1).copied().unwrap_or(360), n.get(2).copied().unwrap_or(64));
                        if let Err(e) = crate::pathtrace::render_to_file(&self.doc, &self.cam, [w, h], s, std::path::Path::new(out), if std::env::var_os("HOMEUP_PT_CPU").is_some() { None } else { self.gpu_ctx.clone() }) {
                            eprintln!("ptrace: {e}");
                        }
                    }
                    "zoomext" => {
                        self.cache_dirty = true;
                        self.zoom_extents();
                    }
                    "openskp" => {
                        self.open_skp(std::path::PathBuf::from(v));
                        if std::env::var("HOMEUP_TRACE").is_ok() {
                            eprintln!("{}", self.message);
                            for i in &self.doc.root.instances {
                                let d = &self.doc.defs[i.def];
                                eprintln!("  inst {:?} def={} faces={} edges={} insts={} hidden={} t={:?}", i.name, d.name, d.geom.faces.len(), d.geom.edges.len(), d.geom.instances.len(), i.hidden, i.xf.w_axis);
                            }
                        }
                    }
                    "texdistort" => {
                        if let Some(t) = self.texpos.as_mut() {
                            t.yellow = t.red + (t.yellow - t.red) * 0.55;
                        }
                        self.texpos_apply_pub();
                    }
                    "pat" => {
                        let p = crate::hatch::parse_pat(";%UNITS=MM\n*TIJOLO\n0, 0,0, 0,75\n90, 0,0, 75,75, 75,-75\n45, 0,0, 0,40, 20,-10, 0,-10\n").unwrap();
                        self.doc.hatch_pats.push(p);
                        self.doc.style.section_hatch = crate::hatch::Hatch { custom: Some(self.doc.hatch_pats.len() - 1), scale: 0.004, fill: [240, 235, 220], line: [30, 30, 30], ..Default::default() };
                        self.dirty = true;
                    }
                    "tin" => {
                        self.create_scratch_tin(DVec3::new(-14.0, -6.0, 0.0), 10.0, 9.0);
                        let d = self.doc.defs.len() - 1;
                        let g = &mut self.doc.defs[d].geom;
                        for p in &mut g.verts {
                            p.z += 2.5 * crate::sandbox::falloff(p.truncate().distance(glam::DVec2::new(5.0, 4.5)), 4.0);
                        }
                        crate::sandbox::recompute_normals(g);
                        self.sel.clear();
                        self.cache_dirty = true;
                        self.zoom_extents();
                    }
                    "copyalong" => {
                        let mut path = crate::geom::Geom::default();
                        path.add_segments(&[(DVec3::new(-2.0, -4.0, 0.0), DVec3::new(6.0, -4.0, 0.0)), (DVec3::new(6.0, -4.0, 0.0), DVec3::new(6.0, 3.0, 0.0))], None, false, DVec3::new(0.0, 0.0, 20.0));
                        let src = self.doc.root.instances[0].def;
                        let ca = crate::procedural::CopyAlong { source: src, base: glam::DMat4::IDENTITY, source_len: 0.5, mode: crate::procedural::AlongMode::EntirePath, dist: crate::procedural::Distribution::ByCount(8), orient: crate::procedural::Orientation::Stair, margin: 0.0, margin_relative: false };
                        self.doc.defs.push(crate::doc::Definition { name: "Copiar ao longo".into(), geom: path, group: false, face_me: false, procedure: Some(ca), ifc: None, author: None, light: None });
                        let d = self.doc.defs.len() - 1;
                        self.regen_procedure(d);
                        self.doc.root.instances.push(crate::geom::Instance { def: d, ..Default::default() });
                        self.sel = crate::geom::Sel { insts: [self.doc.root.instances.len() - 1].into(), ..Default::default() };
                        self.cache_dirty = true;
                    }
                    "texpos" => {
                        let f = self.doc.root.faces.iter().position(|f| f.normal.z.abs() < 0.1 && f.material.is_some_and(|m| self.doc.materials[m].texture.is_some()));
                        if let Some(f) = f {
                            self.sel = crate::geom::Sel { faces: [f].into(), ..Default::default() };
                            self.start_texture_position();
                            if let Some(t) = self.texpos.as_mut() {
                                let c = t.red;
                                t.green = c + (t.green - c) * 2.0;
                                t.blue = c + glam::DVec2::new(-(t.green - c).y, (t.green - c).x) * 0.5;
                            }
                            self.texpos_apply_pub();
                        }
                    }
                    "hatch" => {
                        let p = crate::hatch::HatchPattern::ALL[v.parse::<usize>().unwrap_or(1).min(6)];
                        self.doc.style.section_hatch = crate::hatch::Hatch { pattern: p, scale: 0.4, angle_deg: 0.0, line: [20, 20, 20], fill: [235, 225, 200], ..Default::default() };
                        self.dirty = true;
                    }
                    "dash" => {
                        self.doc.tags.push(crate::settings::Tag { name: "Demolição".into(), visible: true, color: [200, 60, 60], dash: crate::hatch::Dash::Dashed });
                        let t = self.doc.tags.len() - 1;
                        for i in &mut self.doc.root.instances {
                            i.tag = Some(t);
                        }
                        self.dirty = true;
                    }
                    "wm" => {
                        let img = image::RgbaImage::from_fn(240, 80, |x, y| {
                            let on = (x / 20 + y / 20) % 2 == 0;
                            if on { image::Rgba([20, 60, 160, 255]) } else { image::Rgba([255, 255, 255, 255]) }
                        });
                        let t = crate::tex::TextureData::from_rgba("logo", 240, 80, img.into_raw()).unwrap();
                        self.doc.watermark_images.push(t.clone());
                        self.doc.watermark_images.push(t);
                        let n = self.doc.watermark_images.len();
                        use crate::watermark::{Watermark, WmMode};
                        self.doc.style.watermarks.push(Watermark { name: "fundo".into(), image: n - 2, overlay: false, mask: false, blend: 0.5, mode: WmMode::Tiled, anchor: [0.0, 0.0], scale: 1.0 });
                        self.doc.style.watermarks.push(Watermark { name: "logo".into(), image: n - 1, overlay: true, mask: true, blend: 0.9, mode: WmMode::Positioned, anchor: [1.0, 1.0], scale: 1.0 });
                    }
                    "imgent" => {
                        let img = image::RgbaImage::from_fn(200, 100, |x, y| image::Rgba([(x + 50) as u8, (y * 2) as u8, 160, 255]));
                        let t = crate::tex::TextureData::from_rgba("planta", 200, 100, img.into_raw()).unwrap();
                        let mut m = crate::doc::Material::color("planta", t.average(), 1.0);
                        self.doc.textures.push(t);
                        m.texture = Some(self.doc.textures.len() - 1);
                        self.doc.materials.push(m);
                        self.doc.defs.push(crate::doc::Definition { name: "Imagem: planta".into(), geom: crate::imageimport::image_geom(0.5), group: false, face_me: false, procedure: None, ifc: None, author: None, light: None });
                        let def = self.doc.defs.len() - 1;
                        let xf = glam::DMat4::from_translation(DVec3::new(-4.0, -3.0, 0.0)) * glam::DMat4::from_scale(DVec3::splat(3.0));
                        self.doc.root.instances.push(crate::geom::Instance { def, xf, material: Some(self.doc.materials.len() - 1), ..Default::default() });
                        self.cache_dirty = true;
                    }
                    "mphoto" => {
                        let img = image::RgbaImage::from_fn(600, 400, |x, y| image::Rgba([(150 + x / 10) as u8, (170 + y / 10) as u8, 190, 255]));
                        let mut png = std::io::Cursor::new(Vec::new());
                        img.write_to(&mut png, image::ImageFormat::Png).unwrap();
                        self.start_match_photo("fachada", png.into_inner());
                    }
                    "pole" => {
                        // free-standing edges (a pole and a wire) for Shadows > From edges
                        let segs = [(DVec3::new(-2.0, -1.0, 0.0), DVec3::new(-2.0, -1.0, 3.0)), (DVec3::new(-2.0, -1.0, 3.0), DVec3::new(0.0, -1.0, 3.0))];
                        self.doc.root.add_segments(&segs, None, false, DVec3::new(10.0, -10.0, 10.0));
                        self.dirty = true;
                        self.cache_dirty = true;
                    }
                    "libhdri" => match crate::library::fetch_hdri(v) {
                        Ok(png) => self.insert_environment(v, png, "Poly Haven (CC0)"),
                        Err(e) => eprintln!("libhdri: {e}"),
                    },
                    "matedit" => self.mat_tab_edit = true,
                    "trayonly" => self.tray_open.clear(),
                    "scenes3" => {
                        for (yaw, pitch) in [(-45.0, 35.26), (-90.0, 0.0), (-90.0, 89.9)] {
                            self.set_view(yaw, pitch);
                            self.zoom_extents();
                            self.add_scene();
                        }
                        self.set_view(-45.0, 30.0);
                        self.zoom_extents();
                    }
                    "lockgroup" => {
                        if let Some(i) = self.doc.root.instances.first_mut() {
                            i.locked = true;
                            i.name = "Caixa".into();
                        }
                    }
                    "selface" => {
                        self.sel = Sel { faces: [1usize].into(), ..Default::default() };
                    }
                    "tags" => {
                        for (n, c) in [("Paredes", [230, 126, 34]), ("Mobiliário", [52, 152, 219]), ("Paisagismo", [46, 204, 113])] {
                            self.doc.tags.push(crate::settings::Tag { name: n.into(), visible: true, color: c, ..Default::default() });
                        }
                        for f in &mut self.doc.root.faces {
                            if f.normal.z.abs() < 0.1 {
                                f.tag = Some(0);
                            }
                        }
                        self.display.color_by_tag = true;
                    }
                    "edit-toolbar" => {
                        self.show_more = true;
                        self.edit_toolbar = true;
                    }
                    "flyout" => self.flyout = Some(3),
                    "flyout-arc" => self.flyout = Some(4),
                    "search" => {
                        self.search = v.to_string();
                        ctx.memory_mut(|m| m.request_focus(egui::Id::unique(crate::chrome::SEARCH_ID)));
                    }
                    "panel" => {
                        self.open_panel = match v {
                            "components" => Some(PanelKind::Components),
                            "materials" => Some(PanelKind::Materials),
                            "scenes" => Some(PanelKind::Scenes),
                            "instructor" => Some(PanelKind::Instructor),
                            "styles" => Some(PanelKind::Styles),
                            "tags" => Some(PanelKind::Tags),
                            "shadows" => Some(PanelKind::Shadows),
                            "display" => Some(PanelKind::Display),
                            "soften" => Some(PanelKind::Soften),
                            "model" => Some(PanelKind::ModelInfo),
                            "library" => Some(PanelKind::Library),
                            "outliner" => Some(PanelKind::Outliner),
                            "environments" => Some(PanelKind::Environments),
                            "none" => None,
                            _ => Some(PanelKind::Entity),
                        }
                    }
                    "select" => {
                        let n = self.doc.root.instances.len();
                        self.sel = Sel { insts: (0..n).collect(), ..Default::default() };
                    }
                    "ortho" => self.cam.ortho = true,
                    "shadows" => {
                        self.doc.shadows.on = true;
                        self.doc.shadows.minutes = v.parse::<u32>().unwrap_or(15 * 60);
                    }
                    "style" => {
                        if let Some(s) = crate::settings::Style::presets().get(v.parse::<usize>().unwrap_or(0)) {
                            self.doc.style = s.clone();
                        }
                    }
                    "north" => self.display.true_north = true,
                    "tool" => {
                        if v == "select" {
                            self.set_tool(crate::app::Tool::Select);
                        }
                    }
                    "yaw" => {
                        self.cam.yaw = v.parse::<f64>().unwrap_or(0.0).to_radians();
                        self.zoom_extents();
                    }
                    "pitch" => {
                        self.cam.pitch = v.parse::<f64>().unwrap_or(20.0).to_radians();
                        self.zoom_extents();
                    }
                    "nosel" => self.sel.clear(),
                    "nosection" => {
                        for s in &mut self.doc.sections {
                            s.active = false;
                        }
                    }
                    "top" => {
                        self.cam.yaw = (-90f64).to_radians();
                        self.cam.pitch = 89f64.to_radians();
                        self.zoom_extents();
                    }
                    _ => {}
                }
            }
            self.dirty = true;
        }
        if self.snapshot.as_ref().is_some_and(|s| s.setup.iter().any(|f| f == "hover")) && frame > 3 && frame < 200 {
            // synthetic pointer inside the app (no OS input): sweep the viewport with every tool
            let tools = [
                crate::app::Tool::Line,
                crate::app::Tool::Select,
                crate::app::Tool::Rect,
                crate::app::Tool::PushPull,
                crate::app::Tool::Move,
                crate::app::Tool::Eraser,
                crate::app::Tool::Paint,
                crate::app::Tool::Tape,
                crate::app::Tool::Circle,
                crate::app::Tool::Dimension,
                crate::app::Tool::Section,
            ];
            let t = tools[(frame as usize / 18) % tools.len()];
            if std::env::var("HOMEUP_TRACE").is_ok() {
                eprintln!("frame {frame} tool {:?}", t);
            }
            if self.tool != t {
                self.set_tool(t);
            }
            let w = self.cam.size;
            let f = frame as f64;
            let px = glam::DVec2::new(w.x * (0.5 + 0.45 * (f * 0.13).sin()), w.y * (0.5 + 0.45 * (f * 0.071).cos()));
            let pos = self.viewport_min + egui::vec2(px.x as f32, px.y as f32);
            let i = crate::app::In { px, pos, pressed: false, released: false, dbl: false, triple: false, dragging_far: false, mods: Default::default(), delta: glam::DVec2::ZERO, primary_down: false };
            self.overlay = Default::default();
            self.tool_frame(&i);
            self.dirty = true;
        }
        if self.snapshot.as_ref().is_some_and(|s| s.setup.iter().any(|f| f == "orbit")) && frame < 200 {
            self.cam.orbit(glam::DVec2::new(9.0, if frame % 40 < 20 { 2.5 } else { -2.5 }));
            self.cam.zoom_at(self.cam.target, if frame % 60 < 30 { 0.98 } else { 1.02 });
        }
        let s = self.snapshot.as_mut().unwrap();
        if frame >= 200 && !s.requested {
            s.requested = true;
            let path = s.path.clone();
            let done = s.done.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
                if let Err(e) = save_png(&path, &img) {
                    eprintln!("snapshot: {e}");
                }
                done.store(true, Ordering::SeqCst);
            })));
        }
        if s.done.load(Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}
