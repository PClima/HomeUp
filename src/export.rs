//! Export Image: high-resolution renders without the interface, either with the
//! real-time renderer (instant) or the photoreal path tracer (pathtrace.rs).

use crate::app::HomeUpApp;
use eframe::egui::{self, RichText};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Engine {
    Realtime,
    PathTraced,
}

#[derive(Clone, Debug)]
pub struct ExportCfg {
    pub width: u32,
    pub height: u32,
    pub engine: Engine,
    /// Samples per pixel for the path tracer.
    pub samples: u32,
    pub keep_view_aspect: bool,
}

impl Default for ExportCfg {
    fn default() -> Self {
        ExportCfg { width: 3840, height: 2160, engine: Engine::Realtime, samples: 256, keep_view_aspect: false }
    }
}

pub const PRESETS: [(&str, u32, u32); 6] = [
    ("Full HD (1920 × 1080)", 1920, 1080),
    ("2K (2560 × 1440)", 2560, 1440),
    ("4K (3840 × 2160)", 3840, 2160),
    ("8K (7680 × 4320)", 7680, 4320),
    ("Quadrado 4K (3840 × 3840)", 3840, 3840),
    ("A3 300 dpi (4961 × 3508)", 4961, 3508),
];

/// A pending real-time export (needs the GPU, so it runs inside the viewport frame).
pub struct PendingRaster {
    pub path: std::path::PathBuf,
    pub size: [u32; 2],
}

