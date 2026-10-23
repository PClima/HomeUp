//! Length parsing/formatting for the Measurements box (internal unit: meters).
//! The display unit and precision come from the model settings.

use serde::{Deserialize, Serialize};
use std::cell::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LenUnit {
    #[default]
    M,
    Cm,
    Mm,
    In,
    Ft,
}

impl LenUnit {
    pub const ALL: [LenUnit; 5] = [LenUnit::M, LenUnit::Cm, LenUnit::Mm, LenUnit::In, LenUnit::Ft];
    pub fn meters(self) -> f64 {
        match self {
            LenUnit::M => 1.0,
            LenUnit::Cm => 0.01,
            LenUnit::Mm => 0.001,
            LenUnit::In => 0.0254,
            LenUnit::Ft => 0.3048,
        }
    }
    pub fn suffix(self) -> &'static str {
        match self {
            LenUnit::M => " m",
            LenUnit::Cm => " cm",
            LenUnit::Mm => " mm",
            LenUnit::In => "\"",
            LenUnit::Ft => "'",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            LenUnit::M => "Metros",
            LenUnit::Cm => "Centímetros",
            LenUnit::Mm => "Milímetros",
            LenUnit::In => "Polegadas",
            LenUnit::Ft => "Pés",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AreaUnit {
    #[default]
    M2,
    Cm2,
    Ft2,
}

impl AreaUnit {
    pub const ALL: [AreaUnit; 3] = [AreaUnit::M2, AreaUnit::Cm2, AreaUnit::Ft2];
    pub fn label(self) -> &'static str {
        match self {
            AreaUnit::M2 => "Metros quadrados",
            AreaUnit::Cm2 => "Centímetros quadrados",
            AreaUnit::Ft2 => "Pés quadrados",
        }
    }
    fn factor(self) -> (f64, &'static str) {
        match self {
            AreaUnit::M2 => (1.0, " m²"),
            AreaUnit::Cm2 => (1e4, " cm²"),
            AreaUnit::Ft2 => (10.7639, " pés²"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VolUnit {
    #[default]
    M3,
    Liters,
    Ft3,
}

impl VolUnit {
    pub const ALL: [VolUnit; 3] = [VolUnit::M3, VolUnit::Liters, VolUnit::Ft3];
    pub fn label(self) -> &'static str {
        match self {
            VolUnit::M3 => "Metros cúbicos",
            VolUnit::Liters => "Litros",
            VolUnit::Ft3 => "Pés cúbicos",
        }
    }
    fn factor(self) -> (f64, &'static str) {
        match self {
            VolUnit::M3 => (1.0, " m³"),
            VolUnit::Liters => (1000.0, " L"),
            VolUnit::Ft3 => (35.3147, " pés³"),
        }
    }
}

thread_local! {
    static CFG: Cell<(LenUnit, usize, AreaUnit, VolUnit)> = const { Cell::new((LenUnit::M, 2, AreaUnit::M2, VolUnit::M3)) };
}

pub fn set_units(len: LenUnit, precision: usize, area: AreaUnit, vol: VolUnit) {
    CFG.with(|c| c.set((len, precision.min(6), area, vol)));
}

fn cfg() -> (LenUnit, usize, AreaUnit, VolUnit) {
    CFG.with(|c| c.get())
}

pub fn format_len(m: f64) -> String {
    let (u, p, _, _) = cfg();
    format!("{:.*}{}", p, m / u.meters(), u.suffix()).replace('.', ",")
}

pub fn format_area(m2: f64) -> String {
    let (_, p, a, _) = cfg();
    let (f, s) = a.factor();
    format!("{:.*}{}", p, m2 * f, s).replace('.', ",")
}

#[allow(dead_code)]
pub fn format_volume(m3: f64) -> String {
    let (_, p, _, v) = cfg();
    let (f, s) = v.factor();
    format!("{:.*}{}", p, m3 * f, s).replace('.', ",")
}

/// Parse a single length such as `2`, `1,5`, `150cm`, `-30 mm`, `2.4m`, `12"`, `3'`.
/// A bare number uses the model's length unit.
pub fn parse_len(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', ".").to_lowercase();
    if s.is_empty() {
        return None;
    }
    let (num, scale) = if let Some(n) = s.strip_suffix("mm") {
        (n, 0.001)
    } else if let Some(n) = s.strip_suffix("cm") {
        (n, 0.01)
    } else if let Some(n) = s.strip_suffix('m') {
        (n, 1.0)
    } else if let Some(n) = s.strip_suffix('"') {
        (n, 0.0254)
    } else if let Some(n) = s.strip_suffix('\'') {
        (n, 0.3048)
    } else {
        (s.as_str(), cfg().0.meters())
    };
    num.trim().parse::<f64>().ok().map(|v| v * scale)
}

/// Parse a pair `a,b` / `a;b` for the rectangle. Missing entries are `None`.
pub fn parse_pair(s: &str) -> Option<(Option<f64>, Option<f64>)> {
    let sep = if s.contains(';') { ';' } else { ',' };
    let mut it = s.splitn(2, sep);
    let a = it.next().unwrap_or("").trim();
    let b = it.next()?.trim();
    let pa = if a.is_empty() { None } else { Some(parse_len(a)?) };
    let pb = if b.is_empty() { None } else { Some(parse_len(b)?) };
    Some((pa, pb))
}

/// Angle in degrees: `45`, `-30`, `22,5`, `90deg`, `90°`.
pub fn parse_angle(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', ".").to_lowercase();
    let s = s.trim_end_matches("deg").trim_end_matches('°').trim();
    s.parse::<f64>().ok()
}

/// Segment count such as `24s`.
pub fn parse_sides(s: &str) -> Option<usize> {
    let s = s.trim().to_lowercase();
    let n = s.strip_suffix('s')?.trim().parse::<usize>().ok()?;
    (3..=999).contains(&n).then_some(n)
}

/// Array after a copy: `x3`, `3x`, `*3` (multiply) or `/3` (divide).
/// Returns (count, divide).
pub fn parse_array(s: &str) -> Option<(usize, bool)> {
    let s = s.trim().to_lowercase();
    let (num, divide) = if let Some(n) = s.strip_prefix('/') {
        (n.to_string(), true)
    } else if let Some(n) = s.strip_prefix('x').or_else(|| s.strip_prefix('*')) {
        (n.to_string(), false)
    } else if let Some(n) = s.strip_suffix('x') {
        (n.to_string(), false)
    } else {
        return None;
    };
    let n = num.trim().parse::<usize>().ok()?;
    (1..=500).contains(&n).then_some((n, divide))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn angles_sides_arrays() {
        assert_eq!(parse_angle("45"), Some(45.0));
        assert_eq!(parse_angle("22,5"), Some(22.5));
        assert_eq!(parse_angle("90deg"), Some(90.0));
        assert_eq!(parse_sides("24s"), Some(24));
        assert_eq!(parse_sides("24"), None);
        assert_eq!(parse_array("x3"), Some((3, false)));
        assert_eq!(parse_array("5x"), Some((5, false)));
        assert_eq!(parse_array("/4"), Some((4, true)));
        assert_eq!(parse_array("3"), None);
    }
    #[test]
    fn parse() {
        assert_eq!(parse_len("2"), Some(2.0));
        assert_eq!(parse_len("1,5"), Some(1.5));
        assert!((parse_len("150cm").unwrap() - 1.5).abs() < 1e-12);
        assert!((parse_len("-30 mm").unwrap() + 0.03).abs() < 1e-12);
        assert_eq!(parse_pair("2,3"), Some((Some(2.0), Some(3.0))));
        assert_eq!(parse_pair("1,5;2"), Some((Some(1.5), Some(2.0))));
        assert_eq!(parse_pair(",3"), Some((None, Some(3.0))));
        assert_eq!(parse_pair("2"), None);
    }
}
