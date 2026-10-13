//! Panorama rendering: canvas layout for a projection, seam selection, multi-band (Laplacian
//! pyramid) or feather blending, and automatic cropping.

use super::align::Alignment;
use super::geometry::{ypr_rotation, LensType, Projection, Rot, V3};
use super::photometric::Photometric;
use super::{Cancel, Progress, StitchError};
use crate::imaging::RgbImage;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::f64::consts::{FRAC_PI_2, PI};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlendMode {
    Multiband,
    Feather,
    Seams,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum CropMode {
    Auto,
    None,
    Manual,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenderSettings {
    pub projection: Projection,
    pub blend: BlendMode,
    /// Number of multi-band levels; 0 = automatic.
    pub bands: usize,
    pub crop: CropMode,
    /// Manual crop margins as fractions of the canvas (left, top, right, bottom).
    pub manual_crop: [f32; 4],
    /// Force a complete 360°×180° equirectangular canvas.
    pub full_sphere: bool,
    /// Output scale relative to native resolution.
    pub scale: f64,
    /// Upper bound for output size, megapixels.
    pub max_megapixels: f64,
    pub yaw: f64,
    pub pitch: f64,
    pub roll: f64,
    /// Field of view override for zoomable projections (degrees, 0 = auto).
    pub circular_fisheye: bool,
    pub bicubic: bool,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            projection: Projection::Cylindrical,
            blend: BlendMode::Multiband,
            bands: 0,
            crop: CropMode::Auto,
            manual_crop: [0.0; 4],
            full_sphere: false,
            scale: 1.0,
            max_megapixels: 300.0,
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            circular_fisheye: false,
            bicubic: true,
        }
    }
}

/// Description of the output canvas.
#[derive(Clone, Debug)]
pub struct Canvas {
    pub proj: Projection,
    pub view: Rot,
    pub u0: f64,
    pub v0: f64,
    /// pixels per plane unit
    pub scale: f64,
    pub w: usize,
    pub h: usize,
    /// Per-image bounding boxes in canvas pixels (x0, y0, x1, y1), None if not visible.
    pub boxes: Vec<Option<(usize, usize, usize, usize)>>,
}

impl Canvas {
    #[inline]
    pub fn pixel_dir(&self, px: f64, py: f64) -> Option<V3> {
        let u = self.u0 + (px + 0.5) / self.scale;
        let v = self.v0 + (py + 0.5) / self.scale;
        self.proj.inverse(u, v).map(|d| self.view.inverse() * d)
    }

    pub fn megapixels(&self) -> f64 {
        (self.w * self.h) as f64 / 1e6
    }
}

pub fn native_scale(al: &Alignment) -> f64 {
    let mut f: Vec<f64> = al.cameras.iter().flatten().map(|c| c.f).collect();
    if f.is_empty() {
        return 1000.0;
    }
    f.sort_by(|a, b| a.partial_cmp(b).unwrap());
    f[f.len() / 2]
}

