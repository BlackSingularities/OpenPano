//! High-level pipeline shared by the GUI and the command line interface.

use crate::imaging::RgbImage;
use crate::loader::{self, Metadata};
use crate::output::{self, OutputFormat, OutputSettings, SourceInfo};
use crate::stitch::align::{self, AlignImage, AlignSettings, Alignment};
use crate::stitch::features;
use crate::stitch::photometric::{self, PhotoSettings, Photometric};
use crate::stitch::render::{self, RenderOutput, RenderSettings, Source};
use crate::stitch::{Cancel, Progress, StitchError};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Working-resolution copy of a source image used for alignment and previews.
pub struct WorkImage {
    pub path: PathBuf,
    pub meta: Metadata,
    pub is_raw: bool,
    pub full_w: usize,
    pub full_h: usize,
    pub img: RgbImage,
    /// work / full scale
    pub scale: f64,
    pub thumb: (usize, usize, Vec<u8>),
}

pub const WORK_MEGAPIXELS: f64 = 1.6;

pub fn load_work(path: &Path) -> Result<WorkImage, StitchError> {
    let l = loader::load(path, true)?;
    let (img, s) = l.image.scaled_to_megapixels(WORK_MEGAPIXELS);
    // scale relative to the full-resolution image (RAW half loads are smaller than full)
    let scale = img.w as f64 / l.full_w as f64;
    let _ = s;
    let tw = 160usize;
    let th = ((img.h as f64 / img.w as f64) * tw as f64).round().max(1.0) as usize;
    let t = img.resized(tw, th);
    let conv = crate::imaging::ToSrgb8::new();
    let mut rgba = Vec::with_capacity(tw * th * 4);
    for p in &t.data {
        rgba.extend_from_slice(&[conv.conv(p[0]), conv.conv(p[1]), conv.conv(p[2]), 255]);
    }
    Ok(WorkImage {
        path: path.to_path_buf(),
        meta: l.meta,
        is_raw: l.is_raw,
        full_w: l.full_w,
        full_h: l.full_h,
        img,
        scale,
        thumb: (tw, th, rgba),
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PhotoOptions {
    pub exposure: bool,
    pub color: bool,
    pub vignetting: bool,
}

impl Default for PhotoOptions {
    fn default() -> Self {
        Self { exposure: true, color: true, vignetting: true }
    }
}

pub struct Aligned {
    pub alignment: Alignment,
    pub photo: Photometric,
}

pub fn run_align(works: &[Arc<WorkImage>], st: &AlignSettings, ph: &PhotoOptions, manual: &[align::ManualPoint], progress: &Progress, cancel: &Cancel) -> Result<Aligned, StitchError> {
    progress.set_stage(crate::i18n::tr(crate::i18n::Key::StageFeatures));
    let done = std::sync::atomic::AtomicUsize::new(0);
    let n = works.len();
    let aimgs: Vec<AlignImage> = works
        .par_iter()
        .map(|w| {
            let lum = w.img.luminance();
            let g = features::gray_from_linear(&lum, w.img.w, w.img.h);
            let f = features::detect(&g, st.max_features);
            let c = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            progress.set_fraction(c as f32 / n as f32);
            let exif_focal = w.meta.focal_pixels(w.full_w, w.full_h);
            let focal_key = format!(
                "{}|{}|{}x{}|{:?}|{:?}",
                w.meta.model,
                w.meta.lens,
                w.full_w.max(w.full_h),
                w.full_w.min(w.full_h),
                w.meta.focal_mm.map(|v| (v * 10.0).round() as i64),
                w.meta.focal_35mm.map(|v| v.round() as i64)
            );
            AlignImage {
                features: f,
                scale: w.scale,
                work_w: w.img.w,
                work_h: w.img.h,
                full_w: w.full_w,
                full_h: w.full_h,
                exif_focal,
                focal_key,
            }
        })
        .collect();
    cancel.check()?;
    progress.set_stage(crate::i18n::tr(crate::i18n::Key::StageMatching));
    let alignment = align::align(&aimgs, st, manual, progress, cancel)?;
    progress.set_stage(crate::i18n::tr(crate::i18n::Key::StagePhotometric));
    let work: Vec<RgbImage> = works.iter().map(|w| w.img.clone()).collect();
    let scales: Vec<f64> = works.iter().map(|w| w.scale).collect();
    let photo = photometric::estimate(
        &alignment,
        &work,
        &scales,
        &PhotoSettings { exposure: ph.exposure, color: ph.color, vignetting: ph.vignetting },
    );
    Ok(Aligned { alignment, photo })
}

/// Recompute photometric parameters without re-aligning (when options change).
pub fn recompute_photo(works: &[Arc<WorkImage>], al: &Alignment, ph: &PhotoOptions) -> Photometric {
    let work: Vec<RgbImage> = works.iter().map(|w| w.img.clone()).collect();
    let scales: Vec<f64> = works.iter().map(|w| w.scale).collect();
    photometric::estimate(al, &work, &scales, &PhotoSettings { exposure: ph.exposure, color: ph.color, vignetting: ph.vignetting })
}

/// Render the blended panorama from the working images into an equirectangular map of about
/// `target_mp` megapixels (identity view), used for the real-time preview.
pub fn build_sphere(
    works: &[Arc<WorkImage>],
    al: &Aligned,
    rs: &RenderSettings,
    target_mp: f64,
    progress: &Progress,
    cancel: &Cancel,
) -> Result<crate::preview::Sphere, StitchError> {
    use crate::stitch::geometry::Projection;
    let mut srs = rs.clone();
    srs.projection = Projection::Equirectangular;
    srs.auto_projection = false;
    srs.full_sphere = false;
    srs.yaw = 0.0;
    srs.pitch = 0.0;
    srs.roll = 0.0;
    srs.flip_h = false;
    srs.flip_v = false;
    srs.crop = render::CropMode::None;
    srs.bicubic = false;
    let base = render::layout(&al.alignment, &srs, Some(100.0))?;
    let area = (base.w * base.h) as f64 / 1e4;
    let s = (target_mp * 1e6 / area.max(1e-9)).sqrt();
    let native = render::native_scale(&al.alignment) * works.iter().map(|w| w.scale).fold(f64::MAX, f64::min);
    let s = s.min(native.max(1.0));
    let canvas = render::layout(&al.alignment, &srs, Some(s))?;
    let out = render::render(
        &al.alignment,
        &al.photo,
        &srs,
        canvas,
        |i| Ok(Source { img: works[i].img.clone(), scale: works[i].scale }),
        Some(&|i: usize| Some(Source { img: works[i].img.clone(), scale: works[i].scale })),
        progress,
        cancel,
    )?;
    Ok(crate::preview::Sphere::from_render(&out))
}

pub fn estimate_canvas(al: &Aligned, rs: &RenderSettings) -> Option<(usize, usize)> {
    render::layout(&al.alignment, rs, None).ok().map(|c| (c.w, c.h))
}

pub fn render_full(
    works: &[Arc<WorkImage>],
    al: &Aligned,
    rs: &RenderSettings,
    progress: &Progress,
    cancel: &Cancel,
) -> Result<RenderOutput, StitchError> {
    let canvas = render::layout(&al.alignment, rs, None)?;
    progress.set_stage(format!(
        "{} ({}×{})",
        crate::i18n::tr(crate::i18n::Key::StageRendering),
        canvas.w,
        canvas.h
    ));
    render::render(
        &al.alignment,
        &al.photo,
        rs,
        canvas,
        |i| {
            let l = loader::load(&works[i].path, false)?;
            let scale = l.image.w as f64 / works[i].full_w as f64;
            Ok(Source { img: l.image, scale })
        },
        Some(&|i: usize| Some(Source { img: works[i].img.clone(), scale: works[i].scale })),
        progress,
        cancel,
    )
}

pub fn source_info(works: &[Arc<WorkImage>]) -> SourceInfo {
    let first = works.first();
    SourceInfo {
        make: first.map(|w| w.meta.make.clone()).unwrap_or_default(),
        model: first.map(|w| w.meta.model.clone()).unwrap_or_default(),
    }
}

pub fn save_output(path: &Path, out: &RenderOutput, works: &[Arc<WorkImage>], os: &OutputSettings) -> Result<OutputFormat, StitchError> {
    let any_raw = works.iter().any(|w| w.is_raw);
    let fmt = os.format.resolve(any_raw);
    output::save(path, out, fmt, os, &source_info(works))?;
    Ok(fmt)
}
