//! Model templates (File > New from Template, Save as Template, default
//! template in Preferences): built-in presets plus user templates saved under
//! %APPDATA%\HomeUp\templates.

use crate::app::HomeUpApp;
use crate::doc::{Document, SaveFile};
use crate::settings::{Shadows, Style};
use crate::units::{AreaUnit, LenUnit, VolUnit};
use eframe::egui::{self, RichText};
use std::path::PathBuf;

pub const BUILTIN: [(&str, &str); 6] = [
    ("Arquitetura — metros", "Estilo de projeto, materiais fotorrealistas, unidades em metros."),
    ("Arquitetura — centímetros", "Igual ao anterior, com medidas em centímetros."),
    ("Marcenaria — milímetros", "Milímetros sem casas decimais, sombreado com texturas, sem figura humana."),
    ("Paisagismo e urbanismo", "Metros, sombras ligadas e neblina para cenas grandes."),
    ("Planta baixa", "Vista de topo em projeção paralela, linhas ocultas."),
    ("Croqui", "Estilo à mão (perfis, extensões, pontas e tremido)."),
];

pub fn templates_dir() -> PathBuf {
    crate::filemenu::app_dir().join("templates")
}

/// User templates (name, path).
pub fn user_templates() -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = std::fs::read_dir(templates_dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "homeup"))
        .map(|p| (p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), p))
        .collect();
    out.sort();
    out
}

/// Document (and optional camera: yaw, pitch, ortho) for a built-in template.
pub fn builtin(k: usize, scale_figure: bool) -> (Document, Option<(f64, f64, bool)>) {
    let mut doc = if scale_figure && k != 2 { Document::with_scale_figure() } else { Document::new() };
    let presets = Style::presets();
    let preset = |name: &str| presets.iter().find(|s| s.name == name).cloned().unwrap_or_default();
    let mut cam = None;
    match k {
        1 => {
            doc.units.length = LenUnit::Cm;
            doc.units.precision = 1;
            doc.units.area = AreaUnit::Cm2;
        }
        2 => {
            doc.units.length = LenUnit::Mm;
            doc.units.precision = 0;
            doc.units.area = AreaUnit::Cm2;
            doc.units.volume = VolUnit::Liters;
            doc.style = preset("Sombreado com texturas");
        }
        3 => {
            doc.shadows = Shadows { on: true, fog: true, fog_start: 60.0, fog_end: 600.0, ..Default::default() };
        }
        4 => {
            doc.style = preset("Linhas ocultas");
            cam = Some((-90.0, 89.9, true));
        }
        5 => doc.style = preset("Croqui à mão"),
        _ => {}
    }
    (doc, cam)
}