/// Compute the canvas for the given alignment and settings. `px_scale` overrides the pixel scale
/// (pixels per radian at the centre); when None the native resolution × settings.scale is used,
/// limited by max_megapixels.
pub fn layout(al: &Alignment, st: &RenderSettings, px_scale: Option<f64>) -> Result<Canvas, StitchError> {
    let view = ypr_rotation(st.yaw, st.pitch, st.roll).inverse();
    let proj = st.projection;
    let (lu0, lu1, lv0, lv1) = proj.limits();
    let wrap = proj.wraps();
    let mut u0 = f64::MAX;
    let mut u1 = f64::MIN;
    let mut v0 = f64::MAX;
    let mut v1 = f64::MIN;
    let mut img_bounds: Vec<Option<(f64, f64, f64, f64)>> = Vec::new();
    for cam in al.cameras.iter() {
        let Some(cam) = cam else {
            img_bounds.push(None);
            continue;
        };
        let samples = boundary_samples(cam.w, cam.h, 48, 12);
        let mut bu0 = f64::MAX;
        let mut bu1 = f64::MIN;
        let mut bv0 = f64::MAX;
        let mut bv1 = f64::MIN;
        let mut any = false;
        let mut us: Vec<f64> = Vec::new();
        for (x, y) in samples {
            if st.circular_fisheye && al.lens.is_fisheye() && !in_circle(x, y, cam.w, cam.h) {
                continue;
            }
            let d = view * cam.ray_world(al.lens, x, y);
            if let Some((u, v)) = proj.forward(&d) {
                let u = u.clamp(lu0, lu1);
                let v = v.clamp(lv0, lv1);
                bu0 = bu0.min(u);
                bu1 = bu1.max(u);
                bv0 = bv0.min(v);
                bv1 = bv1.max(v);
                us.push(u);
                any = true;
            }
        }
        if !any {
            img_bounds.push(None);
            continue;
        }
        if wrap && bu1 - bu0 > PI {
            // The image straddles the ±180° seam (or contains a pole): it spans the full width.
            bu0 = lu0;
            bu1 = lu1;
            // Check whether the forward camera axis is near a pole; extend vertical bounds then.
            let z = view * (cam.rot * V3::new(0.0, 0.0, 1.0));
            if let Some((_, vz)) = proj.forward(&z) {
                if vz < 0.0 {
                    bv0 = lv0.max(bv0.min(lv0));
                } else {
                    bv1 = lv1.min(bv1.max(lv1));
                }
            }
        }
        u0 = u0.min(bu0);
        u1 = u1.max(bu1);
        v0 = v0.min(bv0);
        v1 = v1.max(bv1);
        img_bounds.push(Some((bu0, bu1, bv0, bv1)));
    }
    if u0 >= u1 || v0 >= v1 {
        return Err(StitchError::EmptyCanvas);
    }
    if st.full_sphere && proj == Projection::Equirectangular {
        u0 = -PI;
        u1 = PI;
        v0 = -FRAC_PI_2;
        v1 = FRAC_PI_2;
    }
    let scale = match px_scale {
        Some(s) => s,
        None => {
            let mut s = native_scale(al) * st.scale;
            let mp = (u1 - u0) * (v1 - v0) * s * s / 1e6;
            if mp > st.max_megapixels {
                s *= (st.max_megapixels / mp).sqrt();
            }
            s
        }
    };
    let w = (((u1 - u0) * scale).ceil() as usize).max(1);
    let h = (((v1 - v0) * scale).ceil() as usize).max(1);
    if w > 60000 || h > 60000 {
        return Err(StitchError::TooLarge);
    }
    let boxes = img_bounds
        .iter()
        .map(|b| {
            b.map(|(a0, a1, b0, b1)| {
                let x0 = (((a0 - u0) * scale).floor() as isize - 2).max(0) as usize;
                let x1 = ((((a1 - u0) * scale).ceil() as isize + 2).max(0) as usize).min(w);
                let y0 = (((b0 - v0) * scale).floor() as isize - 2).max(0) as usize;
                let y1 = ((((b1 - v0) * scale).ceil() as isize + 2).max(0) as usize).min(h);
                (x0, y0, x1.max(x0 + 1).min(w), y1.max(y0 + 1).min(h))
            })
        })
        .collect();
    Ok(Canvas { proj, view, u0, v0, scale, w, h, boxes })
}

fn boundary_samples(w: usize, h: usize, edge: usize, grid: usize) -> Vec<(f64, f64)> {
    let mut v = Vec::new();
    let (wf, hf) = ((w - 1) as f64, (h - 1) as f64);
    for k in 0..=edge {
        let t = k as f64 / edge as f64;
        v.push((t * wf, 0.0));
        v.push((t * wf, hf));
        v.push((0.0, t * hf));
        v.push((wf, t * hf));
    }
    for gy in 1..grid {
        for gx in 1..grid {
            v.push((gx as f64 / grid as f64 * wf, gy as f64 / grid as f64 * hf));
        }
    }
    v
}

#[inline]
fn in_circle(x: f64, y: f64, w: usize, h: usize) -> bool {
    let cx = (w as f64 - 1.0) / 2.0;
    let cy = (h as f64 - 1.0) / 2.0;
    let r = (w.min(h) as f64) / 2.0 * 0.985;
    (x - cx).powi(2) + (y - cy).powi(2) < r * r
}

