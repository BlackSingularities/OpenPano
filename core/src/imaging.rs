//! Basic linear-light image containers and colour helpers.

use rayon::prelude::*;

/// Linear-light RGB image, f32 per channel, row-major.
#[derive(Clone)]
pub struct RgbImage {
    pub w: usize,
    pub h: usize,
    pub data: Vec<[f32; 3]>,
}

impl RgbImage {
    pub fn new(w: usize, h: usize) -> Self {
        Self { w, h, data: vec![[0.0; 3]; w * h] }
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> [f32; 3] {
        self.data[y * self.w + x]
    }

    /// Bilinear sample at continuous pixel coordinates (pixel centres at integer + 0.5 is NOT used;
    /// pixel (0,0) centre is at 0.0). Returns None outside the image.
    #[inline]
    pub fn sample(&self, x: f32, y: f32) -> Option<[f32; 3]> {
        if !(x >= 0.0 && y >= 0.0) {
            return None;
        }
        let x0 = x as usize;
        let y0 = y as usize;
        if x0 + 1 >= self.w || y0 + 1 >= self.h {
            if x0 < self.w && y0 < self.h && x <= (self.w - 1) as f32 && y <= (self.h - 1) as f32 {
                return Some(self.get(x0, y0));
            }
            return None;
        }
        let fx = x - x0 as f32;
        let fy = y - y0 as f32;
        let i = y0 * self.w + x0;
        let a = self.data[i];
        let b = self.data[i + 1];
        let c = self.data[i + self.w];
        let d = self.data[i + self.w + 1];
        let mut o = [0.0f32; 3];
        for k in 0..3 {
            let top = a[k] + (b[k] - a[k]) * fx;
            let bot = c[k] + (d[k] - c[k]) * fx;
            o[k] = top + (bot - top) * fy;
        }
        Some(o)
    }

    /// Bicubic (Catmull-Rom) sample; falls back to bilinear near borders.
    #[inline]
    pub fn sample_cubic(&self, x: f32, y: f32) -> Option<[f32; 3]> {
        let xi = x.floor() as isize;
        let yi = y.floor() as isize;
        if xi < 1 || yi < 1 || xi + 2 >= self.w as isize || yi + 2 >= self.h as isize {
            return self.sample(x, y);
        }
        let fx = x - xi as f32;
        let fy = y - yi as f32;
        let wx = catmull(fx);
        let wy = catmull(fy);
        let mut o = [0.0f32; 3];
        for (j, wyj) in wy.iter().enumerate() {
            let row = (yi - 1 + j as isize) as usize * self.w;
            let mut r = [0.0f32; 3];
            for (i, wxi) in wx.iter().enumerate() {
                let p = self.data[row + (xi - 1 + i as isize) as usize];
                r[0] += p[0] * wxi;
                r[1] += p[1] * wxi;
                r[2] += p[2] * wxi;
            }
            o[0] += r[0] * wyj;
            o[1] += r[1] * wyj;
            o[2] += r[2] * wyj;
        }
        // Clamp tiny negative ringing
        Some([o[0].max(0.0), o[1].max(0.0), o[2].max(0.0)])
    }

    /// Box/area downscale by an arbitrary factor (0 < s <= 1).
    pub fn resized(&self, nw: usize, nh: usize) -> RgbImage {
        let nw = nw.max(1);
        let nh = nh.max(1);
        let sx = self.w as f32 / nw as f32;
        let sy = self.h as f32 / nh as f32;
        let mut out = RgbImage::new(nw, nh);
        out.data.par_chunks_mut(nw).enumerate().for_each(|(y, row)| {
            let y0 = (y as f32 * sy) as usize;
            let y1 = (((y + 1) as f32 * sy).ceil() as usize).min(self.h).max(y0 + 1);
            for (x, px) in row.iter_mut().enumerate() {
                let x0 = (x as f32 * sx) as usize;
                let x1 = (((x + 1) as f32 * sx).ceil() as usize).min(self.w).max(x0 + 1);
                let mut acc = [0.0f32; 3];
                let mut n = 0.0f32;
                for yy in y0..y1 {
                    let r = yy * self.w;
                    for xx in x0..x1 {
                        let p = self.data[r + xx];
                        acc[0] += p[0];
                        acc[1] += p[1];
                        acc[2] += p[2];
                        n += 1.0;
                    }
                }
                *px = [acc[0] / n, acc[1] / n, acc[2] / n];
            }
        });
        out
    }

    pub fn scaled_to_megapixels(&self, mp: f64) -> (RgbImage, f64) {
        let cur = (self.w * self.h) as f64 / 1e6;
        if cur <= mp {
            return (self.clone(), 1.0);
        }
        let s = (mp / cur).sqrt();
        let nw = (self.w as f64 * s).round() as usize;
        let nh = (self.h as f64 * s).round() as usize;
        let s_eff = nw as f64 / self.w as f64;
        (self.resized(nw, nh), s_eff)
    }

    pub fn rotate_orientation(self, orientation: u32) -> RgbImage {
        // EXIF orientation values
        match orientation {
            2 => self.transform(|x, y, w, _h| (w - 1 - x, y), false),
            3 => self.transform(|x, y, w, h| (w - 1 - x, h - 1 - y), false),
            4 => self.transform(|x, y, _w, h| (x, h - 1 - y), false),
            5 => self.transform(|x, y, _w, _h| (y, x), true),
            6 => self.transform(|x, y, _w, h| (y, h - 1 - x), true),
            7 => self.transform(|x, y, w, h| (w - 1 - y, h - 1 - x), true),
            8 => self.transform(|x, y, w, _h| (w - 1 - y, x), true),
            _ => self,
        }
    }

    /// Generic remap where `f(dst_x, dst_y, src_w, src_h)` returns source coordinates.
    fn transform(self, f: impl Fn(usize, usize, usize, usize) -> (usize, usize) + Sync, swap: bool) -> RgbImage {
        let (nw, nh) = if swap { (self.h, self.w) } else { (self.w, self.h) };
        let mut out = RgbImage::new(nw, nh);
        let src = &self;
        out.data.par_chunks_mut(nw).enumerate().for_each(|(y, row)| {
            for (x, px) in row.iter_mut().enumerate() {
                let (sx, sy) = f(x, y, src.w, src.h);
                *px = src.data[sy * src.w + sx];
            }
        });
        out
    }

    pub fn luminance(&self) -> Vec<f32> {
        self.data.par_iter().map(|p| luma(*p)).collect()
    }
}

#[inline]
fn catmull(t: f32) -> [f32; 4] {
    let t2 = t * t;
    let t3 = t2 * t;
    [
        -0.5 * t3 + t2 - 0.5 * t,
        1.5 * t3 - 2.5 * t2 + 1.0,
        -1.5 * t3 + 2.0 * t2 + 0.5 * t,
        0.5 * t3 - 0.5 * t2,
    ]
}

#[inline]
pub fn luma(p: [f32; 3]) -> f32 {
    0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]
}

