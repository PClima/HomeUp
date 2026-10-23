//! Start page, recent files, app preferences, PNG/STL export, printing,
//! autosave and toolbar customization.

use crate::app::HomeUpApp;
use eframe::egui::{self, Align2, Color32, RichText, Vec2};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prefs {
    #[serde(default)]
    pub recent: Vec<PathBuf>,
    #[serde(default = "yes")]
    pub start_page: bool,
    #[serde(default = "yes")]
    pub scale_figure: bool,
    #[serde(default = "five")]
    pub autosave_min: u32,
    #[serde(default = "one")]
    pub ui_zoom: f32,
    /// Toolbar slots hidden by the user (indices into `chrome::SLOTS`).
    #[serde(default)]
    pub hidden_slots: Vec<usize>,
    /// SketchUp-desktop layout (toolbars, Large Tool Set, stacked tray).
    #[serde(default = "yes")]
    pub desktop_ui: bool,
    #[serde(default)]
    pub hidden_toolbars: Vec<String>,
    #[serde(default = "yes")]
    pub large_tool_set: bool,
    #[serde(default = "yes")]
    pub scene_tabs: bool,
    /// Panel titles left out of the Default Tray.
    #[serde(default = "default_tray_hidden")]
    pub tray_hidden: Vec<String>,
    /// Template used by File > New ("builtin:N" or a template file path).
    #[serde(default = "default_template")]
    pub default_template: String,
    /// Name used by Credits > Claim.
    #[serde(default)]
    pub author_name: String,
}

fn default_template() -> String {
    "builtin:0".into()
}

fn default_tray_hidden() -> Vec<String> {
    ["Exibição", "Informações do modelo", "Biblioteca", "Instrutor"].iter().map(|s| s.to_string()).collect()
}

fn yes() -> bool {
    true
}
fn five() -> u32 {
    5
}
fn one() -> f32 {
    1.0
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            recent: Vec::new(),
            start_page: true,
            scale_figure: true,
            autosave_min: 5,
            ui_zoom: 1.0,
            hidden_slots: Vec::new(),
            desktop_ui: true,
            hidden_toolbars: Vec::new(),
            large_tool_set: true,
            scene_tabs: true,
            tray_hidden: default_tray_hidden(),
            default_template: default_template(),
            author_name: String::new(),
        }
    }
}

pub fn app_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("HomeUp")
}

impl Prefs {
    pub fn load() -> Prefs {
        std::fs::read_to_string(app_dir().join("prefs.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    pub fn save(&self) {
        let dir = app_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(dir.join("prefs.json"), s);
        }
    }
}

/// Pending PNG export: (path, crop rect in physical pixels, then print?).
pub type PendingShot = Arc<Mutex<Option<String>>>;

impl HomeUpApp {
    pub(crate) fn add_recent(&mut self, p: &std::path::Path) {
        if self.snapshot.is_some() {
            return;
        }
        let p = p.to_path_buf();
        self.prefs.recent.retain(|r| r != &p);
        self.prefs.recent.insert(0, p);
        self.prefs.recent.truncate(10);
        self.prefs.save();
    }

