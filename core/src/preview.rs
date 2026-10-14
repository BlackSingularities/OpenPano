//! Real-time preview: the blended panorama is rendered once into an equirectangular "sphere map"
//! (identity view); any projection / view change is then a fast per-pixel remap of that map.

use crate::imaging::ToSrgb8;
use crate::stitch::geometry::{Projection, V3};
use rayon::prelude::*;

pub struct Sphere {
    pub u0: f64,
    pub v0: f64,
    pub scale: f64,
    pub w: usize,
    pub h: usize,
    /// sRGB RGBA, alpha = coverage
    pub rgba: Vec<[u8; 4]>,
    pub wraps: bool,
}

impl Sphere {
    pub fn from_render(out: &crate::stitch::render::RenderOutput) -> Sphere {
        let c = &out.canvas;
        let conv = ToSrgb8::new();
        let rgba = out
            .image
            .data
            .par_iter()
            .zip(out.alpha.par_iter())
            .map(|(p, a)| [conv.conv(p[0]), conv.conv(p[1]), conv.conv(p[2]), *a])
            .collect();
        let wraps = (c.w as f64 / c.scale) > std::f64::consts::TAU * 0.995;
        Sphere { u0: c.u0, v0: c.v0, scale: c.scale, w: out.image.w, h: out.image.h, rgba, wraps }
    }

    /// Pixels per radian of the map.
    pub fn resolution(&self) -> f64 {
        self.scale
    }

    #[inline]
    pub fn sample(&self, d: &V3) -> Option<[f32; 4]> {
        let (u, v) = Projection::Equirectangular.forward(d)?;
        let mut x = (u - self.u0) * self.scale - 0.5;
        let y = (v - self.v0) * self.scale - 0.5;
        let w = self.w as f64;
        if self.wraps {
            x = x.rem_euclid(w);
        }
        if y < -0.5 || y > self.h as f64 - 0.5 || x < -0.5 || x > w - 0.5 {
            return None;
        }
        let y = y.clamp(0.0, (self.h - 1) as f64);
        let xf = x.floor();
        let yf = y.floor();
        let fx = (x - xf) as f32;
        let fy = (y - yf) as f32;
        let x0 = (xf as isize).clamp(0, self.w as isize - 1) as usize;
        let y0 = (yf as usize).min(self.h - 1);
        let x1 = if self.wraps { (x0 + 1) % self.w } else { (x0 + 1).min(self.w - 1) };
        let y1 = (y0 + 1).min(self.h - 1);
        let p = |xx: usize, yy: usize| self.rgba[yy * self.w + xx];
        let (a, b, c, e) = (p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1));
        let mut o = [0f32; 4];
        for k in 0..4 {
            let t = a[k] as f32 + (b[k] as f32 - a[k] as f32) * fx;
            let bt = c[k] as f32 + (e[k] as f32 - c[k] as f32) * fx;
            o[k] = t + (bt - t) * fy;
        }
        Some(o)
    }
}