/// A source image for rendering together with its scale relative to the camera's full resolution.
pub struct Source {
    pub img: RgbImage,
    pub scale: f64,
}

pub struct RenderOutput {
    pub image: RgbImage,
    /// 255 where covered by at least one image
    pub alpha: Vec<u8>,
    pub canvas: Canvas,
    /// crop rectangle within the canvas (x, y, w, h)
    pub crop: (usize, usize, usize, usize),
}

struct Mapper<'a> {
    cam: &'a super::align::Camera,
    lens: LensType,
    inv_rot: Rot,
    cx: f64,
    cy: f64,
    hw: f64,
    hh: f64,
    circular: bool,
}

impl<'a> Mapper<'a> {
    fn new(cam: &'a super::align::Camera, lens: LensType, circular: bool) -> Self {
        Self {
            cam,
            lens,
            inv_rot: cam.rot.inverse(),
            cx: (cam.w as f64 - 1.0) / 2.0,
            cy: (cam.h as f64 - 1.0) / 2.0,
            hw: cam.w as f64 / 2.0,
            hh: cam.h as f64 / 2.0,
            circular: circular && lens.is_fisheye(),
        }
    }

    /// World direction -> centred full-res image coordinates and seam weight.
    #[inline]
    fn map(&self, d: &V3) -> Option<(f64, f64, f32)> {
        let c = self.inv_rot * d;
        let (x, y) = self.lens.project(&c, self.cam.f)?;
        let xn = x / self.hw;
        let yn = y / self.hh;
        if xn.abs() >= 1.0 || yn.abs() >= 1.0 {
            return None;
        }
        let w = if self.circular {
            let r = self.hw.min(self.hh) * 0.985;
            let rr = (x * x + y * y).sqrt() / r;
            if rr >= 1.0 {
                return None;
            }
            1.0 - rr
        } else {
            (1.0 - xn.abs()) * (1.0 - yn.abs())
        };
        Some((x, y, w as f32))
    }
}

const NONE: u16 = u16::MAX;