    pub(crate) fn start_page(&mut self, ctx: &egui::Context) {
        if !self.show_home {
            return;
        }
        let mut open = true;
        let mut action: Option<u8> = None;
        let mut recent: Option<PathBuf> = None;
        egui::Window::new("Página inicial")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.label(RichText::new("HomeUp").size(26.0).strong().color(Color32::from_rgb(20, 60, 110)));
                ui.label("Modelagem 3D simples, inspirada no SketchUp.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Novo modelo").size(15.0)).clicked() {
                        action = Some(0);
                    }
                    if ui.button(RichText::new("Abrir…").size(15.0)).clicked() {
                        action = Some(1);
                    }
                    if ui.button(RichText::new("Importar modelo…").size(15.0)).clicked() {
                        action = Some(2);
                    }
                });
                ui.add_space(8.0);
                ui.label(RichText::new("Modelos").strong());
                if let Some(k) = self.template_list(ui) {
                    self.new_from_template(&k);
                }
                ui.add_space(8.0);
                ui.label(RichText::new("Arquivos recentes").strong());
                if self.prefs.recent.is_empty() {
                    ui.weak("Nenhum arquivo aberto ainda.");
                }
                for p in self.prefs.recent.clone() {
                    let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    let exists = p.exists();
                    ui.horizontal(|ui| {
                        if ui.add_enabled(exists, egui::Button::new(&name)).on_hover_text(p.display().to_string()).clicked() {
                            recent = Some(p.clone());
                        }
                        ui.weak(p.parent().map(|d| d.display().to_string()).unwrap_or_default());
                    });
                }
                ui.add_space(6.0);
                ui.separator();
                if ui.checkbox(&mut self.prefs.start_page, "Mostrar esta página ao iniciar").changed() {
                    self.prefs.save();
                }
            });
        if let Some(p) = recent {
            self.open_path(p);
            self.show_home = false;
        }
        match action {
            Some(0) => {
                self.new_file_pub();
                self.show_home = false;
            }
            Some(1) => {
                self.show_home = false;
                self.open_file_pub();
            }
            Some(2) => {
                self.show_home = false;
                self.import_file();
            }
            _ => {}
        }
        if !open {
            self.show_home = false;
        }
    }

    pub(crate) fn prefs_window(&mut self, ctx: &egui::Context) {
        if !self.show_prefs {
            return;
        }
        let mut open = true;
        let mut changed = false;
        egui::Window::new("Configurações do aplicativo").collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
            changed |= ui.checkbox(&mut self.prefs.start_page, "Mostrar a página inicial ao abrir").changed();
            changed |= ui.checkbox(&mut self.prefs.scale_figure, "Incluir a figura de escala em modelos novos").changed();
            changed |= ui.add(egui::Slider::new(&mut self.prefs.autosave_min, 0..=30).text("Salvamento automático (min, 0 = desligado)")).changed();
            changed |= ui.add(egui::Slider::new(&mut self.prefs.ui_zoom, 0.75..=2.0).text("Escala da interface")).changed();
            ui.weak(format!("Preferências em {}", app_dir().display()));
            ui.weak(format!("Salvamento automático em {}", app_dir().join("autosave.homeup").display()));
        });
        if changed {
            self.prefs.save();
            ctx.set_zoom_factor(self.prefs.ui_zoom);
        }
        if !open {
            self.show_prefs = false;
        }
    }

    pub(crate) fn autosave(&mut self, now: f64) {
        if self.prefs.autosave_min == 0 || !self.modified || self.snapshot.is_some() {
            return;
        }
        if now - self.last_autosave < self.prefs.autosave_min as f64 * 60.0 {
            return;
        }
        self.last_autosave = now;
        let dir = app_dir();
        let _ = std::fs::create_dir_all(&dir);
        let f = crate::doc::SaveFile {
            format: "homeup".into(),
            version: 2,
            doc: self.doc.clone(),
            camera: self.cam.to_array(),
            ortho: self.cam.ortho,
        };
        if let Ok(s) = serde_json::to_string(&f) {
            let _ = std::fs::write(dir.join("autosave.homeup"), s);
        }
    }

    pub(crate) fn export_stl(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("STL", &["stl"]).set_file_name("modelo.stl").save_file() else { return };
        let mut tris: Vec<[glam::DVec3; 3]> = Vec::new();
        self.doc.visit(&[], &mut |it| {
            let flip = it.xf.determinant() < 0.0;
            for f in it.geom.faces.iter().filter(|f| self.doc.face_visible(f)) {
                for t in it.geom.triangulate_face(f) {
                    let p = t.map(|v| it.xf.transform_point3(it.geom.p(v)) * 1000.0);
                    tris.push(if flip { [p[0], p[2], p[1]] } else { p });
                }
            }
        });
        let mut out: Vec<u8> = Vec::with_capacity(84 + tris.len() * 50);
        let mut header = [0u8; 80];
        let h = b"HomeUp STL export (mm)";
        header[..h.len()].copy_from_slice(h);
        out.extend_from_slice(&header);
        out.extend_from_slice(&(tris.len() as u32).to_le_bytes());
        for t in &tris {
            let n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
            for v in [n, t[0], t[1], t[2]] {
                for c in [v.x, v.y, v.z] {
                    out.extend_from_slice(&(c as f32).to_le_bytes());
                }
            }
            out.extend_from_slice(&[0, 0]);
        }
        match std::fs::write(&path, out) {
            Ok(()) => self.message = format!("STL exportado (milímetros): {}", path.display()),
            Err(e) => self.message = format!("Erro ao exportar: {e}"),
        }
    }

    /// Export the viewport as PNG (optionally sending it to the printer).
    pub(crate) fn export_png(&mut self, ctx: &egui::Context, print: bool) {
        let path = if print {
            std::env::temp_dir().join("homeup-print.png")
        } else {
            let Some(p) = rfd::FileDialog::new().add_filter("Imagem PNG", &["png"]).set_file_name("vista.png").save_file() else { return };
            p
        };
        let ppp = ctx.pixels_per_point();
        let r = self.viewport_rect;
        let crop = [(r.min.x * ppp) as usize, (r.min.y * ppp) as usize, (r.width() * ppp) as usize, (r.height() * ppp) as usize];
        let status = self.shot_status.clone();
        let p = path.clone();
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
            let msg = match crate::filemenu::save_crop(&p, &img, crop) {
                Ok(()) => {
                    if print {
                        let _ = std::process::Command::new("powershell")
                            .args(["-NoProfile", "-Command", &format!("Start-Process -FilePath '{}' -Verb Print", p.display())])
                            .spawn();
                        "Imagem enviada para a impressora.".to_string()
                    } else {
                        format!("Imagem exportada: {}", p.display())
                    }
                }
                Err(e) => format!("Erro ao exportar imagem: {e}"),
            };
            *status.lock().unwrap() = Some(msg);
        })));
        self.message = "Gerando imagem…".into();
    }

    pub(crate) fn poll_shot_status(&mut self) {
        if let Some(m) = self.shot_status.lock().unwrap().take() {
            self.message = m;
        }
    }
}

pub fn save_crop(path: &std::path::Path, img: &egui::ColorImage, crop: [usize; 4]) -> Result<(), String> {
    let [x0, y0, w, h] = crop;
    let (iw, ih) = (img.size[0], img.size[1]);
    let w = w.min(iw.saturating_sub(x0));
    let h = h.min(ih.saturating_sub(y0));
    if w == 0 || h == 0 {
        return Err("área vazia".into());
    }
    let mut bytes = Vec::with_capacity(w * h * 4);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            bytes.extend_from_slice(&img.pixels[y * iw + x].to_array());
        }
    }
    let f = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(|e| e.to_string())?;
    wr.write_image_data(&bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crop_writes_png() {
        let mut img = egui::ColorImage::filled([4, 3], Color32::RED);
        img.pixels[5] = Color32::BLUE;
        let p = std::env::temp_dir().join("homeup-crop-test.png");
        save_crop(&p, &img, [1, 1, 2, 2]).unwrap();
        let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&p).unwrap()));
        let r = dec.read_info().unwrap();
        assert_eq!((r.info().width, r.info().height), (2, 2));
    }
}
