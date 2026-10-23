//! Tray panels in the SketchUp 2026 desktop style: Materials (preview, Select /
//! Edit tabs, photoreal maps), Components and Scenes with thumbnails,
//! Environments and the Outliner.

use crate::app::{HomeUpApp, Tool};
use crate::doc::Material;
use crate::settings::Environment;
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Vec2};
use glam::DVec2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MapSlot {
    Color,
    Normal,
    Rough,
    Metal,
    Ao,
}

impl MapSlot {
    fn get(self, m: &Material) -> Option<usize> {
        match self {
            MapSlot::Color => m.texture,
            MapSlot::Normal => m.normal_tex,
            MapSlot::Rough => m.rough_tex,
            MapSlot::Metal => m.metal_tex,
            MapSlot::Ao => m.ao_tex,
        }
    }
    fn set(self, m: &mut Material, v: Option<usize>) {
        match self {
            MapSlot::Color => m.texture = v,
            MapSlot::Normal => m.normal_tex = v,
            MapSlot::Rough => m.rough_tex = v,
            MapSlot::Metal => m.metal_tex = v,
            MapSlot::Ao => m.ao_tex = v,
        }
    }
    fn label(self) -> &'static str {
        match self {
            MapSlot::Color => "textura",
            MapSlot::Normal => "normal",
            MapSlot::Rough => "rugosidade",
            MapSlot::Metal => "metal",
            MapSlot::Ao => "AO",
        }
    }
}

fn hash_of<T: std::hash::Hash>(t: &T) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

fn pick_image(title: &str, hdr: bool) -> Option<std::path::PathBuf> {
    let mut d = rfd::FileDialog::new().set_title(title);
    d = if hdr { d.add_filter("Panorama", &["hdr", "exr", "jpg", "jpeg", "png"]) } else { d.add_filter("Imagem", &["jpg", "jpeg", "png"]) };
    d.pick_file()
}

impl HomeUpApp {
    /// Cached UI thumbnail; `make` runs only when the signature changed
    /// (at most a few per frame so big models stay responsive).
    pub(crate) fn ui_thumb(&mut self, ctx: &egui::Context, key: &str, sig: u64, make: impl FnOnce(&Self) -> Option<image::RgbaImage>) -> Option<egui::TextureId> {
        if let Some((s, h)) = self.ui_thumbs.get(key) {
            if *s == sig || self.thumb_budget == 0 {
                return Some(h.id());
            }
        }
        if self.thumb_budget == 0 {
            ctx.request_repaint();
            return None;
        }
        self.thumb_budget -= 1;
        let img = make(self)?;
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
        let h = ctx.load_texture(format!("thumb-{key}"), ci, egui::TextureOptions::LINEAR);
        let id = h.id();
        self.ui_thumbs.insert(key.to_string(), (sig, h));
        Some(id)
    }

    fn material_preview(&mut self, ctx: &egui::Context, k: usize, size: u32) -> Option<egui::TextureId> {
        let m = self.doc.materials.get(k)?.clone();
        let tex_len = m.texture.and_then(|t| self.doc.textures.get(t)).map(|t| t.bytes.len()).unwrap_or(0);
        let sig = hash_of(&(format!("{m:?}"), tex_len, size));
        self.ui_thumb(ctx, &format!("mat-{k}-{size}"), sig, |app| {
            let tex = m.texture.and_then(|t| app.doc.textures.get(t)).and_then(|t| t.decode()).map(|i| image::imageops::thumbnail(&i, 128, 128));
            Some(crate::thumbs::material_sphere(&m, tex.as_ref(), size))
        })
    }

    // ------------------------------------------------------------ materials

