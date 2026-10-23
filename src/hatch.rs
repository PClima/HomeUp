//! Section-fill hatch patterns (Hatch Patterns panel) and tag dash patterns.
//! Hatches are procedural: the face shader draws them in the section plane's
//! coordinates, so they stay crisp at any zoom. The same formulas render the
//! panel previews on the CPU.

use crate::app::HomeUpApp;
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Vec2};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HatchPattern {
    #[default]
    Solid,
    Diagonal,
    Cross,
    Brick,
    Concrete,
    Earth,
    Insulation,
}

impl HatchPattern {
    pub const ALL: [HatchPattern; 7] =
        [HatchPattern::Solid, HatchPattern::Diagonal, HatchPattern::Cross, HatchPattern::Brick, HatchPattern::Concrete, HatchPattern::Earth, HatchPattern::Insulation];
    pub fn label(self) -> &'static str {
        match self {
            HatchPattern::Solid => "Sólido",
            HatchPattern::Diagonal => "Diagonal (ANSI31)",
            HatchPattern::Cross => "Cruzado",
            HatchPattern::Brick => "Alvenaria / tijolo",
            HatchPattern::Concrete => "Concreto",
            HatchPattern::Earth => "Terra",
            HatchPattern::Insulation => "Isolamento",
        }
    }
    pub fn index(self) -> f32 {
        Self::ALL.iter().position(|p| *p == self).unwrap_or(0) as f32
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hatch {
    pub pattern: HatchPattern,
    /// Pattern repeat (m).
    pub scale: f32,
    pub angle_deg: f32,
    pub line: [u8; 3],
    pub fill: [u8; 3],
    /// Imported .PAT pattern (index into `Document::hatch_pats`); overrides `pattern`.
    #[serde(default)]
    pub custom: Option<usize>,
    /// Image hatch (index into `Document::textures`, tiled every `scale` m); overrides the rest.
    #[serde(default)]
    pub image: Option<usize>,
}

impl Default for Hatch {
    fn default() -> Self {
        Hatch { pattern: HatchPattern::Solid, scale: 0.25, angle_deg: 0.0, line: [30, 30, 30], fill: [40, 40, 40], custom: None, image: None }
    }
}

/// One line family of an AutoCAD-style .PAT hatch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PatFamily {
    pub angle_deg: f64,
    pub origin: [f64; 2],
    /// Offset between successive lines: along the line, across it.
    pub delta: [f64; 2],
    /// Dash pattern: positive = line, negative = gap, 0 = dot. Empty = solid.
    pub dashes: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PatPattern {
    pub name: String,
    pub families: Vec<PatFamily>,
    /// Meters per pattern unit (mm or inch files).
    pub unit_m: f64,
}

/// Parse the first pattern of a .PAT file.
pub fn parse_pat(text: &str) -> Result<PatPattern, String> {
    let mut unit_m = 0.001;
    let mut name: Option<String> = None;
    let mut families = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        let upper = line.to_uppercase();
        if upper.starts_with(";%UNITS=") {
            unit_m = if upper.contains("INCH") { 0.0254 } else { 0.001 };
            continue;
        }
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(h) = line.strip_prefix('*') {
            if name.is_some() {
                break; // only one pattern per file
            }
            name = Some(h.split(',').next().unwrap_or("").trim().to_string());
            continue;
        }
        if name.is_none() {
            continue;
        }
        let nums: Vec<f64> = line.split(',').filter_map(|s| s.trim().parse::<f64>().ok()).collect();
        if nums.len() < 5 {
            return Err(format!("linha inválida no .PAT: \"{line}\""));
        }
        families.push(PatFamily { angle_deg: nums[0], origin: [nums[1], nums[2]], delta: [nums[3], nums[4]], dashes: nums[5..].iter().take(6).copied().collect() });
    }
    let name = name.ok_or("arquivo .PAT sem padrão (*NOME)")?;
    if families.is_empty() {
        return Err("padrão sem linhas".into());
    }
    families.truncate(8);
    Ok(PatPattern { name, families, unit_m })
}

/// Ink coverage of a .PAT pattern at `q` (pattern units), line half-width `w`.
pub fn pat_ink(p: &PatPattern, q: [f64; 2], w: f64) -> f64 {
    let mut ink: f64 = 0.0;
    for f in &p.families {
        let dy = f.delta[1];
        if dy.abs() < 1e-9 {
            continue;
        }
        let (sn, cs) = f.angle_deg.to_radians().sin_cos();
        let (px, py) = (q[0] - f.origin[0], q[1] - f.origin[1]);
        let (s, t) = (px * cs + py * sn, -px * sn + py * cs);
        let k = (t / dy).round();
        if (t - k * dy).abs() > w {
            continue;
        }
        if f.dashes.is_empty() {
            return 1.0;
        }
        let total: f64 = f.dashes.iter().map(|d| d.abs().max(w)).sum();
        let sp = s - k * f.delta[0];
        let pos = sp.rem_euclid(total);
        let mut acc = 0.0;
        for d in &f.dashes {
            let len = d.abs().max(w);
            if pos >= acc && pos < acc + len && *d >= 0.0 {
                ink = 1.0;
            }
            acc += len;
        }
    }
    ink
}