pub fn render(
    al: &Alignment,
    photo: &Photometric,
    st: &RenderSettings,
    canvas: Canvas,
    mut load: impl FnMut(usize) -> Result<Source, StitchError>,
    progress: &Progress,
    cancel: &Cancel,
) -> Result<RenderOutput, StitchError> {
    let (cw, ch) = (canvas.w, canvas.h);
    let order: Vec<usize> = (0..al.cameras.len()).filter(|&i| al.cameras[i].is_some() && canvas.boxes[i].is_some()).collect();
    if order.is_empty() {
        return Err(StitchError::EmptyCanvas);
    }
    // ---------- pass 1: seam labels ----------
    let mut label = vec![NONE; cw * ch];
    let mut best = vec![0f32; cw * ch];
    for (k, &i) in order.iter().enumerate() {
        cancel.check()?;
        let cam = al.cameras[i].as_ref().unwrap();
        let mapper = Mapper::new(cam, al.lens, st.circular_fisheye);
        let (x0, y0, x1, y1) = canvas.boxes[i].unwrap();
        let lrows = &mut label[y0 * cw..y1 * cw];
        let brows = &mut best[y0 * cw..y1 * cw];
        lrows.par_chunks_mut(cw).zip(brows.par_chunks_mut(cw)).enumerate().for_each(|(ry, (lr, br))| {
            let y = (y0 + ry) as f64;
            for x in x0..x1 {
                let Some(d) = canvas.pixel_dir(x as f64, y) else { continue };
                if let Some((_, _, w)) = mapper.map(&d) {
                    if w > br[x] {
                        br[x] = w;
                        lr[x] = i as u16;
                    }
                }
            }
        });
        progress.set_fraction(0.1 * (k + 1) as f32 / order.len() as f32);
    }
    drop(best);

    // ---------- pass 2: warp + blend ----------
    let levels = match st.blend {
        BlendMode::Multiband => {
            let auto = ((cw.min(ch) as f64 / 8.0).log2().floor() as isize).clamp(1, 7) as usize;
            if st.bands == 0 {
                auto
            } else {
                st.bands.min(auto.max(1) + 2).clamp(1, 10)
            }
        }
        _ => 0,
    };
    let mut acc = Accumulator::new(cw, ch, levels);
    for (k, &i) in order.iter().enumerate() {
        cancel.check()?;
        let base = 0.1 + 0.8 * k as f32 / order.len() as f32;
        progress.set_fraction(base);
        let src = load(i)?;
        cancel.check()?;
        let cam = al.cameras[i].as_ref().unwrap();
        let mapper = Mapper::new(cam, al.lens, st.circular_fisheye);
        let (bx0, by0, bx1, by1) = canvas.boxes[i].unwrap();
        // tile with margin, aligned to 2^levels
        let align = 1usize << levels;
        let margin = if levels > 0 { 3 * align } else { 0 };
        let tx0 = (bx0.saturating_sub(margin) / align) * align;
        let ty0 = (by0.saturating_sub(margin) / align) * align;
        let tx1 = (bx1 + margin).min(cw);
        let ty1 = (by1 + margin).min(ch);
        let tw = tx1 - tx0;
        let th = ty1 - ty0;
        let gains = photo.gains[i];
        let hd2 = ((cam.w * cam.w + cam.h * cam.h) as f64) / 4.0;
        let s = src.scale;
        let (sw, sh) = (src.img.w as f64, src.img.h as f64);
        let mut tile = vec![[0f32; 3]; tw * th];
        let mut valid = vec![0f32; tw * th];
        let mut band = vec![0f32; tw * th];
        let mut feather = vec![0f32; tw * th];
        tile.par_chunks_mut(tw)
            .zip(valid.par_chunks_mut(tw))
            .zip(band.par_chunks_mut(tw))
            .zip(feather.par_chunks_mut(tw))
            .enumerate()
            .for_each(|(ry, (((trow, vrow), brow), frow))| {
                let y = ty0 + ry;
                for rx in 0..tw {
                    let x = tx0 + rx;
                    let Some(d) = canvas.pixel_dir(x as f64, y as f64) else { continue };
                    let Some((ix, iy, w)) = mapper.map(&d) else { continue };
                    let sx = ix * s + (sw - 1.0) / 2.0;
                    let sy = iy * s + (sh - 1.0) / 2.0;
                    let px = if st.bicubic { src.img.sample_cubic(sx as f32, sy as f32) } else { src.img.sample(sx as f32, sy as f32) };
                    let Some(p) = px else { continue };
                    let vf = photo.vig_factor((ix * ix + iy * iy) / hd2);
                    trow[rx] = [p[0] * gains[0] * vf, p[1] * gains[1] * vf, p[2] * gains[2] * vf];
                    vrow[rx] = 1.0;
                    frow[rx] = w;
                    if label[y * cw + x] == i as u16 {
                        brow[rx] = 1.0;
                    }
                }
            });
        drop(src);
        match st.blend {
            BlendMode::Feather => acc.add_feather(&tile, &feather, tx0, ty0, tw, th),
            BlendMode::Seams => acc.add_feather(&tile, &band, tx0, ty0, tw, th),
            BlendMode::Multiband => {
                fill_push_pull(&mut tile, &valid, tw, th);
                acc.add_multiband(tile, band, tx0, ty0, tw, th);
            }
        }
        let _ = feather;
    }
    cancel.check()?;
    progress.set_fraction(0.92);
    let mut image = acc.finish();
    let alpha: Vec<u8> = label.par_iter().map(|&l| if l == NONE { 0 } else { 255 }).collect();
    for (p, a) in image.data.iter_mut().zip(alpha.iter()) {
        if *a == 0 {
            *p = [0.0; 3];
        }
    }
    // ---------- crop ----------
    let full = (0, 0, cw, ch);
    let crop = match st.crop {
        CropMode::None => full,
        CropMode::Auto => auto_crop(&alpha, cw, ch).unwrap_or(full),
        CropMode::Manual => {
            let [l, t, r, b] = st.manual_crop;
            let x = (l.clamp(0.0, 0.95) * cw as f32) as usize;
            let y = (t.clamp(0.0, 0.95) * ch as f32) as usize;
            let x1 = ((1.0 - r.clamp(0.0, 0.95)) * cw as f32) as usize;
            let y1 = ((1.0 - b.clamp(0.0, 0.95)) * ch as f32) as usize;
            if x1 > x + 8 && y1 > y + 8 {
                (x, y, x1 - x, y1 - y)
            } else {
                full
            }
        }
    };
    let (image, alpha) = if crop != full {
        let (x, y, w, h) = crop;
        let mut out = RgbImage::new(w, h);
        let mut a = vec![0u8; w * h];
        for r in 0..h {
            out.data[r * w..(r + 1) * w].copy_from_slice(&image.data[(y + r) * cw + x..(y + r) * cw + x + w]);
            a[r * w..(r + 1) * w].copy_from_slice(&alpha[(y + r) * cw + x..(y + r) * cw + x + w]);
        }
        (out, a)
    } else {
        (image, alpha)
    };
    progress.set_fraction(1.0);
    Ok(RenderOutput { image, alpha, canvas, crop })
}

