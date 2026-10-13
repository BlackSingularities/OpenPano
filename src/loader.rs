//! Loading of source images (raster and RAW) plus EXIF metadata relevant to stitching.

use crate::imaging::{srgb8_lut, RgbImage};
use crate::raw;
use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

pub const RASTER_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "tif", "tiff", "webp", "bmp"];

pub fn is_supported(p: &Path) -> bool {
    let ext = p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    RASTER_EXTENSIONS.contains(&ext.as_str()) || raw::RAW_EXTENSIONS.contains(&ext.as_str())
}

#[derive(Clone, Debug, Default)]
pub struct Metadata {
    pub make: String,
    pub model: String,
    pub lens: String,
    pub focal_mm: Option<f64>,
    pub focal_35mm: Option<f64>,
    /// Sensor width in mm derived from focal plane resolution tags, if available.
    pub sensor_width_mm: Option<f64>,
    pub exposure: Option<f64>,
    pub fnumber: Option<f64>,
    pub iso: Option<u32>,
    pub datetime: String,
    pub orientation: u32,
}

impl Metadata {
    /// Focal length in pixels for an image of the given (oriented) dimensions.
    pub fn focal_pixels(&self, w: usize, h: usize) -> Option<f64> {
        let long = w.max(h) as f64;
        if let Some(f35) = self.focal_35mm.filter(|v| *v > 0.5) {
            // 35 mm equivalent refers to the long side of a 36x24 mm frame (diagonal-matched
            // for non-3:2 aspect ratios; long-side approximation is standard practice).
            let diag_px = ((w * w + h * h) as f64).sqrt();
            let _ = long;
            return Some(f35 * diag_px / 43.266_615);
        }
        if let (Some(f), Some(sw)) = (self.focal_mm, self.sensor_width_mm) {
            if f > 0.0 && sw > 1.0 {
                return Some(f / sw * long);
            }
        }
        None
    }

    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.model.is_empty() {
            parts.push(self.model.clone());
        }
        if let Some(f) = self.focal_mm {
            parts.push(format!("{:.0} mm", f));
        }
        if let Some(n) = self.fnumber {
            parts.push(format!("f/{:.1}", n));
        }
        if let Some(e) = self.exposure {
            if e < 1.0 && e > 0.0 {
                parts.push(format!("1/{:.0} s", 1.0 / e));
            } else {
                parts.push(format!("{:.1} s", e));
            }
        }
        if let Some(i) = self.iso {
            parts.push(format!("ISO {}", i));
        }
        parts.join(" · ")
    }
}