pub fn template_key_label(key: &str) -> String {
    if let Some(k) = key.strip_prefix("builtin:").and_then(|k| k.parse::<usize>().ok()) {
        return BUILTIN.get(k).map(|b| b.0.to_string()).unwrap_or_default();
    }
    PathBuf::from(key).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

impl HomeUpApp {
    /// Replace the document with a template (`builtin:N` or a file path).
    pub(crate) fn apply_template(&mut self, key: &str) {
        if let Some(k) = key.strip_prefix("builtin:").and_then(|k| k.parse::<usize>().ok()) {
            let (doc, cam) = builtin(k, self.prefs.scale_figure);
            self.doc = doc;
            if let Some((yaw, pitch, ortho)) = cam {
                self.cam.ortho = ortho;
                self.set_view(yaw, pitch);
            }
            return;
        }
        match std::fs::read_to_string(key).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str::<SaveFile>(&s).map_err(|e| e.to_string())) {
            Ok(f) => {
                self.doc = f.doc;
                self.cam.set_array(f.camera);
                self.cam.ortho = f.ortho;
            }
            Err(e) => self.message = format!("Modelo inválido ({e}); usando o padrão."),
        }
    }

    pub(crate) fn new_from_template(&mut self, key: &str) {
        self.new_file();
        self.apply_template(key);
        self.cache_dirty = true;
        self.dirty = true;
        self.show_templates = false;
        self.show_home = false;
        self.message = format!("Novo modelo: {}", template_key_label(key));
    }

    fn save_as_template(&mut self, name: &str) {
        let dir = templates_dir();
        let safe: String = name.chars().map(|c| if c.is_alphanumeric() || " -_".contains(c) { c } else { '_' }).collect();
        if safe.trim().is_empty() {
            self.message = "Dê um nome ao modelo.".into();
            return;
        }
        let path = dir.join(format!("{}.homeup", safe.trim()));
        let f = SaveFile { format: "homeup".into(), version: 2, doc: self.doc.clone(), camera: self.cam.to_array(), ortho: self.cam.ortho };
        match std::fs::create_dir_all(&dir).map_err(|e| e.to_string()).and_then(|_| serde_json::to_string(&f).map_err(|e| e.to_string())).and_then(|s| std::fs::write(&path, s).map_err(|e| e.to_string())) {
            Ok(()) => {
                if self.template_as_default {
                    self.prefs.default_template = path.display().to_string();
                    self.prefs.save();
                }
                self.message = format!("Modelo salvo: {}", path.display());
                self.show_save_template = false;
            }
            Err(e) => self.message = format!("Erro ao salvar o modelo: {e}"),
        }
    }

    /// Template chooser rows (shared by the window and the start page).
    pub(crate) fn template_list(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let mut pick = None;
        let default = self.prefs.default_template.clone();
        for (k, (name, desc)) in BUILTIN.iter().enumerate() {
            let key = format!("builtin:{k}");
            ui.horizontal(|ui| {
                let text = if key == default { RichText::new(format!("{name}  (padrão)")).strong() } else { RichText::new(*name) };
                if ui.button(text).on_hover_text(*desc).clicked() {
                    pick = Some(key.clone());
                }
                ui.weak(*desc);
            });
        }
        let user = user_templates();
        if !user.is_empty() {
            ui.label(RichText::new("Meus modelos").small());
        }
        for (name, path) in user {
            let key = path.display().to_string();
            ui.horizontal(|ui| {
                let text = if key == default { RichText::new(format!("{name}  (padrão)")).strong() } else { RichText::new(&name) };
                if ui.button(text).clicked() {
                    pick = Some(key.clone());
                }
                if ui.small_button("Apagar").clicked() {
                    let _ = std::fs::remove_file(&path);
                }
            });
        }
        pick
    }

    pub(crate) fn template_windows(&mut self, ctx: &egui::Context) {
        if self.show_templates {
            let mut open = true;
            let mut pick = None;
            let mut make_default = None;
            egui::Window::new("Novo a partir de modelo").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
                ui.set_min_width(460.0);
                pick = self.template_list(ui);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Modelo padrão para Novo (Ctrl+N):");
                    let cur = template_key_label(&self.prefs.default_template);
                    egui::ComboBox::from_id_salt("default-template").selected_text(cur).show_ui(ui, |ui| {
                        for (k, (name, _)) in BUILTIN.iter().enumerate() {
                            if ui.selectable_label(false, *name).clicked() {
                                make_default = Some(format!("builtin:{k}"));
                            }
                        }
                        for (name, path) in user_templates() {
                            if ui.selectable_label(false, name).clicked() {
                                make_default = Some(path.display().to_string());
                            }
                        }
                    });
                });
            });
            if let Some(d) = make_default {
                self.prefs.default_template = d;
                self.prefs.save();
            }
            if let Some(k) = pick {
                self.new_from_template(&k);
            }
            if !open {
                self.show_templates = false;
            }
        }
        if self.show_save_template {
            let mut open = true;
            let mut save = false;
            egui::Window::new("Salvar como modelo").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
                ui.label("O modelo guarda unidades, estilo, sombras, localização, ambiente, etiquetas, materiais e a geometria atual.");
                ui.horizontal(|ui| {
                    ui.label("Nome:");
                    ui.text_edit_singleline(&mut self.template_name);
                });
                ui.checkbox(&mut self.template_as_default, "Usar como modelo padrão");
                if ui.button("Salvar").clicked() {
                    save = true;
                }
            });
            if save {
                let name = self.template_name.clone();
                self.save_as_template(&name);
            }
            if !open {
                self.show_save_template = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtins_have_their_settings() {
        assert_eq!(builtin(1, true).0.units.length, LenUnit::Cm);
        let (d, _) = builtin(2, true);
        assert_eq!(d.units.length, LenUnit::Mm);
        assert!(d.defs.is_empty(), "joinery template has no scale figure");
        assert!(builtin(3, false).0.shadows.on);
        assert!(builtin(4, false).1.is_some_and(|c| c.2));
        assert!(builtin(5, false).0.style.jitter > 0.0);
        assert_eq!(template_key_label("builtin:0"), BUILTIN[0].0);
    }
}