/// Largest axis-aligned rectangle fully covered (computed on a reduced mask).
pub fn auto_crop(alpha: &[u8], w: usize, h: usize) -> Option<(usize, usize, usize, usize)> {
    let f = ((w.max(h) as f64) / 1200.0).max(1.0);
    let sw = ((w as f64) / f).floor().max(1.0) as usize;
    let sh = ((h as f64) / f).floor().max(1.0) as usize;
    // a reduced cell is covered only if all of its source pixels are covered
    let mut cov = vec![true; sw * sh];
    cov.par_chunks_mut(sw).enumerate().for_each(|(cy, row)| {
        let y0 = (cy as f64 * f) as usize;
        let y1 = (((cy + 1) as f64 * f).ceil() as usize).min(h);
        for (cx, c) in row.iter_mut().enumerate() {
            let x0 = (cx as f64 * f) as usize;
            let x1 = (((cx + 1) as f64 * f).ceil() as usize).min(w);
            'o: for y in y0..y1 {
                for x in x0..x1 {
                    if alpha[y * w + x] == 0 {
                        *c = false;
                        break 'o;
                    }
                }
            }
        }
    });
    let mut heights = vec![0usize; sw];
    let mut best = (0usize, 0usize, 0usize, 0usize, 0usize); // area, x, y, w, h
    for y in 0..sh {
        for x in 0..sw {
            heights[x] = if cov[y * sw + x] { heights[x] + 1 } else { 0 };
        }
        // largest rectangle in histogram
        let mut stack: Vec<usize> = Vec::new();
        for x in 0..=sw {
            let cur = if x < sw { heights[x] } else { 0 };
            while let Some(&top) = stack.last() {
                if heights[top] <= cur {
                    break;
                }
                stack.pop();
                let hgt = heights[top];
                let left = stack.last().map(|&l| l + 1).unwrap_or(0);
                let wid = x - left;
                if hgt * wid > best.0 {
                    best = (hgt * wid, left, y + 1 - hgt, wid, hgt);
                }
            }
            stack.push(x);
        }
    }
    if best.0 == 0 {
        return None;
    }
    let (_, x, y, bw, bh) = best;
    let px = ((x as f64) * f).ceil() as usize;
    let py = ((y as f64) * f).ceil() as usize;
    let pw = (((bw as f64) * f).floor() as usize).min(w - px);
    let ph = (((bh as f64) * f).floor() as usize).min(h - py);
    if pw < 8 || ph < 8 {
        return None;
    }
    Some((px, py, pw, ph))
}

// ---------------------------------------------------------------------------------------------
// Pyramid helpers

