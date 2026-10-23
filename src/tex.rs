//! Image textures embedded in the document (original JPG/PNG bytes, base64 in
//! the saved file) and their conversion into GPU texture layers.

use base64::Engine;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Side of every GPU texture layer (images are resampled to it).
pub const LAYER: u32 = 512;
/// Maximum number of distinct textures on the GPU at once.
pub const MAX_LAYERS: usize = 96;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextureData {
    pub name: String,
    /// Encoded image (JPG/PNG).
    #[serde(serialize_with = "ser_b64", deserialize_with = "de_b64")]
    pub bytes: Vec<u8>,
    pub w: u32,
    pub h: u32,
}

fn ser_b64<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(v))
}

fn de_b64<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
    let s = String::deserialize(d)?;
    base64::engine::general_purpose::STANDARD.decode(s).map_err(serde::de::Error::custom)
}

impl TextureData {
    /// Validate and wrap an encoded image.
    pub fn from_bytes(name: &str, bytes: Vec<u8>) -> Result<TextureData, String> {
        let img = image::load_from_memory(&bytes).map_err(|e| format!("imagem inválida: {e}"))?;
        Ok(TextureData { name: name.to_string(), w: img.width(), h: img.height(), bytes })
    }

    /// Wrap raw RGBA pixels (re-encoded as PNG).
    pub fn from_rgba(name: &str, w: u32, h: u32, rgba: Vec<u8>) -> Result<TextureData, String> {
        let img = image::RgbaImage::from_raw(w, h, rgba).ok_or("tamanho de imagem inválido")?;
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
        Ok(TextureData { name: name.to_string(), w, h, bytes: out.into_inner() })
    }

    pub fn decode(&self) -> Option<image::RgbaImage> {
        image::load_from_memory(&self.bytes).ok().map(|i| i.to_rgba8())
    }

    /// Average color (used for swatches and untextured styles).
    pub fn average(&self) -> [u8; 3] {
        let Some(img) = self.decode() else { return [200, 200, 200] };
        let small = image::imageops::resize(&img, 16, 16, image::imageops::FilterType::Triangle);
        let mut acc = [0u64; 3];
        for p in small.pixels() {
            for k in 0..3 {
                acc[k] += p[k] as u64;
            }
        }
        let n = (small.width() * small.height()).max(1) as u64;
        [(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8]
    }

    /// Mip chain of a LAYER x LAYER resampled copy (RGBA8).
    pub fn mip_chain(&self) -> Vec<Vec<u8>> {
        let base = match self.decode() {
            Some(img) => image::imageops::resize(&img, LAYER, LAYER, image::imageops::FilterType::Triangle),
            None => image::RgbaImage::from_pixel(LAYER, LAYER, image::Rgba([255, 0, 255, 255])),
        };
        let mut out = vec![base.clone().into_raw()];
        let mut cur = base;
        let mut s = LAYER;
        while s > 1 {
            s /= 2;
            cur = image::imageops::resize(&cur, s, s, image::imageops::FilterType::Triangle);
            out.push(cur.clone().into_raw());
        }
        out
    }

    /// Small thumbnail for the UI.
    pub fn thumbnail(&self, size: u32) -> Option<(u32, u32, Vec<u8>)> {
        let img = self.decode()?;
        let t = image::imageops::thumbnail(&img, size, size);
        Some((t.width(), t.height(), t.into_raw()))
    }
}

pub fn mip_levels() -> u32 {
    LAYER.ilog2() + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn png_roundtrip_and_mips() {
        let rgba: Vec<u8> = (0..8 * 8).flat_map(|i| [(i * 4) as u8, 100, 50, 255]).collect();
        let t = TextureData::from_rgba("t", 8, 8, rgba).unwrap();
        assert_eq!((t.w, t.h), (8, 8));
        let json = serde_json::to_string(&t).unwrap();
        let back: TextureData = serde_json::from_str(&json).unwrap();
        assert_eq!(back.bytes, t.bytes);
        let mips = t.mip_chain();
        assert_eq!(mips.len() as u32, mip_levels());
        assert_eq!(mips[0].len(), (LAYER * LAYER * 4) as usize);
        assert_eq!(mips.last().unwrap().len(), 4);
        let avg = t.average();
        assert!(avg[1] > 90 && avg[1] < 110);
    }
}
