//! Model settings: tags, styles, shadows/sun, location and units.

use crate::units::{AreaUnit, LenUnit, VolUnit};
use glam::DVec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    pub visible: bool,
    pub color: [u8; 3],
    /// Dash pattern for edges on this tag.
    #[serde(default)]
    pub dash: crate::hatch::Dash,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FaceStyle {
    #[default]
    Photoreal,
    Shaded,
    Monochrome,
    HiddenLine,
    Wireframe,
}

impl FaceStyle {
    pub const ALL: [FaceStyle; 5] = [FaceStyle::Photoreal, FaceStyle::Shaded, FaceStyle::Monochrome, FaceStyle::HiddenLine, FaceStyle::Wireframe];
    pub fn label(self) -> &'static str {
        match self {
            FaceStyle::Photoreal => "Materiais fotorrealistas",
            FaceStyle::Shaded => "Sombreado com texturas",
            FaceStyle::Monochrome => "Monocromático",
            FaceStyle::HiddenLine => "Linhas ocultas",
            FaceStyle::Wireframe => "Estrutura de arame",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Style {
    pub name: String,
    pub face: FaceStyle,
    pub xray: bool,
    pub edges: bool,
    pub edge_color: [u8; 3],
    pub sky: bool,
    pub bg: [u8; 3],
    #[serde(default)]
    pub back_edges: bool,
    /// Profile (silhouette) line thickness in px; 0 = off.
    #[serde(default = "two")]
    pub profiles: f32,
    #[serde(default)]
    pub extension: f32,
    #[serde(default)]
    pub endpoints: f32,
    /// Depth cue: maximum edge width (px) for the nearest edges; 0 = off.
    #[serde(default)]
    pub depth_cue: f32,
    /// Jitter: random offset of edge ends (px); 0 = off.
    #[serde(default)]
    pub jitter: f32,
    #[serde(default)]
    pub edge_mode: EdgeColor,
    /// Draw tag dash patterns.
    #[serde(default = "yes")]
    pub dashes: bool,
    /// Section fill used when `override_hatch` is on (or a material has none).
    #[serde(default)]
    pub section_hatch: crate::hatch::Hatch,
    #[serde(default = "yes")]
    pub override_hatch: bool,
    #[serde(default)]
    pub watermarks: Vec<crate::watermark::Watermark>,
    #[serde(default = "yes")]
    pub show_watermarks: bool,
    #[serde(default = "yes")]
    pub ao: bool,
    #[serde(default = "half")]
    pub ao_strength: f32,
}

fn two() -> f32 {
    2.0
}
fn yes() -> bool {
    true
}
fn half() -> f32 {
    0.5
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EdgeColor {
    #[default]
    Same,
    Material,
    Axis,
}

impl EdgeColor {
    pub const ALL: [EdgeColor; 3] = [EdgeColor::Same, EdgeColor::Material, EdgeColor::Axis];
    pub fn label(self) -> &'static str {
        match self {
            EdgeColor::Same => "Todas iguais",
            EdgeColor::Material => "Por material",
            EdgeColor::Axis => "Por eixo",
        }
    }
}

/// Image-based environment (Environments panel).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,
    /// Equirectangular panorama (index into the document textures); None = procedural sky.
    pub image: Option<usize>,
    pub rotation_deg: f32,
    pub sky_exposure: f32,
    pub refl_exposure: f32,
    pub skydome: bool,
    pub reflections: bool,
}

impl Default for Environment {
    fn default() -> Self {
        Environment { name: "Céu padrão".into(), image: None, rotation_deg: 0.0, sky_exposure: 1.0, refl_exposure: 1.0, skydome: true, reflections: true }
    }
}

impl Default for Style {
    fn default() -> Self {
        Style {
            name: "Estilo de projeto arquitetônico".into(),
            face: FaceStyle::Photoreal,
            xray: false,
            edges: true,
            edge_color: [25, 25, 25],
            sky: true,
            bg: [214, 226, 236],
            back_edges: false,
            profiles: 2.0,
            extension: 0.0,
            endpoints: 0.0,
            depth_cue: 0.0,
            jitter: 0.0,
            edge_mode: EdgeColor::Same,
            dashes: true,
            section_hatch: Default::default(),
            override_hatch: true,
            watermarks: Vec::new(),
            show_watermarks: true,
            ao: true,
            ao_strength: 0.5,
        }
    }
}

impl Style {
    pub fn presets() -> Vec<Style> {
        let d = Style::default();
        vec![
            d.clone(),
            Style { name: "Sombreado com texturas".into(), face: FaceStyle::Shaded, ao: false, ..d.clone() },
            Style { name: "Raio-X".into(), xray: true, ..d.clone() },
            Style { name: "Arestas ocultas".into(), back_edges: true, face: FaceStyle::HiddenLine, sky: false, bg: [255, 255, 255], ..d.clone() },
            Style { name: "Croqui à mão".into(), face: FaceStyle::HiddenLine, extension: 6.0, endpoints: 6.0, jitter: 1.5, depth_cue: 3.0, profiles: 3.0, sky: false, bg: [252, 250, 244], ..d.clone() },
            Style { name: "Cor por eixo".into(), edge_mode: EdgeColor::Axis, ..d.clone() },
            Style { name: "Monocromático".into(), face: FaceStyle::Monochrome, ..d.clone() },
            Style { name: "Linhas ocultas".into(), face: FaceStyle::HiddenLine, sky: false, bg: [255, 255, 255], ..d.clone() },
            Style { name: "Estrutura de arame".into(), face: FaceStyle::Wireframe, sky: false, bg: [255, 255, 255], ..d.clone() },
            Style { name: "Sombreado sem arestas".into(), edges: false, ..d.clone() },
            Style { name: "Croqui azul".into(), face: FaceStyle::HiddenLine, edge_color: [30, 70, 160], sky: false, bg: [240, 245, 252], ..d },
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shadows {
    pub on: bool,
    pub month: u32,
    pub day: u32,
    /// Local time, minutes from midnight.
    pub minutes: u32,
    /// UTC offset in hours.
    pub tz: f64,
    /// Direct light intensity (0..100).
    pub light: f32,
    /// Ambient light (0..100).
    pub dark: f32,
    pub on_ground: bool,
    #[serde(default = "yes")]
    pub on_faces: bool,
    #[serde(default = "yes")]
    pub from_edges: bool,
    pub fog: bool,
    pub fog_start: f32,
    pub fog_end: f32,
}

impl Default for Shadows {
    fn default() -> Self {
        Shadows { on: false, month: 11, day: 8, minutes: 13 * 60 + 30, tz: -3.0, light: 80.0, dark: 45.0, on_ground: true, on_faces: true, from_edges: true, fog: false, fog_start: 30.0, fog_end: 200.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Location {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    /// Angle of true north from the green axis, degrees (clockwise from above).
    pub north_deg: f64,
}

impl Default for Location {
    fn default() -> Self {
        Location { name: "São Paulo, Brasil".into(), lat: -23.55, lon: -46.63, north_deg: 0.0 }
    }
}

pub const CITIES: [(&str, f64, f64, f64); 10] = [
    ("São Paulo, Brasil", -23.55, -46.63, -3.0),
    ("Rio de Janeiro, Brasil", -22.91, -43.17, -3.0),
    ("Brasília, Brasil", -15.79, -47.88, -3.0),
    ("Belo Horizonte, Brasil", -19.92, -43.94, -3.0),
    ("Porto Alegre, Brasil", -30.03, -51.23, -3.0),
    ("Recife, Brasil", -8.05, -34.88, -3.0),
    ("Manaus, Brasil", -3.12, -60.02, -4.0),
    ("Lisboa, Portugal", 38.72, -9.14, 0.0),
    ("Nova York, EUA", 40.71, -74.0, -5.0),
    ("Boulder, EUA", 40.01, -105.27, -7.0),
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitsCfg {
    pub length: LenUnit,
    pub precision: usize,
    pub area: AreaUnit,
    pub volume: VolUnit,
    pub angle_precision: usize,
    pub text_size: f32,
    pub dim_size: f32,
}

impl Default for UnitsCfg {
    fn default() -> Self {
        UnitsCfg { length: LenUnit::M, precision: 2, area: AreaUnit::M2, volume: VolUnit::M3, angle_precision: 1, text_size: 13.0, dim_size: 13.0 }
    }
}

fn day_of_year(month: u32, day: u32) -> f64 {
    const CUM: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    (CUM[(month.clamp(1, 12) - 1) as usize] + day.clamp(1, 31)) as f64
}

/// Direction toward the sun in world coordinates (z up, green axis = north
/// rotated by `north_deg`). The z component is the sine of the elevation.
pub fn sun_direction(s: &Shadows, loc: &Location) -> DVec3 {
    let n = day_of_year(s.month, s.day);
    let hour = s.minutes as f64 / 60.0;
    let g = std::f64::consts::TAU / 365.0 * (n - 1.0 + (hour - 12.0) / 24.0);
    let eqtime = 229.18 * (0.000075 + 0.001868 * g.cos() - 0.032077 * g.sin() - 0.014615 * (2.0 * g).cos() - 0.040849 * (2.0 * g).sin());
    let decl = 0.006918 - 0.399912 * g.cos() + 0.070257 * g.sin() - 0.006758 * (2.0 * g).cos() + 0.000907 * (2.0 * g).sin()
        - 0.002697 * (3.0 * g).cos()
        + 0.00148 * (3.0 * g).sin();
    let offset = eqtime + 4.0 * loc.lon - 60.0 * s.tz;
    let tst = hour * 60.0 + offset;
    let ha = (tst / 4.0 - 180.0).to_radians();
    let lat = loc.lat.to_radians();
    let east = -decl.cos() * ha.sin();
    let north = decl.sin() * lat.cos() - decl.cos() * ha.cos() * lat.sin();
    let up = lat.sin() * decl.sin() + lat.cos() * decl.cos() * ha.cos();
    let a = loc.north_deg.to_radians();
    // north = green axis rotated clockwise by `a`; east = red axis rotated the same way
    let nv = DVec3::new(a.sin(), a.cos(), 0.0);
    let ev = DVec3::new(a.cos(), -a.sin(), 0.0);
    (ev * east + nv * north + DVec3::Z * up).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sun_positions_make_sense() {
        let sp = Location::default();
        // noon in São Paulo near the December solstice: sun almost overhead, slightly south
        let s = Shadows { month: 12, day: 21, minutes: 12 * 60 + 10, tz: -3.0, ..Default::default() };
        let d = sun_direction(&s, &sp);
        assert!(d.z > 0.95, "{d:?}");
        // June noon: lower and to the north
        let s = Shadows { month: 6, day: 21, minutes: 12 * 60 + 10, ..s };
        let d = sun_direction(&s, &sp);
        assert!(d.z > 0.6 && d.z < 0.8 && d.y > 0.5, "{d:?}");
        // morning sun is in the east, afternoon in the west
        let m = sun_direction(&Shadows { minutes: 8 * 60, ..s.clone() }, &sp);
        let a = sun_direction(&Shadows { minutes: 16 * 60, ..s }, &sp);
        assert!(m.x > 0.3 && a.x < -0.3);
    }
}