    pub(crate) fn materials_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        // header: preview sphere, name and quick actions
        let cur = self.edit_mat.or(self.cur_mat).filter(|k| *k < self.doc.materials.len());
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(72.0), Sense::hover());
            ui.painter().rect_filled(rect, 4.0, Color32::from_gray(236));
            match cur.and_then(|k| self.material_preview(&ctx, k, 96)) {
                Some(id) => {
                    ui.painter().image(id, rect.shrink(2.0), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                }
                None => {
                    ui.painter().circle_stroke(rect.center(), 28.0, Stroke::new(1.0, Color32::GRAY));
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "Padrão", egui::FontId::proportional(11.0), Color32::DARK_GRAY);
                }
            }
            ui.vertical(|ui| {
                match cur {
                    Some(k) => {
                        if ui.add(egui::TextEdit::singleline(&mut self.doc.materials[k].name).desired_width(150.0)).changed() {
                            self.modified = true;
                        }
                    }
                    None => {
                        ui.label(RichText::new("Padrão").strong());
                    }
                }
                ui.horizontal(|ui| {
                    if ui.small_button("Criar").on_hover_text("Criar material").clicked() {
                        let n = self.doc.materials.len() + 1;
                        let base = cur.map(|k| self.doc.materials[k].clone());
                        let mut m = base.unwrap_or_else(|| Material::color("", [200, 180, 140], 1.0));
                        m.name = format!("Material {n}");
                        self.doc.materials.push(m);
                        self.cur_mat = Some(self.doc.materials.len() - 1);
                        self.edit_mat = self.cur_mat;
                        self.mat_tab_edit = true;
                        self.modified = true;
                    }
                    if ui.small_button("Amostrar").on_hover_text("Conta-gotas: pegar o material de uma face (Alt no Pintar)").clicked() {
                        self.set_tool(Tool::Sample);
                    }
                    if ui.small_button("Padrão").on_hover_text("Pintar com o material padrão").clicked() {
                        self.cur_mat = None;
                        self.edit_mat = None;
                        self.set_tool(Tool::Paint);
                    }
                });
                if let Some(k) = cur {
                    let src = self.doc.materials[k].source.clone();
                    if !src.is_empty() {
                        ui.weak(RichText::new(format!("Origem: {src}")).small());
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            if ui.selectable_label(!self.mat_tab_edit, "Selecionar").clicked() {
                self.mat_tab_edit = false;
            }
            if ui.add_enabled(cur.is_some(), egui::Button::selectable(self.mat_tab_edit, "Editar")).clicked() {
                self.mat_tab_edit = true;
            }
        });
        ui.separator();
        if self.mat_tab_edit && cur.is_some() {
            self.material_editor(ui, cur.unwrap());
        } else {
            self.material_picker(ui);
        }
    }

    fn material_picker(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            ui.label("No modelo");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Biblioteca online…").on_hover_text("Materiais prontos (ambientCG / Poly Haven, CC0)").clicked() {
                    self.open_panel = Some(crate::chrome::PanelKind::Library);
                    self.tray_open.insert(crate::chrome::PanelKind::Library);
                    let lib = self.library.get_or_insert_with(|| crate::library::Library::new(&ctx));
                    lib.tab = crate::library::Source::AmbientCg;
                }
            });
        });
        let w = ui.available_width();
        let cell = 44.0;
        let cols = ((w / (cell + 4.0)).floor() as usize).max(1);
        let n = self.doc.materials.len();
        let mut clicked = None;
        let mut double = None;
        let mut del = None;
        let mut area = None;
        for row in 0..n.div_ceil(cols).max(1) {
            ui.horizontal(|ui| {
                for k in (row * cols)..((row + 1) * cols).min(n) {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(cell), Sense::click());
                    match self.material_preview(&ctx, k, 64) {
                        Some(id) => {
                            ui.painter().image(id, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                        }
                        None => {
                            let c = self.doc.materials[k].color;
                            ui.painter().circle_filled(rect.center(), cell * 0.45, Color32::from_rgb(c[0], c[1], c[2]));
                        }
                    }
                    if self.cur_mat == Some(k) {
                        ui.painter().rect_stroke(rect, 4.0, Stroke::new(2.0, Color32::from_rgb(30, 110, 220)), StrokeKind::Inside);
                    }
                    let name = self.doc.materials[k].name.clone();
                    let resp = resp.on_hover_text(name);
                    if resp.clicked() {
                        clicked = Some(k);
                    }
                    if resp.double_clicked() {
                        double = Some(k);
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Editar").clicked() {
                            double = Some(k);
                            ui.close();
                        }
                        if ui.button("Área").clicked() {
                            area = Some(k);
                            ui.close();
                        }
                        if ui.button("Apagar").clicked() {
                            del = Some(k);
                            ui.close();
                        }
                    });
                }
            });
        }
        if n == 0 {
            ui.weak("Nenhum material no modelo. Use Criar ou a biblioteca online.");
        }
        if let Some(k) = clicked {
            self.cur_mat = Some(k);
            self.edit_mat = Some(k);
            if self.tool != Tool::Paint {
                self.set_tool(Tool::Paint);
            }
        }
        if let Some(k) = double {
            self.cur_mat = Some(k);
            self.edit_mat = Some(k);
            self.mat_tab_edit = true;
        }
        if let Some(k) = del {
            self.checkpoint_keep_sel();
            self.doc.remove_material(k);
            self.edit_mat = None;
            self.cur_mat = None;
        }
        if let Some(k) = area {
            let a = self.material_area(k);
            self.message = if a > 0.0 {
                format!("Área com {}: {}", self.doc.materials[k].name, crate::units::format_area(a))
            } else {
                format!("{} não está aplicado em nenhuma face.", self.doc.materials[k].name)
            };
        }
    }

    fn map_slot(&mut self, ui: &mut egui::Ui, k: usize, slot: MapSlot) -> bool {
        let ctx = ui.ctx().clone();
        let mut changed = false;
        let cur = slot.get(&self.doc.materials[k]).filter(|t| *t < self.doc.textures.len());
        ui.horizontal(|ui| {
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
            match cur.and_then(|t| self.tex_thumb(&ctx, t)) {
                Some(id) => {
                    ui.painter().image(id, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                }
                None => {
                    ui.painter().rect_stroke(rect, 2.0, Stroke::new(1.0, Color32::GRAY), StrokeKind::Inside);
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "+", egui::FontId::proportional(16.0), Color32::GRAY);
                }
            }
            let load = resp.on_hover_text(format!("Carregar mapa de {}", slot.label())).clicked() | ui.small_button(if cur.is_some() { "Trocar…" } else { "Carregar…" }).clicked();
            if load {
                if let Some(path) = pick_image(&format!("Mapa de {}", slot.label()), false) {
                    match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| {
                        crate::tex::TextureData::from_bytes(&path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), b)
                    }) {
                        Ok(t) => {
                            self.checkpoint_keep_sel();
                            let avg = t.average();
                            self.doc.textures.push(t);
                            let m = &mut self.doc.materials[k];
                            slot.set(m, Some(self.doc.textures.len() - 1));
                            if slot == MapSlot::Color {
                                m.color = avg;
                                if m.source.is_empty() {
                                    m.source = "imagem própria".into();
                                }
                            }
                            changed = true;
                        }
                        Err(e) => self.message = format!("Imagem inválida: {e}"),
                    }
                }
            }
            if cur.is_some() && ui.small_button("×").on_hover_text("Remover mapa").clicked() {
                self.checkpoint_keep_sel();
                slot.set(&mut self.doc.materials[k], None);
                changed = true;
            }
        });
        changed
    }

    fn material_editor(&mut self, ui: &mut egui::Ui, k: usize) {
        let mut changed = false;
        let section = |ui: &mut egui::Ui, t: &str| {
            ui.add_space(4.0);
            ui.label(RichText::new(t).strong().color(Color32::from_rgb(40, 60, 90)));
        };
        section(ui, "Cor");
        {
            let m = &mut self.doc.materials[k];
            ui.horizontal(|ui| {
                ui.label(if m.texture.is_some() { "Tom médio" } else { "Cor" });
                changed |= ui.color_edit_button_srgb(&mut m.color).changed();
                let [r, g, b] = m.color;
                ui.weak(format!("RGB {r}, {g}, {b}"));
            });
        }
        ui.horizontal(|ui| {
            ui.label("Textura");
            changed |= self.map_slot(ui, k, MapSlot::Color);
        });
        if self.doc.materials[k].texture.is_some() {
            let m = &mut self.doc.materials[k];
            ui.horizontal(|ui| {
                ui.label("Tamanho");
                changed |= ui.add(egui::DragValue::new(&mut m.tex_size[0]).speed(0.01).range(0.01..=100.0).suffix(" m")).changed();
                ui.label("×");
                changed |= ui.add(egui::DragValue::new(&mut m.tex_size[1]).speed(0.01).range(0.01..=100.0).suffix(" m")).changed();
            });
        }
        section(ui, "Opacidade");
        changed |= ui.add(egui::Slider::new(&mut self.doc.materials[k].alpha, 0.0..=1.0).fixed_decimals(2)).changed();
        section(ui, "Metal");
        {
            let m = &mut self.doc.materials[k];
            ui.horizontal(|ui| {
                ui.label("Metalicidade");
                changed |= ui.add(egui::Slider::new(&mut m.metalness, 0.0..=1.0).fixed_decimals(2)).changed();
            });
        }
        ui.horizontal(|ui| {
            ui.label("Mapa");
            changed |= self.map_slot(ui, k, MapSlot::Metal);
        });
        section(ui, "Rugosidade");
        {
            let m = &mut self.doc.materials[k];
            ui.horizontal(|ui| {
                ui.label("Rugosidade");
                changed |= ui.add(egui::Slider::new(&mut m.roughness, 0.0..=1.0).fixed_decimals(2)).changed();
            });
        }
        ui.horizontal(|ui| {
            ui.label("Mapa");
            changed |= self.map_slot(ui, k, MapSlot::Rough);
        });
        if self.doc.materials[k].rough_tex.is_some() {
            changed |= ui.checkbox(&mut self.doc.materials[k].invert_rough, "Inverter (mapa de brilho)").changed();
        }
        section(ui, "Normal");
        ui.horizontal(|ui| {
            ui.label("Mapa");
            changed |= self.map_slot(ui, k, MapSlot::Normal);
        });
        if self.doc.materials[k].normal_tex.is_some() {
            let m = &mut self.doc.materials[k];
            ui.horizontal(|ui| {
                ui.label("Intensidade");
                changed |= ui.add(egui::Slider::new(&mut m.normal_scale, 0.0..=3.0).fixed_decimals(2)).changed();
            });
        }
        section(ui, "Oclusão de ambiente");
        ui.horizontal(|ui| {
            ui.label("Mapa");
            changed |= self.map_slot(ui, k, MapSlot::Ao);
        });
        if self.doc.materials[k].ao_tex.is_some() {
            let m = &mut self.doc.materials[k];
            ui.horizontal(|ui| {
                ui.label("Intensidade");
                changed |= ui.add(egui::Slider::new(&mut m.ao_strength, 0.0..=1.0).fixed_decimals(2)).changed();
            });
        }
        if self.doc.style.face != crate::settings::FaceStyle::Photoreal {
            ui.add_space(6.0);
            ui.weak("Metal, rugosidade, normal e AO aparecem no estilo \"Materiais fotorrealistas\".");
        }
        if changed {
            self.modified = true;
            self.dirty = true;
        }
    }

    // ------------------------------------------------------------ components

    pub(crate) fn components_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let comps: Vec<usize> = (0..self.doc.defs.len()).filter(|d| !self.doc.defs[*d].group).collect();
        ui.horizontal(|ui| {
            ui.label("No modelo");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Biblioteca online…").on_hover_text("Móveis prontos (Poly Haven, CC0)").clicked() {
                    self.open_panel = Some(crate::chrome::PanelKind::Library);
                    self.tray_open.insert(crate::chrome::PanelKind::Library);
                    let lib = self.library.get_or_insert_with(|| crate::library::Library::new(&ctx));
                    lib.tab = crate::library::Source::PolyModels;
                }
            });
        });
        if comps.is_empty() {
            ui.weak("Nenhum componente. Selecione geometria e aperte G.");
        }
        let w = ui.available_width();
        let cell = 72.0;
        let cols = ((w / (cell + 6.0)).floor() as usize).max(1);
        let rev = self.rev;
        for row in comps.chunks(cols) {
            ui.horizontal(|ui| {
                for &d in row {
                    ui.vertical(|ui| {
                        ui.set_width(cell);
                        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(cell), Sense::click());
                        ui.painter().rect_filled(rect, 4.0, Color32::from_gray(244));
                        let sig = hash_of(&(rev, d));
                        if let Some(id) = self.ui_thumb(&ctx, &format!("def-{d}"), sig, |app| crate::thumbs::definition(&app.doc, d, 128)) {
                            ui.painter().image(id, rect.shrink(3.0), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                        }
                        let n = self.doc.instance_count(d);
                        let name = self.doc.defs[d].name.clone();
                        if resp.on_hover_text(format!("{name}\n{n} no modelo — clique para inserir")).clicked() {
                            self.set_tool(Tool::Place(d));
                        }
                        let short: String = name.chars().take(12).collect();
                        ui.label(RichText::new(short).small());
                    });
                }
            });
        }
        if !self.doc.defs.is_empty() && ui.button("Limpar não usados").clicked() {
            self.checkpoint();
            let n = self.doc.purge_unused();
            self.message = format!("{n} definições removidas.");
        }
    }

    // ------------------------------------------------------------ scenes

    pub(crate) fn scene_thumb(&mut self, ctx: &egui::Context, k: usize) -> Option<egui::TextureId> {
        let s = self.doc.scenes.get(k)?.clone();
        let sig = hash_of(&(self.rev, s.cam.map(|v| v.to_bits()), s.ortho));
        self.ui_thumb(ctx, &format!("scene-{k}"), sig, |app| {
            let mut cam = app.cam.clone();
            cam.set_array(s.cam);
            cam.ortho = s.ortho;
            cam.size = DVec2::new(160.0, 90.0);
            Some(crate::thumbs::model_view(&app.doc, cam.view_proj(), 160, 90))
        })
    }

    pub(crate) fn scenes_ui(&mut self, ui: &mut egui::Ui, now: f64) {
        let ctx = ui.ctx().clone();
        let mut go = None;
        let mut del = None;
        let mut up = None;
        ui.horizontal(|ui| {
            if ui.button("+ Adicionar cena").clicked() {
                self.add_scene();
            }
            if ui.add_enabled(!self.doc.scenes.is_empty(), egui::Button::new("Atualizar")).on_hover_text("Salvar a vista atual na cena selecionada").clicked() {
                up = Some(self.scene_sel.min(self.doc.scenes.len().saturating_sub(1)));
            }
        });
        for k in 0..self.doc.scenes.len() {
            ui.horizontal(|ui| {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(96.0, 54.0), Sense::click());
                ui.painter().rect_filled(rect, 3.0, Color32::from_gray(230));
                if let Some(id) = self.scene_thumb(&ctx, k) {
                    ui.painter().image(id, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                }
                if self.scene_sel == k {
                    ui.painter().rect_stroke(rect, 3.0, Stroke::new(2.0, Color32::from_rgb(30, 110, 220)), StrokeKind::Inside);
                }
                if resp.clicked() {
                    self.scene_sel = k;
                    go = Some(k);
                }
                ui.vertical(|ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut self.doc.scenes[k].name).desired_width(120.0));
                    if r.changed() {
                        self.modified = true;
                    }
                    ui.horizontal(|ui| {
                        if ui.small_button("Ir").clicked() {
                            self.scene_sel = k;
                            go = Some(k);
                        }
                        if ui.small_button("Apagar").clicked() {
                            del = Some(k);
                        }
                    });
                });
            });
        }
        if let Some(k) = up {
            self.checkpoint_keep_sel();
            self.doc.scenes[k].cam = self.cam.to_array();
            self.doc.scenes[k].ortho = self.cam.ortho;
        }
        if let Some(k) = go {
            self.go_scene(k, now);
        }
        if let Some(k) = del {
            self.checkpoint_keep_sel();
            self.doc.scenes.remove(k);
        }
    }

    // ------------------------------------------------------------ environments

    /// Add an equirectangular panorama (PNG/JPG bytes) as the active environment.
    pub(crate) fn insert_environment(&mut self, name: &str, bytes: Vec<u8>, source: &str) {
        match crate::tex::TextureData::from_bytes(name, bytes) {
            Ok(t) => {
                self.checkpoint_keep_sel();
                self.doc.env_images.push(t);
                let env = Environment { name: name.to_string(), image: Some(self.doc.env_images.len() - 1), ..Default::default() };
                self.doc.envs.push(env.clone());
                self.doc.env = env;
                if self.doc.style.face != crate::settings::FaceStyle::Photoreal {
                    self.doc.style.face = crate::settings::FaceStyle::Photoreal;
                }
                self.doc.style.sky = true;
                self.message = format!("Ambiente {name} ({source}) aplicado.");
            }
            Err(e) => self.message = format!("Falha no ambiente {name}: {e}"),
        }
    }

    pub(crate) fn environments_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            if ui.button("+ Adicionar…").on_hover_text("Panorama equirretangular (.hdr, .exr, .jpg, .png)").clicked() {
                if let Some(path) = pick_image("Ambiente (panorama 360°)", true) {
                    let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    let is_hdr = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("hdr") || e.eq_ignore_ascii_case("exr"));
                    match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| if is_hdr { crate::library::tonemap_hdr(&b) } else { Ok(b) }) {
                        Ok(b) => self.insert_environment(&name, b, "arquivo"),
                        Err(e) => self.message = format!("Falha ao ler o panorama: {e}"),
                    }
                }
            }
            if ui.button("Biblioteca online…").on_hover_text("HDRIs CC0 do Poly Haven").clicked() {
                self.open_panel = Some(crate::chrome::PanelKind::Library);
                self.tray_open.insert(crate::chrome::PanelKind::Library);
                let lib = self.library.get_or_insert_with(|| crate::library::Library::new(&ctx));
                lib.tab = crate::library::Source::PolyHdris;
            }
        });
        ui.label(RichText::new("No modelo").small().color(Color32::from_gray(110)));
        let mut pick = None;
        let mut del = None;
        let default_active = self.doc.env.image.is_none();
        if ui.selectable_label(default_active, "Céu padrão (gradiente)").clicked() {
            pick = Some(None);
        }
        for k in 0..self.doc.envs.len() {
            let e = self.doc.envs[k].clone();
            ui.horizontal(|ui| {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(96.0, 48.0), Sense::click());
                ui.painter().rect_filled(rect, 3.0, Color32::from_gray(220));
                if let Some(i) = e.image {
                    let len = self.doc.env_images.get(i).map(|t| t.bytes.len()).unwrap_or(0);
                    if let Some(id) = self.ui_thumb(&ctx, &format!("env-{i}"), len as u64, |app| {
                        let img = app.doc.env_images.get(i)?.decode()?;
                        Some(image::imageops::thumbnail(&img, 192, 96))
                    }) {
                        ui.painter().image(id, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                    }
                }
                let active = self.doc.env.image == e.image && e.image.is_some();
                if active {
                    ui.painter().rect_stroke(rect, 3.0, Stroke::new(2.0, Color32::from_rgb(30, 110, 220)), StrokeKind::Inside);
                }
                if resp.clicked() {
                    pick = Some(Some(k));
                }
                ui.vertical(|ui| {
                    ui.label(&e.name);
                    if ui.small_button("Remover").clicked() {
                        del = Some(k);
                    }
                });
            });
        }
        match pick {
            Some(None) => self.doc.env = Environment { image: None, ..self.doc.env.clone() },
            Some(Some(k)) => self.doc.env = self.doc.envs[k].clone(),
            None => {}
        }
        if let Some(k) = del {
            self.checkpoint_keep_sel();
            let e = self.doc.envs.remove(k);
            if self.doc.env.image == e.image {
                self.doc.env.image = None;
            }
        }
        ui.separator();
        ui.label(RichText::new("Configurações").strong());
        let mut ch = false;
        let env = &mut self.doc.env;
        ui.add_enabled_ui(env.image.is_some(), |ui| {
            ui.horizontal(|ui| {
                ui.label("Rotação");
                ch |= ui.add(egui::Slider::new(&mut env.rotation_deg, 0.0..=360.0).suffix("°").fixed_decimals(0)).changed();
            });
            ch |= ui.checkbox(&mut env.skydome, "Usar como cúpula do céu").changed();
            ui.horizontal(|ui| {
                ui.label("Exposição do céu");
                ch |= ui.add(egui::Slider::new(&mut env.sky_exposure, 0.1..=3.0).fixed_decimals(2)).changed();
            });
        });
        ch |= ui.checkbox(&mut env.reflections, "Usar para reflexos").changed();
        ui.horizontal(|ui| {
            ui.label("Exposição dos reflexos");
            ch |= ui.add(egui::Slider::new(&mut env.refl_exposure, 0.0..=3.0).fixed_decimals(2)).changed();
        });
        if ch {
            let e = self.doc.env.clone();
            if let Some(slot) = self.doc.envs.iter_mut().find(|x| x.image == e.image && e.image.is_some()) {
                *slot = e;
            }
            self.modified = true;
        }
        if self.doc.style.face != crate::settings::FaceStyle::Photoreal {
            ui.add_space(4.0);
            ui.weak("Os reflexos aparecem no estilo \"Materiais fotorrealistas\".");
        }
    }

    // ------------------------------------------------------------ outliner

    pub(crate) fn outliner_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.outliner_filter).hint_text("Filtrar").desired_width(150.0));
            ui.checkbox(&mut self.outliner_by_tag, "Por etiqueta");
        });
        ui.separator();
        let q = crate::chrome::fold(&self.outliner_filter);
        let mut actions: Vec<OutAction> = Vec::new();
        let model_name = self.file.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or("Modelo".into());
        ui.label(RichText::new(model_name).strong());
        if self.outliner_by_tag {
            let mut tags: Vec<(Option<usize>, String)> = vec![(None, "Sem etiqueta".into())];
            tags.extend(self.doc.tags.iter().enumerate().map(|(i, t)| (Some(i), t.name.clone())));
            for (t, name) in tags {
                let items: Vec<usize> = (0..self.doc.root.instances.len()).filter(|i| self.doc.root.instances[*i].tag == t).collect();
                if items.is_empty() {
                    continue;
                }
                egui::CollapsingHeader::new(name).id_salt(("out-tag", t)).default_open(true).show(ui, |ui| {
                    for i in items {
                        self.outliner_node(ui, &[], i, &q, 0, &mut actions);
                    }
                });
            }
        } else {
            for i in 0..self.doc.root.instances.len() {
                self.outliner_node(ui, &[], i, &q, 0, &mut actions);
            }
        }
        self.outliner_sections(ui, &q);
        if self.doc.root.instances.is_empty() && self.doc.sections.is_empty() {
            ui.weak("Sem grupos, componentes ou planos de seção no nível superior.");
        }
        for a in actions {
            self.apply_outliner(a);
        }
    }

    fn section_label(&self, k: usize) -> String {
        let s = &self.doc.sections[k];
        if s.name.is_empty() {
            format!("Plano de seção {}", k + 1)
        } else {
            s.name.clone()
        }
    }

    /// Section planes listed after the groups/components.
    fn outliner_sections(&mut self, ui: &mut egui::Ui, q: &str) {
        let mut del = None;
        let mut activate = None;
        let mut zoom = None;
        let mut rename_done = None;
        for k in 0..self.doc.sections.len() {
            let label = self.section_label(k);
            if !q.is_empty() && !crate::chrome::fold(&label).contains(q) {
                continue;
            }
            let (active, hidden) = (self.doc.sections[k].active, self.doc.sections[k].hidden);
            ui.horizontal(|ui| {
                ui.add_space(18.0);
                let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                let c = Color32::from_rgb(220, 120, 20);
                ui.painter().add(egui::Shape::closed_line(
                    vec![r.left_top() + Vec2::new(3.0, 4.0), r.right_top() + Vec2::new(-2.0, 1.0), r.right_bottom() + Vec2::new(-3.0, -4.0), r.left_bottom() + Vec2::new(2.0, -1.0)],
                    Stroke::new(1.3, c),
                ));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(if active { "ativo" } else { "inativo" }).on_hover_text("Ativar/desativar o corte").clicked() {
                        activate = Some(k);
                    }
                    if ui.small_button(if hidden { "mostrar" } else { "ocultar" }).on_hover_text("Ocultar o símbolo do plano").clicked() {
                        self.checkpoint_keep_sel();
                        self.doc.sections[k].hidden = !hidden;
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        if let Some((rk, text)) = self.section_rename.as_mut().filter(|(rk, _)| *rk == k) {
                            let resp = ui.add(egui::TextEdit::singleline(text).desired_width(130.0));
                            resp.request_focus();
                            if resp.lost_focus() {
                                rename_done = Some((*rk, text.clone()));
                            }
                            return;
                        }
                        let mut text = RichText::new(&label);
                        if hidden {
                            text = text.italics().color(Color32::GRAY);
                        }
                        if self.outliner_section == Some(k) {
                            text = text.background_color(Color32::from_rgb(200, 222, 248));
                        }
                        let resp = ui.add(egui::Label::new(text).truncate().sense(Sense::click()));
                        if resp.clicked() {
                            self.outliner_section = if self.outliner_section == Some(k) { None } else { Some(k) };
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Renomear").clicked() {
                                self.section_rename = Some((k, self.doc.sections[k].name.clone()));
                                ui.close();
                            }
                            if ui.button(if active { "Desativar corte" } else { "Ativar corte" }).clicked() {
                                activate = Some(k);
                                ui.close();
                            }
                            if ui.button("Inverter").clicked() {
                                self.checkpoint_keep_sel();
                                self.doc.sections[k].n = -self.doc.sections[k].n;
                                self.dirty = true;
                                ui.close();
                            }
                            if ui.button("Zoom no plano").clicked() {
                                zoom = Some(k);
                                ui.close();
                            }
                            if ui.button("Apagar").clicked() {
                                del = Some(k);
                                ui.close();
                            }
                        });
                    });
                });
            });
        }
        if let Some((k, name)) = rename_done {
            self.section_rename = None;
            self.checkpoint_keep_sel();
            self.doc.sections[k].name = name;
        }
        if let Some(k) = activate {
            self.checkpoint_keep_sel();
            let was = self.doc.sections[k].active;
            for s in &mut self.doc.sections {
                s.active = false;
            }
            self.doc.sections[k].active = !was;
            self.dirty = true;
        }
        if let Some(k) = zoom {
            let s = &self.doc.sections[k];
            let r = self.doc.bounds().map(|(lo, hi)| (hi - lo).length() * 0.3).unwrap_or(2.0).max(1.0);
            let (p, d) = (s.p, glam::DVec3::splat(r));
            self.cam.frame_bounds(p - d, p + d);
        }
        if let Some(k) = del {
            self.checkpoint_keep_sel();
            self.doc.sections.remove(k);
            self.outliner_section = None;
            self.dirty = true;
        }
    }

    /// Does the instance (or any child) match the filter?
    fn outliner_matches(&self, parent: &[usize], i: usize, q: &str, depth: usize) -> bool {
        if q.is_empty() {
            return true;
        }
        let g = self.doc.geom(parent);
        let Some(inst) = g.instances.get(i) else { return false };
        let label = self.outliner_label(inst);
        if crate::chrome::fold(&label).contains(q) {
            return true;
        }
        if depth > 16 {
            return false;
        }
        let mut p = parent.to_vec();
        p.push(i);
        let n = self.doc.defs.get(inst.def).map(|d| d.geom.instances.len()).unwrap_or(0);
        (0..n).any(|c| self.outliner_matches(&p, c, q, depth + 1))
    }

    fn outliner_label(&self, inst: &crate::geom::Instance) -> String {
        let d = self.doc.defs.get(inst.def);
        let kind = if d.is_some_and(|d| d.group) { "Grupo" } else { d.map(|d| d.name.as_str()).unwrap_or("?") };
        if inst.name.is_empty() {
            kind.to_string()
        } else {
            format!("{} <{}>", inst.name, kind)
        }
    }

    fn outliner_node(&mut self, ui: &mut egui::Ui, parent: &[usize], i: usize, q: &str, depth: usize, actions: &mut Vec<OutAction>) {
        if depth > 16 || !self.outliner_matches(parent, i, q, 0) {
            return;
        }
        let g = self.doc.geom(parent);
        let Some(inst) = g.instances.get(i).cloned() else { return };
        let mut path = parent.to_vec();
        path.push(i);
        let children = self.doc.defs.get(inst.def).map(|d| d.geom.instances.len()).unwrap_or(0);
        let selected = self.path.as_slice() == parent && self.sel.insts.contains(&i);
        let editing = self.path == path;
        let label = self.outliner_label(&inst);
        let group = self.doc.defs.get(inst.def).is_some_and(|d| d.group);
        let renaming = self.outliner_rename.as_ref().is_some_and(|(p, _)| *p == path);
        let id = ui.make_persistent_id(("out", &path));
        let state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
        let row = |ui: &mut egui::Ui, actions: &mut Vec<OutAction>, rename: &mut Option<(Vec<usize>, String)>| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
            let c = if group { Color32::from_rgb(90, 100, 120) } else { Color32::from_rgb(40, 110, 200) };
            ui.painter().rect_stroke(r.shrink(2.0), 1.0, Stroke::new(1.3, c), StrokeKind::Inside);
            if !group {
                ui.painter().line_segment([r.shrink(2.0).left_top(), r.shrink(2.0).right_bottom()], Stroke::new(1.0, c));
            }
            if renaming {
                if let Some((_, text)) = rename.as_mut() {
                    let resp = ui.add(egui::TextEdit::singleline(text).desired_width(130.0));
                    resp.request_focus();
                    if resp.lost_focus() {
                        actions.push(OutAction::Rename(path.clone(), text.clone()));
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let lock = ui.small_button(if inst.locked { "bloq." } else { "livre" });
                if lock.on_hover_text(if inst.locked { "Bloqueado — clique para desbloquear" } else { "Clique para bloquear" }).clicked() {
                    actions.push(OutAction::Lock(path.clone(), !inst.locked));
                }
                if ui.small_button(if inst.hidden { "mostrar" } else { "ocultar" }).clicked() {
                    actions.push(OutAction::Hide(path.clone(), !inst.hidden));
                }
                if renaming {
                    return;
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let mut text = RichText::new(&label);
                if inst.hidden {
                    text = text.italics().color(Color32::GRAY);
                }
                if editing {
                    text = text.strong();
                }
                if selected {
                    text = text.background_color(Color32::from_rgb(200, 222, 248));
                }
                let resp = ui.add(egui::Label::new(text).truncate().sense(Sense::click()));
                if resp.clicked() {
                    actions.push(OutAction::Select(parent.to_vec(), i));
                }
                if resp.double_clicked() {
                    actions.push(OutAction::Edit(path.clone()));
                }
                resp.context_menu(|ui| {
                    if ui.button("Renomear").clicked() {
                        actions.push(OutAction::StartRename(path.clone(), inst.name.clone()));
                        ui.close();
                    }
                    if ui.button(if inst.hidden { "Mostrar" } else { "Ocultar" }).clicked() {
                        actions.push(OutAction::Hide(path.clone(), !inst.hidden));
                        ui.close();
                    }
                    if ui.button(if inst.locked { "Desbloquear" } else { "Bloquear" }).clicked() {
                        actions.push(OutAction::Lock(path.clone(), !inst.locked));
                        ui.close();
                    }
                    if ui.button("Editar grupo/componente").clicked() {
                        actions.push(OutAction::Edit(path.clone()));
                        ui.close();
                    }
                    if ui.button("Zoom na seleção").clicked() {
                        actions.push(OutAction::Zoom(path.clone()));
                        ui.close();
                    }
                });
                });
            });
        };
        let mut rename = if renaming { self.outliner_rename.take() } else { None };
        if children == 0 {
            ui.horizontal(|ui| {
                ui.add_space(18.0);
                row(ui, actions, &mut rename);
            });
        } else {
            let (_, _, body) = state
                .show_header(ui, |ui| {
                    row(ui, actions, &mut rename);
                })
                .body(|ui| {
                    for c in 0..children {
                        self.outliner_node(ui, &path, c, q, depth + 1, actions);
                    }
                });
            let _ = body;
        }
        if renaming {
            self.outliner_rename = rename;
        }
    }

    fn apply_outliner(&mut self, a: OutAction) {
        match a {
            OutAction::Select(parent, i) => {
                if self.path != parent {
                    self.path = parent;
                }
                self.sel = crate::geom::Sel { insts: [i].into(), ..Default::default() };
                self.dirty = true;
                self.cache_dirty = true;
            }
            OutAction::Edit(path) => {
                if self.inst_at(&path).is_some_and(|i| i.locked) {
                    self.message = "Grupo/componente bloqueado — desbloqueie para editar.".into();
                    return;
                }
                self.path = path;
                self.sel.clear();
                self.dirty = true;
                self.cache_dirty = true;
            }
            OutAction::StartRename(path, name) => self.outliner_rename = Some((path, name)),
            OutAction::Rename(path, name) => {
                self.outliner_rename = None;
                self.checkpoint_keep_sel();
                if let Some(i) = self.inst_at_mut(&path) {
                    i.name = name;
                }
            }
            OutAction::Hide(path, h) => {
                self.checkpoint_keep_sel();
                if let Some(i) = self.inst_at_mut(&path) {
                    i.hidden = h;
                }
            }
            OutAction::Lock(path, l) => {
                self.checkpoint_keep_sel();
                if let Some(i) = self.inst_at_mut(&path) {
                    i.locked = l;
                }
            }
            OutAction::Zoom(path) => {
                let (parent, i) = (path[..path.len() - 1].to_vec(), path[path.len() - 1]);
                let xf = self.doc.xf(&parent);
                if let Some(inst) = self.doc.geom(&parent).instances.get(i).cloned() {
                    if let Some((lo, hi)) = self.doc.def_bounds(inst.def) {
                        let m = xf * self.doc.eff_xf(&inst);
                        let pts = [lo, hi, glam::DVec3::new(lo.x, hi.y, lo.z), glam::DVec3::new(hi.x, lo.y, hi.z)].map(|p| m.transform_point3(p));
                        let (mut a, mut b) = (pts[0], pts[0]);
                        for p in pts {
                            a = a.min(p);
                            b = b.max(p);
                        }
                        self.cam.frame_bounds(a, b);
                    }
                }
            }
        }
    }

    fn inst_at(&self, path: &[usize]) -> Option<&crate::geom::Instance> {
        let (last, parent) = path.split_last()?;
        self.doc.geom(parent).instances.get(*last)
    }

    fn inst_at_mut(&mut self, path: &[usize]) -> Option<&mut crate::geom::Instance> {
        let (last, parent) = path.split_last()?;
        self.doc.geom_mut(parent).instances.get_mut(*last)
    }

    /// Remove locked instances from the selection before editing it.
    pub(crate) fn drop_locked(&mut self) -> bool {
        let g = self.doc.geom(&self.path);
        let locked: Vec<usize> = self.sel.insts.iter().copied().filter(|i| g.instances.get(*i).is_some_and(|x| x.locked)).collect();
        if locked.is_empty() {
            return false;
        }
        for i in locked {
            self.sel.insts.remove(&i);
        }
        self.message = "Itens bloqueados foram ignorados (desbloqueie no Outliner ou Informações da entidade).".into();
        true
    }
}

enum OutAction {
    Select(Vec<usize>, usize),
    Edit(Vec<usize>),
    StartRename(Vec<usize>, String),
    Rename(Vec<usize>, String),
    Hide(Vec<usize>, bool),
    Lock(Vec<usize>, bool),
    Zoom(Vec<usize>),
}