fn reduce3(src: &[[f32; 3]], w: usize, h: usize) -> (Vec<[f32; 3]>, usize, usize) {
    let nw = w.div_ceil(2).max(1);
    let nh = h.div_ceil(2).max(1);
    const K: [f32; 5] = [1.0 / 16.0, 4.0 / 16.0, 6.0 / 16.0, 4.0 / 16.0, 1.0 / 16.0];
    // horizontal
    let mut tmp = vec![[0f32; 3]; nw * h];
    tmp.par_chunks_mut(nw).enumerate().for_each(|(y, row)| {
        let s = &src[y * w..(y + 1) * w];
        for (x, o) in row.iter_mut().enumerate() {
            let cx = 2 * x as isize;
            let mut a = [0f32; 3];
            for (k, kv) in K.iter().enumerate() {
                let xx = (cx + k as isize - 2).clamp(0, w as isize - 1) as usize;
                let p = s[xx];
                a[0] += p[0] * kv;
                a[1] += p[1] * kv;
                a[2] += p[2] * kv;
            }
            *o = a;
        }
    });
    let mut out = vec![[0f32; 3]; nw * nh];
    out.par_chunks_mut(nw).enumerate().for_each(|(y, row)| {
        let cy = 2 * y as isize;
        for (x, o) in row.iter_mut().enumerate() {
            let mut a = [0f32; 3];
            for (k, kv) in K.iter().enumerate() {
                let yy = (cy + k as isize - 2).clamp(0, h as isize - 1) as usize;
                let p = tmp[yy * nw + x];
                a[0] += p[0] * kv;
                a[1] += p[1] * kv;
                a[2] += p[2] * kv;
            }
            *o = a;
        }
    });
    (out, nw, nh)
}

fn reduce1(src: &[f32], w: usize, h: usize) -> (Vec<f32>, usize, usize) {
    let nw = w.div_ceil(2).max(1);
    let nh = h.div_ceil(2).max(1);
    const K: [f32; 5] = [1.0 / 16.0, 4.0 / 16.0, 6.0 / 16.0, 4.0 / 16.0, 1.0 / 16.0];
    let mut tmp = vec![0f32; nw * h];
    tmp.par_chunks_mut(nw).enumerate().for_each(|(y, row)| {
        let s = &src[y * w..(y + 1) * w];
        for (x, o) in row.iter_mut().enumerate() {
            let cx = 2 * x as isize;
            let mut a = 0f32;
            for (k, kv) in K.iter().enumerate() {
                let xx = (cx + k as isize - 2).clamp(0, w as isize - 1) as usize;
                a += s[xx] * kv;
            }
            *o = a;
        }
    });
    let mut out = vec![0f32; nw * nh];
    out.par_chunks_mut(nw).enumerate().for_each(|(y, row)| {
        let cy = 2 * y as isize;
        for (x, o) in row.iter_mut().enumerate() {
            let mut a = 0f32;
            for (k, kv) in K.iter().enumerate() {
                let yy = (cy + k as isize - 2).clamp(0, h as isize - 1) as usize;
                a += tmp[yy * nw + x] * kv;
            }
            *o = a;
        }
    });
    (out, nw, nh)
}

/// Expand (w,h) image to (tw,th) using the Burt-Adelson interpolation kernel.
fn expand3(src: &[[f32; 3]], w: usize, h: usize, tw: usize, th: usize) -> Vec<[f32; 3]> {
    let get = |v: &[[f32; 3]], stride: usize, x: isize, y: usize, maxx: usize| -> [f32; 3] { v[y * stride + x.clamp(0, maxx as isize - 1) as usize] };
    let mut tmp = vec![[0f32; 3]; tw * h];
    tmp.par_chunks_mut(tw).enumerate().for_each(|(y, row)| {
        for (x, o) in row.iter_mut().enumerate() {
            let k = (x / 2) as isize;
            *o = if x % 2 == 0 {
                let a = get(src, w, k - 1, y, w);
                let b = get(src, w, k, y, w);
                let c = get(src, w, k + 1, y, w);
                [(a[0] + 6.0 * b[0] + c[0]) / 8.0, (a[1] + 6.0 * b[1] + c[1]) / 8.0, (a[2] + 6.0 * b[2] + c[2]) / 8.0]
            } else {
                let b = get(src, w, k, y, w);
                let c = get(src, w, k + 1, y, w);
                [(b[0] + c[0]) / 2.0, (b[1] + c[1]) / 2.0, (b[2] + c[2]) / 2.0]
            };
        }
    });
    let mut out = vec![[0f32; 3]; tw * th];
    out.par_chunks_mut(tw).enumerate().for_each(|(y, row)| {
        let k = (y / 2) as isize;
        let r = |yy: isize| (yy.clamp(0, h as isize - 1)) as usize * tw;
        for (x, o) in row.iter_mut().enumerate() {
            *o = if y % 2 == 0 {
                let a = tmp[r(k - 1) + x];
                let b = tmp[r(k) + x];
                let c = tmp[r(k + 1) + x];
                [(a[0] + 6.0 * b[0] + c[0]) / 8.0, (a[1] + 6.0 * b[1] + c[1]) / 8.0, (a[2] + 6.0 * b[2] + c[2]) / 8.0]
            } else {
                let b = tmp[r(k) + x];
                let c = tmp[r(k + 1) + x];
                [(b[0] + c[0]) / 2.0, (b[1] + c[1]) / 2.0, (b[2] + c[2]) / 2.0]
            };
        }
    });
    out
}

