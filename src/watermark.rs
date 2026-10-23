//! Style watermarks: images drawn behind the model (background, blended in the
//! renderer's sky pass) or in front of it (overlay, painted by egui), stretched,
//! tiled or positioned, with optional mask (white = transparent).

use crate::app::HomeUpApp;
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Vec2};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum WmMode {
    #[default]
    Stretched,
    Tiled,
    Positioned,
}

impl WmMode {
    pub const ALL: [WmMode; 3] = [WmMode::Stretched, WmMode::Tiled, WmMode::Positioned];
    pub fn label(self) -> &'static str {
        match self {
            WmMode::Stretched => "Esticada na tela",
            WmMode::Tiled => "Lado a lado",
            WmMode::Positioned => "Posicionada",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Watermark {
    pub name: String,
    /// Index into `Document::watermark_images`.
    pub image: usize,
    /// In front of the model (otherwise behind it).
    pub overlay: bool,
    /// White becomes transparent, black takes the background color.
    pub mask: bool,
    /// Opacity 0..1.
    pub blend: f32,
    pub mode: WmMode,
    /// Anchor for "Positioned" (0..1 in x and y; 1,1 = bottom right).
    pub anchor: [f32; 2],
    /// Image scale (1 = original pixel size).
    pub scale: f32,
}

/// RGBA image ready for display (mask applied).
pub fn prepared(img: &image::RgbaImage, mask: bool, bg: [u8; 3]) -> image::RgbaImage {
    if !mask {
        return img.clone();
    }
    let mut out = img.clone();
    for p in out.pixels_mut() {
        let lum = (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0;
        let a = ((1.0 - lum) * p[3] as f32).round() as u8;
        *p = image::Rgba([bg[0], bg[1], bg[2], a]);
    }
    out
}

fn sig_of(w: &Watermark, len: usize, bg: [u8; 3]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (w.image, len, w.mask, bg).hash(&mut h);
    h.finish() | 1
}

impl HomeUpApp {
    /// Keep the renderer's background layers current: the matched photo (when
    /// its scene is showing) and every background watermark, the first one in
    /// the list drawn on top.
    pub(crate) fn sync_watermark(&mut self, rs: &eframe::egui_wgpu::RenderState) {
        enum Src {
            Photo(usize),
            Mark(usize, bool),
        }
        let bg = self.doc.style.bg;
        let mut layers: Vec<(u64, [f32; 8])> = Vec::new();
        let mut srcs: Vec<Src> = Vec::new();
        if let Some(mp) = self.current_photo() {
            if let Some(t) = self.doc.photos.get(mp.image) {
                layers.push(((t.bytes.len() as u64) << 8 | 5, [1.0, 3.0, 1.0, 0.0, t.w as f32, t.h as f32, 0.5, 0.5]));
                srcs.push(Src::Photo(mp.image));
            }
        }
        let st = &self.doc.style;
        if st.show_watermarks {
            for w in st.watermarks.iter().rev().filter(|w| !w.overlay && w.image < self.doc.watermark_images.len()) {
                let t = &self.doc.watermark_images[w.image];
                let mode = match w.mode {
                    WmMode::Stretched => 0.0,
                    WmMode::Tiled => 1.0,
                    WmMode::Positioned => 2.0,
                };
                layers.push((sig_of(w, t.bytes.len(), bg), [w.blend.clamp(0.0, 1.0), mode, w.scale, 0.0, t.w as f32, t.h as f32, w.anchor[0], w.anchor[1]]));
                srcs.push(Src::Mark(w.image, w.mask));
            }
        }
        let doc = &self.doc;
        let make = |k: usize| match srcs[k] {
            Src::Photo(i) => doc.photos.get(i).and_then(|t| t.decode()),
            Src::Mark(i, mask) => doc.watermark_images.get(i).and_then(|t| t.decode()).map(|img| prepared(&img, mask, bg)),
        };
        if let Some(r) = self.renderer.as_mut() {
            r.set_bg_layers(rs, &layers, make);
        }
    }

    /// Overlay watermarks, painted over the 3D view.
    pub(crate) fn paint_overlay_watermarks(&mut self, ctx: &egui::Context, painter: &egui::Painter, rect: Rect) {
        if !self.doc.style.show_watermarks {
            return;
        }
        let bg = self.doc.style.bg;
        let marks: Vec<Watermark> = self.doc.style.watermarks.iter().filter(|w| w.overlay && w.image < self.doc.watermark_images.len()).cloned().collect();
        let ppp = ctx.pixels_per_point();
        for w in marks {
            let t = &self.doc.watermark_images[w.image];
            let (iw, ih) = (t.w as f32, t.h as f32);
            let sig = sig_of(&w, t.bytes.len(), bg);
            let Some(id) = self.ui_thumb(ctx, &format!("wm-{}", w.image), sig, |app| {
                let t = &app.doc.watermark_images[w.image];
                let img = t.decode()?;
                let img = if img.width() > 2048 { image::imageops::thumbnail(&img, 2048, 2048) } else { img };
                Some(prepared(&img, w.mask, bg))
            }) else {
                continue;
            };
            let tint = Color32::from_white_alpha((w.blend.clamp(0.0, 1.0) * 255.0) as u8);
            let full = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            let size = Vec2::new(iw, ih) * w.scale / ppp;
            match w.mode {
                WmMode::Stretched => {
                    painter.image(id, rect, full, tint);
                }
                WmMode::Positioned => {
                    let min = rect.min + (rect.size() - size) * Vec2::new(w.anchor[0], w.anchor[1]);
                    painter.image(id, Rect::from_min_size(min, size), full, tint);
                }
                WmMode::Tiled => {
                    let (sx, sy) = (size.x.max(8.0), size.y.max(8.0));
                    let nx = (rect.width() / sx).ceil() as usize;
                    let ny = (rect.height() / sy).ceil() as usize;
                    for y in 0..ny.min(200) {
                        for x in 0..nx.min(200) {
                            let r = Rect::from_min_size(rect.min + Vec2::new(x as f32 * sx, y as f32 * sy), Vec2::new(sx, sy));
                            painter.with_clip_rect(rect).image(id, r, full, tint);
                        }
                    }
                }
            }
        }
    }

    /// Styles > Edit > Watermark settings.
    pub(crate) fn watermark_ui(&mut self, ui: &mut egui::Ui) {
        let mut st = self.doc.style.clone();
        ui.checkbox(&mut st.show_watermarks, "Exibir marcas d'água");
        if ui.button("+ Adicionar marca d'água…").clicked() {
            if let Some(path) = rfd::FileDialog::new().add_filter("Imagem", &["png", "jpg", "jpeg", "bmp", "webp"]).pick_file() {
                let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| crate::tex::TextureData::from_bytes(&name, b)) {
                    Ok(t) => {
                        self.doc.watermark_images.push(t);
                        st.watermarks.push(Watermark {
                            name,
                            image: self.doc.watermark_images.len() - 1,
                            overlay: false,
                            mask: false,
                            blend: 0.6,
                            mode: WmMode::Positioned,
                            anchor: [1.0, 1.0],
                            scale: 1.0,
                        });
                    }
                    Err(e) => self.message = format!("Imagem inválida: {e}"),
                }
            }
        }
        let n = st.watermarks.len();
        let mut swap = None;
        let mut del = None;
        for k in 0..n {
            let w = &mut st.watermarks[k];
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut w.name).desired_width(120.0));
                    if ui.add_enabled(k > 0, egui::Button::new("Subir").small()).clicked() {
                        swap = Some((k, k - 1));
                    }
                    if ui.add_enabled(k + 1 < n, egui::Button::new("Descer").small()).clicked() {
                        swap = Some((k, k + 1));
                    }
                    if ui.small_button("Apagar").clicked() {
                        del = Some(k);
                    }
                });
                ui.horizontal(|ui| {
                    ui.radio_value(&mut w.overlay, false, "Fundo");
                    ui.radio_value(&mut w.overlay, true, "Sobreposição");
                    ui.checkbox(&mut w.mask, "Criar máscara");
                });
                ui.horizontal(|ui| {
                    ui.label("Mesclar");
                    ui.add(egui::Slider::new(&mut w.blend, 0.0..=1.0).fixed_decimals(2));
                });
                egui::ComboBox::from_id_salt(("wm-mode", k)).selected_text(w.mode.label()).show_ui(ui, |ui| {
                    for m in WmMode::ALL {
                        ui.selectable_value(&mut w.mode, m, m.label());
                    }
                });
                if w.mode != WmMode::Stretched {
                    ui.horizontal(|ui| {
                        ui.label("Escala");
                        ui.add(egui::Slider::new(&mut w.scale, 0.05..=4.0).logarithmic(true).fixed_decimals(2));
                    });
                }
                if w.mode == WmMode::Positioned {
                    ui.label(RichText::new("Posição").small());
                    egui::Grid::new(("wm-pos", k)).spacing([2.0, 2.0]).show(ui, |ui| {
                        for y in 0..3 {
                            for x in 0..3 {
                                let a = [x as f32 * 0.5, y as f32 * 0.5];
                                if ui.selectable_label(w.anchor == a, "  ").clicked() {
                                    w.anchor = a;
                                }
                            }
                            ui.end_row();
                        }
                    });
                }
            });
        }
        if let Some((a, b)) = swap {
            st.watermarks.swap(a, b);
        }
        if let Some(k) = del {
            st.watermarks.remove(k);
        }
        if n == 0 {
            ui.weak("Logotipos ou carimbos aparecem atrás (fundo) ou na frente (sobreposição) do modelo.");
        }
        if st != self.doc.style {
            if !st.name.ends_with('*') {
                st.name.push('*');
            }
            self.doc.style = st;
            self.modified = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mask_makes_white_transparent() {
        let img = image::RgbaImage::from_fn(2, 1, |x, _| if x == 0 { image::Rgba([255, 255, 255, 255]) } else { image::Rgba([0, 0, 0, 255]) });
        let m = prepared(&img, true, [10, 20, 30]);
        assert_eq!(m.get_pixel(0, 0)[3], 0);
        assert_eq!(*m.get_pixel(1, 0), image::Rgba([10, 20, 30, 255]));
    }
}