/// GPU records (header + 8 families × 3 vec4 per pattern) for the face shader.
pub fn gpu_pats(pats: &[PatPattern]) -> Vec<[f32; 4]> {
    let mut out = Vec::with_capacity(pats.len() * 25);
    for p in pats {
        out.push([p.families.len().min(8) as f32, 0.0, 0.0, 0.0]);
        for k in 0..8 {
            match p.families.get(k) {
                Some(f) => {
                    let d = |i: usize| f.dashes.get(i).copied().unwrap_or(0.0) as f32;
                    out.push([f.angle_deg.to_radians() as f32, f.origin[0] as f32, f.origin[1] as f32, f.delta[0] as f32]);
                    out.push([f.delta[1] as f32, f.dashes.len().min(6) as f32, d(0), d(1)]);
                    out.push([d(2), d(3), d(4), d(5)]);
                }
                None => out.extend([[0.0; 4]; 3]),
            }
        }
    }
    out
}

fn preview_pat(p: &PatPattern, line: [u8; 3], fill: [u8; 3], size: u32) -> image::RgbaImage {
    let span = p.families.iter().map(|f| f.delta[1].abs()).fold(0.0, f64::max).max(1e-6) * 6.0;
    let w = span / size as f64 * 0.7;
    image::RgbaImage::from_fn(size, size, |x, y| {
        let k = pat_ink(p, [x as f64 / size as f64 * span, (size - y) as f64 / size as f64 * span], w);
        let mix = |a: u8, b: u8| (a as f64 * (1.0 - k) + b as f64 * k) as u8;
        image::Rgba([mix(fill[0], line[0]), mix(fill[1], line[1]), mix(fill[2], line[2]), 255])
    })
}

/// Ink coverage (0 = fill color, 1 = line color) at pattern coordinates `q`
/// (one unit = one repeat). Mirrors `hatch()` in the face shader.
pub fn ink(p: HatchPattern, q: [f64; 2]) -> f64 {
    let line = |x: f64, th: f64| if ((x + 0.5).rem_euclid(1.0) - 0.5).abs() < th { 1.0 } else { 0.0 };
    let (x, y) = (q[0], q[1]);
    match p {
        HatchPattern::Solid => 0.0,
        HatchPattern::Diagonal => line((x + y) * std::f64::consts::FRAC_1_SQRT_2 * 2.0, 0.06),
        HatchPattern::Cross => line((x + y) * std::f64::consts::FRAC_1_SQRT_2 * 2.0, 0.06).max(line((x - y) * std::f64::consts::FRAC_1_SQRT_2 * 2.0, 0.06)),
        HatchPattern::Brick => {
            let row = (y * 2.0).floor();
            let off = if row.rem_euclid(2.0) == 1.0 { 0.5 } else { 0.0 };
            line(y * 2.0, 0.05).max(line(x + off, 0.025))
        }
        HatchPattern::Concrete => {
            let cell = [(x * 4.0).floor(), (y * 4.0).floor()];
            let h = ((cell[0] * 12.9898 + cell[1] * 78.233).sin() * 43758.5453).rem_euclid(1.0);
            let c = [(x * 4.0).rem_euclid(1.0) - 0.5, (y * 4.0).rem_euclid(1.0) - 0.5];
            let (cx, cy) = (c[0] - (h - 0.5) * 0.5, c[1] - (h * 7.0).rem_euclid(1.0) * 0.4 + 0.2);
            if h > 0.45 && (cx * cx + cy * cy).sqrt() < 0.11 { 1.0 } else { 0.0 }
        }
        HatchPattern::Earth => {
            let band = (x.floor() + y.floor()).rem_euclid(2.0);
            if band == 0.0 { line(y * 3.0, 0.08) } else { line(x * 3.0, 0.08) }
        }
        HatchPattern::Insulation => line(y * 2.0 + 0.35 * (x * std::f64::consts::TAU).sin(), 0.07),
    }
}