/// Fill invalid pixels by push-pull interpolation so pyramids do not bleed black into seams.
fn fill_push_pull(img: &mut [[f32; 3]], valid: &[f32], w: usize, h: usize) {
    // premultiplied pyramid
    let mut ps: Vec<(Vec<[f32; 3]>, Vec<f32>, usize, usize)> = Vec::new();
    let pre: Vec<[f32; 3]> = img.iter().zip(valid).map(|(p, a)| [p[0] * a, p[1] * a, p[2] * a]).collect();
    ps.push((pre, valid.to_vec(), w, h));
    while {
        let (_, _, cw, chh) = ps.last().unwrap();
        *cw > 1 || *chh > 1
    } {
        let (p, a, cw, chh) = ps.last().unwrap();
        let (np, nw, nh) = reduce3(p, *cw, *chh);
        let (na, _, _) = reduce1(a, *cw, *chh);
        ps.push((np, na, nw, nh));
        if ps.len() > 24 {
            break;
        }
    }
    // coarsest: normalise
    let last = ps.len() - 1;
    let mut filled: Vec<[f32; 3]> = {
        let (p, a, _, _) = &ps[last];
        p.iter().zip(a).map(|(c, &al)| if al > 1e-6 { [c[0] / al, c[1] / al, c[2] / al] } else { [0.0; 3] }).collect()
    };
    for l in (0..last).rev() {
        let (p, a, lw, lh) = &ps[l];
        let (cw, chh) = (ps[l + 1].2, ps[l + 1].3);
        let up = expand3(&filled, cw, chh, *lw, *lh);
        filled = p
            .iter()
            .zip(a)
            .zip(up.iter())
            .map(|((c, &al), u)| {
                if al >= 0.999 {
                    [c[0] / al, c[1] / al, c[2] / al]
                } else {
                    let k = 1.0 - al.clamp(0.0, 1.0);
                    [c[0] + k * u[0], c[1] + k * u[1], c[2] + k * u[2]]
                }
            })
            .collect();
    }
    img.copy_from_slice(&filled);
}

struct Level {
    w: usize,
    h: usize,
    c: Vec<[f32; 3]>,
    wsum: Vec<f32>,
}

struct Accumulator {
    w: usize,
    h: usize,
    levels: Vec<Level>,
    multiband: bool,
}

impl Accumulator {
    fn new(w: usize, h: usize, nlev: usize) -> Self {
        let mut levels = Vec::new();
        let (mut lw, mut lh) = (w, h);
        for _ in 0..=nlev {
            levels.push(Level { w: lw, h: lh, c: vec![[0.0; 3]; lw * lh], wsum: vec![0.0; lw * lh] });
            lw = lw.div_ceil(2).max(1);
            lh = lh.div_ceil(2).max(1);
        }
        Self { w, h, levels, multiband: nlev > 0 }
    }