#[inline]
pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

#[inline]
pub fn linear_to_srgb(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// Lookup table for 8-bit sRGB -> linear.
pub fn srgb8_lut() -> [f32; 256] {
    let mut t = [0.0f32; 256];
    for (i, v) in t.iter_mut().enumerate() {
        *v = srgb_to_linear(i as f32 / 255.0);
    }
    t
}

/// Fast linear->sRGB 8-bit via 4096-entry table.
pub struct ToSrgb8 {
    lut: Vec<u8>,
}

impl ToSrgb8 {
    pub fn new() -> Self {
        let n = 4096;
        let lut = (0..=n)
            .map(|i| (linear_to_srgb(i as f32 / n as f32) * 255.0 + 0.5) as u8)
            .collect();
        Self { lut }
    }
    #[inline]
    pub fn conv(&self, v: f32) -> u8 {
        let i = (v.clamp(0.0, 1.0) * 4096.0) as usize;
        self.lut[i]
    }
}

impl Default for ToSrgb8 {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple global tone operator for display of linear HDR-ish data (used for RAW previews):
/// soft highlight roll-off above `knee`.
#[inline]
pub fn soft_clip(v: f32) -> f32 {
    let knee = 0.8;
    if v <= knee {
        v
    } else {
        let x = v - knee;
        let r = 1.0 - knee;
        knee + r * (x / (x + r))
    }
}