impl HomeUpApp {
    /// Tape Measure resize: after measuring, typing the real length rescales the
    /// active context (the whole model, or the group/component being edited).
    pub(crate) fn tape_resize_window(&mut self, ctx: &egui::Context) {
        let Some((measured, wanted)) = self.tape_resize else { return };
        let mut answer = None;
        let inside = !self.path.is_empty();
        egui::Window::new("Redimensionar").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).show(ctx, |ui| {
            ui.label(format!(
                "Redimensionar {} para que {} passe a medir {}?",
                if inside { "este grupo/componente" } else { "o modelo inteiro" },
                crate::units::format_len(measured),
                crate::units::format_len(wanted)
            ));
            ui.weak(format!("Fator de escala: {:.4}", wanted / measured));
            ui.horizontal(|ui| {
                if ui.button("Redimensionar").clicked() {
                    answer = Some(true);
                }
                if ui.button("Cancelar").clicked() {
                    answer = Some(false);
                }
            });
        });
        match answer {
            Some(true) => {
                self.tape_resize = None;
                self.resize_context(wanted / measured);
            }
            Some(false) => self.tape_resize = None,
            None => {}
        }
    }

    pub(crate) fn resize_context(&mut self, f: f64) {
        if !(f.is_finite() && f > 1e-6) {
            return;
        }
        self.checkpoint_keep_sel();
        let s = glam::DMat4::from_scale(glam::DVec3::splat(f));
        let root = self.path.is_empty();
        let g = self.active_mut();
        for v in &mut g.verts {
            *v *= f;
        }
        for i in &mut g.instances {
            i.xf = s * i.xf;
        }
        if root {
            for gd in &mut self.doc.guides {
                match gd {
                    crate::doc::Guide::Line { p, .. } | crate::doc::Guide::Point { p } => *p *= f,
                }
            }
            for d in &mut self.doc.dims {
                d.a *= f;
                d.b *= f;
                d.off *= f;
            }
            for l in &mut self.doc.labels {
                l.anchor *= f;
                l.pos *= f;
            }
            for sp in &mut self.doc.sections {
                sp.p *= f;
            }
        }
        self.cache_dirty = true;
        self.dirty = true;
        self.message = format!("Redimensionado com fator {f:.4}.");
    }

    pub(crate) fn export_window(&mut self, ctx: &egui::Context) {
        if !self.show_export {
            return;
        }
        let mut open = true;
        let mut go = false;
        let view = self.viewport_rect;
        egui::Window::new("Exportar imagem").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            let c = &mut self.export_cfg;
            ui.label(RichText::new("Tamanho").strong());
            egui::ComboBox::from_id_salt("export-preset")
                .selected_text(PRESETS.iter().find(|p| p.1 == c.width && p.2 == c.height).map(|p| p.0).unwrap_or("Personalizado"))
                .show_ui(ui, |ui| {
                    for (n, w, h) in PRESETS {
                        if ui.selectable_label(c.width == w && c.height == h, n).clicked() {
                            c.width = w;
                            c.height = h;
                        }
                    }
                });
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut c.width).range(64..=16384).suffix(" px"));
                ui.label("×");
                ui.add(egui::DragValue::new(&mut c.height).range(64..=16384).suffix(" px"));
            });
            if ui.checkbox(&mut c.keep_view_aspect, "Mesma proporção da janela de desenho").changed() || c.keep_view_aspect {
                let a = (view.width() / view.height().max(1.0)) as f64;
                c.height = ((c.width as f64 / a).round() as u32).max(64);
            }
            ui.separator();
            ui.label(RichText::new("Renderização").strong());
            ui.radio_value(&mut c.engine, Engine::Realtime, "Rápida — igual à tela (instantânea)");
            ui.radio_value(&mut c.engine, Engine::PathTraced, "Fotorrealista — luz rebatida, sombras suaves e reflexos (mais lenta)");
            if c.engine == Engine::PathTraced {
                ui.horizontal(|ui| {
                    ui.label("Qualidade:");
                    for (l, s) in [("Rascunho", 32u32), ("Boa", 128), ("Alta", 512), ("Máxima", 2048)] {
                        ui.selectable_value(&mut c.samples, s, l);
                    }
                });
                ui.weak(format!("{} amostras por pixel; a imagem aparece aos poucos e pode ser salva a qualquer momento.", c.samples));
            }
            ui.weak("A imagem sai sem eixos, guias, seleção e anotações da interface.");
            ui.add_space(4.0);
            if ui.button(RichText::new("Exportar…").strong()).clicked() {
                go = true;
            }
        });
        if !open {
            self.show_export = false;
        }
        if go {
            let cfg = self.export_cfg.clone();
            match cfg.engine {
                Engine::Realtime => {
                    if let Some(p) = rfd::FileDialog::new().add_filter("Imagem PNG", &["png"]).add_filter("JPEG", &["jpg"]).set_file_name("render.png").save_file() {
                        self.pending_raster = Some(PendingRaster { path: p, size: [cfg.width, cfg.height] });
                        self.show_export = false;
                        self.message = "Gerando imagem…".into();
                    }
                }
                Engine::PathTraced => {
                    self.show_export = false;
                    self.start_path_trace(ctx, [cfg.width, cfg.height], cfg.samples);
                }
            }
        }
    }

    /// Run a pending real-time export (called from the viewport, where the GPU is available).
    pub(crate) fn run_pending_raster(&mut self, rs: &eframe::egui_wgpu::RenderState, ppp: f32) {
        let Some(job) = self.pending_raster.take() else { return };
        let mut cam = self.cam.clone();
        cam.size = glam::DVec2::new(job.size[0] as f64, job.size[1] as f64);
        let vpd = cam.view_proj();
        let clip = crate::annot::active_section(&self.doc).map(|p| [p.n.x as f32, p.n.y as f32, p.n.z as f32, p.d as f32]);
        // keep line widths proportional to the picture size
        let scale = (job.size[1] as f32 / (self.viewport_rect.height() * ppp).max(1.0)) * ppp;
        let fp = self.frame_params(vpd, clip, scale.max(1.0));
        let Some(r) = self.renderer.as_mut() else { return };
        match r.render_offscreen(rs, job.size, &fp) {
            Ok(img) => {
                let res = if job.path.extension().is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg")) {
                    image::DynamicImage::ImageRgba8(img).to_rgb8().save(&job.path)
                } else {
                    img.save(&job.path)
                };
                self.message = match res {
                    Ok(()) => format!("Imagem {}×{} exportada: {}", job.size[0], job.size[1], job.path.display()),
                    Err(e) => format!("Erro ao salvar: {e}"),
                };
            }
            Err(e) => self.message = format!("Não foi possível renderizar nesse tamanho: {e}"),
        }
    }
}