pub fn read_metadata(path: &Path) -> Metadata {
    let mut md = Metadata { orientation: 1, ..Default::default() };
    let Ok(file) = std::fs::File::open(path) else { return md };
    let mut reader = std::io::BufReader::new(file);
    let Ok(ex) = exif::Reader::new().read_from_container(&mut reader) else { return md };
    use exif::{In, Tag, Value};
    let rational = |tag: Tag| -> Option<f64> {
        let f = ex.get_field(tag, In::PRIMARY)?;
        match &f.value {
            Value::Rational(v) if !v.is_empty() => Some(v[0].to_f64()),
            Value::SRational(v) if !v.is_empty() => Some(v[0].to_f64()),
            Value::Short(v) if !v.is_empty() => Some(v[0] as f64),
            Value::Long(v) if !v.is_empty() => Some(v[0] as f64),
            _ => None,
        }
    };
    let text = |tag: Tag| -> String {
        ex.get_field(tag, In::PRIMARY)
            .map(|f| f.display_value().to_string().trim_matches('"').trim().to_string())
            .unwrap_or_default()
    };
    md.make = text(Tag::Make);
    md.model = text(Tag::Model);
    md.lens = text(Tag::LensModel);
    md.datetime = text(Tag::DateTimeOriginal);
    md.focal_mm = rational(Tag::FocalLength).filter(|v| *v > 0.0);
    md.focal_35mm = ex
        .get_field(Tag::FocalLengthIn35mmFilm, In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
        .map(|v| v as f64)
        .filter(|v| *v > 0.0);
    md.exposure = rational(Tag::ExposureTime);
    md.fnumber = rational(Tag::FNumber);
    md.iso = ex.get_field(Tag::PhotographicSensitivity, In::PRIMARY).and_then(|f| f.value.get_uint(0));
    md.orientation = ex
        .get_field(Tag::Orientation, In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
        .unwrap_or(1);
    // Sensor width from focal plane resolution
    if let (Some(xres), Some(unit)) = (
        rational(Tag::FocalPlaneXResolution),
        ex.get_field(Tag::FocalPlaneResolutionUnit, In::PRIMARY).and_then(|f| f.value.get_uint(0)),
    ) {
        let px_w = ex
            .get_field(Tag::PixelXDimension, In::PRIMARY)
            .and_then(|f| f.value.get_uint(0))
            .map(|v| v as f64);
        let unit_mm = match unit {
            2 => 25.4,
            3 => 10.0,
            4 => 1.0,
            5 => 0.001,
            _ => 0.0,
        };
        if let Some(pw) = px_w {
            if xres > 0.0 && unit_mm > 0.0 {
                let sw = pw / xres * unit_mm;
                if (2.0..80.0).contains(&sw) {
                    md.sensor_width_mm = Some(sw);
                }
            }
        }
    }
    md
}

/// Fully loaded source image (linear light, orientation applied).
pub struct Loaded {
    pub image: RgbImage,
    pub meta: Metadata,
    pub is_raw: bool,
    /// Full-resolution size of the oriented image (differs from `image` for half-size RAW loads).
    pub full_w: usize,
    pub full_h: usize,
}

/// Load an image. `half` requests a fast reduced-resolution RAW decode (used for alignment and
/// previews); raster images are always loaded at full resolution.
pub fn load(path: &Path, half: bool) -> Result<Loaded> {
    let mut meta = read_metadata(path);
    if raw::is_raw_path(path) {
        let (img, info) = raw::load_raw(path, half).with_context(|| format!("{}", path.display()))?;
        if meta.make.is_empty() {
            meta.make = info.make;
        }
        if meta.model.is_empty() {
            meta.model = info.model;
        }
        let orient = if info.orientation > 1 { info.orientation } else { meta.orientation };
        meta.orientation = orient;
        let image = img.rotate_orientation(orient);
        let (fw, fh) = if (5..=8).contains(&orient) { (info.full_h, info.full_w) } else { (info.full_w, info.full_h) };
        let (fw, fh) = if half { (fw, fh) } else { (image.w, image.h) };
        return Ok(Loaded { full_w: fw, full_h: fh, image, meta, is_raw: true });
    }
    let dynimg = image::ImageReader::open(path)
        .with_context(|| format!("{}", path.display()))?
        .with_guessed_format()?
        .decode()
        .map_err(|e| anyhow!("{}: {}", path.display(), e))?;
    let image = dynamic_to_linear(&dynimg).rotate_orientation(meta.orientation);
    Ok(Loaded { full_w: image.w, full_h: image.h, image, meta, is_raw: false })
}

pub fn dynamic_to_linear(img: &image::DynamicImage) -> RgbImage {
    use image::DynamicImage as D;
    let w = img.width() as usize;
    let h = img.height() as usize;
    let mut out = RgbImage::new(w, h);
    match img {
        D::ImageRgb8(_) | D::ImageRgba8(_) | D::ImageLuma8(_) | D::ImageLumaA8(_) => {
            let lut = srgb8_lut();
            let rgb = img.to_rgb8();
            for (o, p) in out.data.iter_mut().zip(rgb.pixels()) {
                *o = [lut[p[0] as usize], lut[p[1] as usize], lut[p[2] as usize]];
            }
        }
        D::ImageRgb32F(_) | D::ImageRgba32F(_) => {
            // Float images are assumed to be linear already.
            let rgb = img.to_rgb32f();
            for (o, p) in out.data.iter_mut().zip(rgb.pixels()) {
                *o = [p[0].max(0.0), p[1].max(0.0), p[2].max(0.0)];
            }
        }
        _ => {
            let rgb = img.to_rgb16();
            let lut: Vec<f32> = (0..65536).map(|i| crate::imaging::srgb_to_linear(i as f32 / 65535.0)).collect();
            for (o, p) in out.data.iter_mut().zip(rgb.pixels()) {
                *o = [lut[p[0] as usize], lut[p[1] as usize], lut[p[2] as usize]];
            }
        }
    }
    out
}

/// Expand dropped/selected paths: directories are scanned (non-recursively) for supported files.
pub fn expand_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        if p.is_dir() {
            if let Ok(rd) = std::fs::read_dir(p) {
                let mut v: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_file() && is_supported(p)).collect();
                v.sort();
                out.extend(v);
            }
        } else if is_supported(p) {
            out.push(p.clone());
        }
    }
    out
}