pub fn preview(h: &Hatch, size: u32) -> image::RgbaImage {
    let (s, c) = (h.angle_deg.to_radians() as f64).sin_cos();
    image::RgbaImage::from_fn(size, size, |x, y| {
        let (u, v) = (x as f64 / size as f64 * 2.0, -(y as f64) / size as f64 * 2.0);
        let q = [u * c + v * s, -u * s + v * c];
        let k = ink(h.pattern, q);
        let mix = |a: u8, b: u8| (a as f64 * (1.0 - k) + b as f64 * k) as u8;
        image::Rgba([mix(h.fill[0], h.line[0]), mix(h.fill[1], h.line[1]), mix(h.fill[2], h.line[2]), 255])
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Dash {
    #[default]
    Solid,
    Dashed,
    Dotted,
    DashDot,
    LongDash,
}

impl Dash {
    pub const ALL: [Dash; 5] = [Dash::Solid, Dash::Dashed, Dash::Dotted, Dash::DashDot, Dash::LongDash];
    pub fn label(self) -> &'static str {
        match self {
            Dash::Solid => "Padrão",
            Dash::Dashed => "Tracejado",
            Dash::Dotted => "Pontilhado",
            Dash::DashDot => "Traço-ponto",
            Dash::LongDash => "Traço longo",
        }
    }
    /// Line `layer` marker for the edge shader (0 = solid).
    pub fn code(self) -> f32 {
        Self::ALL.iter().position(|d| *d == self).unwrap_or(0) as f32
    }
}

impl HomeUpApp {
    /// Hatch Patterns panel.
    pub(crate) fn hatch_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut st = self.doc.style.clone();
        ui.checkbox(&mut st.override_hatch, "Usar um preenchimento único do estilo")
            .on_hover_text("Desligado: cada material usa a sua hachura (Editar material > Hachura de corte)");
        let target_mat = (!st.override_hatch).then_some(self.edit_mat.or(self.cur_mat)).flatten().filter(|k| *k < self.doc.materials.len());
        let mut h = match target_mat {
            Some(k) => self.doc.materials[k].hatch.clone().unwrap_or(st.section_hatch.clone()),
            None => st.section_hatch.clone(),
        };
        ui.label(RichText::new(match target_mat {
            Some(k) => format!("Hachura do material \"{}\"", self.doc.materials[k].name),
            None => "Hachura do estilo".to_string(),
        }).strong());
        // pattern grid
        let w = ui.available_width();
        let cols = ((w / 60.0).floor() as usize).max(1);
        for row in HatchPattern::ALL.chunks(cols) {
            ui.horizontal(|ui| {
                for p in row {
                    ui.vertical(|ui| {
                        ui.set_width(54.0);
                        let sample = Hatch { pattern: *p, ..h.clone() };
                        let key = format!("hatch-{:?}-{:?}-{:?}-{}", p, sample.line, sample.fill, sample.angle_deg);
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(48.0), Sense::click());
                        if let Some(id) = self.ui_thumb(&ctx, &key, 1, |_| Some(preview(&sample, 64))) {
                            ui.painter().image(id, r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                        }
                        if h.pattern == *p && h.custom.is_none() && h.image.is_none() {
                            ui.painter().rect_stroke(r, 2.0, Stroke::new(2.0, Color32::from_rgb(30, 110, 220)), StrokeKind::Inside);
                        }
                        if resp.on_hover_text(p.label()).clicked() {
                            h.pattern = *p;
                            if h.custom.is_some() || h.image.is_some() {
                                h.scale = 0.25;
                            }
                            h.custom = None;
                            h.image = None;
                        }
                        ui.label(RichText::new(p.label().split(' ').next().unwrap_or("")).small());
                    });
                }
            });
        }
        // imported patterns and images
        let pats = self.doc.hatch_pats.clone();
        if !pats.is_empty() {
            ui.label(RichText::new("Importados (.PAT)").small().color(Color32::from_gray(110)));
        }
        for row in pats.iter().enumerate().collect::<Vec<_>>().chunks(cols) {
            ui.horizontal(|ui| {
                for (k, p) in row {
                    ui.vertical(|ui| {
                        ui.set_width(54.0);
                        let key = format!("pat-{k}-{}-{:?}-{:?}", p.name, h.line, h.fill);
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(48.0), Sense::click());
                        let (line, fill) = (h.line, h.fill);
                        if let Some(id) = self.ui_thumb(&ctx, &key, 1, |_| Some(preview_pat(p, line, fill, 64))) {
                            ui.painter().image(id, r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                        }
                        if h.custom == Some(*k) && h.image.is_none() {
                            ui.painter().rect_stroke(r, 2.0, Stroke::new(2.0, Color32::from_rgb(30, 110, 220)), StrokeKind::Inside);
                        }
                        if resp.on_hover_text(&p.name).clicked() {
                            h.custom = Some(*k);
                            h.image = None;
                            h.scale = p.unit_m as f32;
                        }
                        let short: String = p.name.chars().take(9).collect();
                        ui.label(RichText::new(short).small());
                    });
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.button("Importar .PAT…").clicked() {
                if let Some(path) = rfd::FileDialog::new().add_filter("Hachura AutoCAD", &["pat"]).pick_file() {
                    match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| parse_pat(&String::from_utf8_lossy(&b))) {
                        Ok(p) => {
                            h.scale = p.unit_m as f32;
                            self.doc.hatch_pats.push(p);
                            h.custom = Some(self.doc.hatch_pats.len() - 1);
                            h.image = None;
                        }
                        Err(e) => self.message = format!("Falha ao importar .PAT: {e}"),
                    }
                }
            }
            if ui.button("Usar imagem…").clicked() {
                if let Some(path) = rfd::FileDialog::new().add_filter("Imagem", &["png", "jpg", "jpeg"]).pick_file() {
                    let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| crate::tex::TextureData::from_bytes(&name, b)) {
                        Ok(t) => {
                            self.doc.textures.push(t);
                            h.image = Some(self.doc.textures.len() - 1);
                            h.scale = 0.5;
                        }
                        Err(e) => self.message = format!("Imagem inválida: {e}"),
                    }
                }
            }
        });
        if let Some(t) = h.image {
            ui.horizontal(|ui| {
                if let Some(id) = self.tex_thumb(&ctx, t) {
                    ui.image((id, Vec2::splat(32.0)));
                }
                ui.label("Hachura de imagem (lado a lado a cada \"Escala\")");
            });
        }
        ui.horizontal(|ui| {
            ui.label("Linhas");
            ui.color_edit_button_srgb(&mut h.line);
            ui.label("Fundo");
            ui.color_edit_button_srgb(&mut h.fill);
        });
        ui.horizontal(|ui| {
            ui.label("Escala");
            let speed = (h.scale as f64 * 0.02).max(0.00001);
            ui.add(egui::DragValue::new(&mut h.scale).range(0.00001..=10.0).speed(speed).max_decimals(5).suffix(" m"))
                .on_hover_text("Padrões internos e imagens: tamanho da repetição. .PAT: metros por unidade do arquivo.");
            ui.label("Rotação");
            ui.add(egui::DragValue::new(&mut h.angle_deg).range(-180.0..=180.0).speed(1.0).suffix("°"));
        });
        match target_mat {
            Some(k) => {
                if self.doc.materials[k].hatch.as_ref() != Some(&h) && (self.doc.materials[k].hatch.is_some() || h != st.section_hatch) {
                    self.doc.materials[k].hatch = Some(h);
                    self.modified = true;
                    self.dirty = true;
                }
                if self.doc.materials[k].hatch.is_some() && ui.small_button("Usar a hachura do estilo neste material").clicked() {
                    self.doc.materials[k].hatch = None;
                    self.dirty = true;
                }
            }
            None => st.section_hatch = h,
        }
        if !self.display.section_fill {
            ui.weak("O preenchimento de seção está desligado (Exibir > Preenchimento de seção).");
        }
        if st != self.doc.style {
            self.doc.style = st;
            self.modified = true;
            self.dirty = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_pat_files() {
        let src = ";%UNITS=MM\n*TIJOLO, Alvenaria de tijolos\n0, 0,0, 0,75\n90, 0,0, 75,75, 75,-75\n";
        let p = parse_pat(src).unwrap();
        assert_eq!(p.name, "TIJOLO");
        assert_eq!(p.families.len(), 2);
        assert_eq!(p.families[1].dashes, vec![75.0, -75.0]);
        assert!((p.unit_m - 0.001).abs() < 1e-12);
        // on a horizontal line, between lines, on the dashed vertical
        assert_eq!(pat_ink(&p, [10.0, 75.0], 1.0), 1.0);
        assert_eq!(pat_ink(&p, [10.0, 30.0], 1.0), 0.0);
        assert_eq!(pat_ink(&p, [0.0, 30.0], 1.0), 1.0);
        assert_eq!(gpu_pats(&[p]).len(), 25);
        assert!(parse_pat("garbage").is_err());
    }
    #[test]
    fn patterns_have_ink() {
        for p in HatchPattern::ALL {
            let n = (0..400).filter(|i| ink(p, [(*i % 20) as f64 * 0.05, (*i / 20) as f64 * 0.05]) > 0.5).count();
            if p == HatchPattern::Solid {
                assert_eq!(n, 0);
            } else {
                assert!(n > 0 && n < 400, "{p:?}: {n}");
            }
        }
    }
}