    fn add_feather(&mut self, tile: &[[f32; 3]], wt: &[f32], x0: usize, y0: usize, tw: usize, th: usize) {
        let lv = &mut self.levels[0];
        let cw = lv.w;
        let crow = &mut lv.c[y0 * cw..(y0 + th) * cw];
        let wrow = &mut lv.wsum[y0 * cw..(y0 + th) * cw];
        crow.par_chunks_mut(cw).zip(wrow.par_chunks_mut(cw)).enumerate().for_each(|(ry, (cr, wr))| {
            for rx in 0..tw {
                let w = wt[ry * tw + rx];
                if w <= 0.0 {
                    continue;
                }
                let p = tile[ry * tw + rx];
                let o = &mut cr[x0 + rx];
                o[0] += p[0] * w;
                o[1] += p[1] * w;
                o[2] += p[2] * w;
                wr[x0 + rx] += w;
            }
        });
    }

    fn add_multiband(&mut self, tile: Vec<[f32; 3]>, mask: Vec<f32>, x0: usize, y0: usize, tw: usize, th: usize) {
        let nlev = self.levels.len() - 1;
        // Gaussian pyramids
        let mut gimg: Vec<(Vec<[f32; 3]>, usize, usize)> = vec![(tile, tw, th)];
        let mut gmask: Vec<Vec<f32>> = vec![mask];
        for l in 0..nlev {
            let (img, w, h) = &gimg[l];
            let (ni, nw, nh) = reduce3(img, *w, *h);
            let (nm, _, _) = reduce1(&gmask[l], *w, *h);
            gimg.push((ni, nw, nh));
            gmask.push(nm);
        }
        for l in 0..=nlev {
            let (lw, lh) = (gimg[l].1, gimg[l].2);
            let lap: Vec<[f32; 3]> = if l < nlev {
                let (ref nimg, nw, nh) = gimg[l + 1];
                let up = expand3(nimg, nw, nh, lw, lh);
                gimg[l].0.par_iter().zip(up.par_iter()).map(|(a, b)| [a[0] - b[0], a[1] - b[1], a[2] - b[2]]).collect()
            } else {
                gimg[l].0.clone()
            };
            let m = &gmask[l];
            let lv = &mut self.levels[l];
            let ox = x0 >> l;
            let oy = y0 >> l;
            let cw = lv.w;
            let rows_end = (oy + lh).min(lv.h);
            let crow = &mut lv.c[oy * cw..rows_end * cw];
            let wrow = &mut lv.wsum[oy * cw..rows_end * cw];
            crow.par_chunks_mut(cw).zip(wrow.par_chunks_mut(cw)).enumerate().for_each(|(ry, (cr, wr))| {
                for rx in 0..lw {
                    let gx = ox + rx;
                    if gx >= cw {
                        break;
                    }
                    let mv = m[ry * lw + rx];
                    if mv <= 1e-7 {
                        continue;
                    }
                    let p = lap[ry * lw + rx];
                    let o = &mut cr[gx];
                    o[0] += p[0] * mv;
                    o[1] += p[1] * mv;
                    o[2] += p[2] * mv;
                    wr[gx] += mv;
                }
            });
        }
    }

    fn finish(self) -> RgbImage {
        let norm = |lv: &Level| -> Vec<[f32; 3]> {
            lv.c.par_iter()
                .zip(lv.wsum.par_iter())
                .map(|(c, &w)| if w > 1e-6 { [c[0] / w, c[1] / w, c[2] / w] } else { [0.0; 3] })
                .collect()
        };
        if !self.multiband {
            let data: Vec<[f32; 3]> = norm(&self.levels[0]).into_iter().map(|p| [p[0].max(0.0), p[1].max(0.0), p[2].max(0.0)]).collect();
            return RgbImage { w: self.w, h: self.h, data };
        }
        let nlev = self.levels.len() - 1;
        let mut cur = norm(&self.levels[nlev]);
        for l in (0..nlev).rev() {
            let (cw, chh) = (self.levels[l + 1].w, self.levels[l + 1].h);
            let (lw, lh) = (self.levels[l].w, self.levels[l].h);
            let up = expand3(&cur, cw, chh, lw, lh);
            let band = norm(&self.levels[l]);
            cur = up.par_iter().zip(band.par_iter()).map(|(a, b)| [a[0] + b[0], a[1] + b[1], a[2] + b[2]]).collect();
        }
        let data = cur.into_iter().map(|p| [p[0].max(0.0), p[1].max(0.0), p[2].max(0.0)]).collect();
        RgbImage { w: self.w, h: self.h, data }
    }
}
