//! Photometric calibration from feature correspondences: per-image exposure gain (optionally per
//! colour channel) and a shared radial vignetting model, solved jointly in the log domain.

use super::align::Alignment;
use crate::imaging::RgbImage;
use nalgebra::{DMatrix, DVector};

#[derive(Clone, Debug)]
pub struct Photometric {
    pub gains: Vec<[f32; 3]>,
    /// log-vignetting coefficients: correction = exp(a r² + b r⁴), r normalised to half diagonal.
    pub vig: [f64; 2],
}

impl Photometric {
    pub fn identity(n: usize) -> Self {
        Self { gains: vec![[1.0; 3]; n], vig: [0.0, 0.0] }
    }

    /// Correction factor for normalised squared radius r2.
    #[inline]
    pub fn vig_factor(&self, r2: f64) -> f32 {
        (self.vig[0] * r2 + self.vig[1] * r2 * r2).exp() as f32
    }
}

pub struct PhotoSettings {
    pub exposure: bool,
    pub color: bool,
    pub vignetting: bool,
}

fn patch_mean(img: &RgbImage, x: f64, y: f64) -> Option<[f32; 3]> {
    let xi = x.round() as isize;
    let yi = y.round() as isize;
    if xi < 2 || yi < 2 || xi + 2 >= img.w as isize || yi + 2 >= img.h as isize {
        return None;
    }
    let mut acc = [0f32; 3];
    for dy in -2..=2 {
        for dx in -2..=2 {
            let p = img.get((xi + dx) as usize, (yi + dy) as usize);
            for k in 0..3 {
                acc[k] += p[k];
            }
        }
    }
    let m = [acc[0] / 25.0, acc[1] / 25.0, acc[2] / 25.0];
    if m.iter().any(|v| *v < 0.003 || *v > 0.92) {
        return None;
    }
    Some(m)
}

/// `work` holds the working-scale images and `scales` the work/full scale factor per image.
pub fn estimate(al: &Alignment, work: &[RgbImage], scales: &[f64], st: &PhotoSettings) -> Photometric {
    let n = work.len();
    let mut ph = Photometric::identity(n);
    if !st.exposure && !st.color && !st.vignetting {
        return ph;
    }
    // unknown layout
    let per_img = if st.color { 3 } else { 1 };
    let use_gain = st.exposure || st.color;
    let ng = if use_gain { n * per_img } else { 0 };
    let nv = if st.vignetting { 2 } else { 0 };
    let nu = ng + nv;
    if nu == 0 {
        return ph;
    }
    struct Obs {
        gi: usize,
        gj: usize,
        dr2: f64,
        dr4: f64,
        rhs: f64,
    }
    let mut obs: Vec<Obs> = Vec::new();
    for p in &al.pairs {
        let (Some(ci), Some(cj)) = (&al.cameras[p.i], &al.cameras[p.j]) else { continue };
        let hd_i = ((ci.w * ci.w + ci.h * ci.h) as f64).sqrt() / 2.0;
        let hd_j = ((cj.w * cj.w + cj.h * cj.h) as f64).sqrt() / 2.0;
        for (a, b) in &p.pts {
            let wi = &work[p.i];
            let wj = &work[p.j];
            let xa = a[0] * scales[p.i] + (wi.w as f64 - 1.0) / 2.0;
            let ya = a[1] * scales[p.i] + (wi.h as f64 - 1.0) / 2.0;
            let xb = b[0] * scales[p.j] + (wj.w as f64 - 1.0) / 2.0;
            let yb = b[1] * scales[p.j] + (wj.h as f64 - 1.0) / 2.0;
            let (Some(ia), Some(ib)) = (patch_mean(wi, xa, ya), patch_mean(wj, xb, yb)) else { continue };
            let r2a = (a[0] * a[0] + a[1] * a[1]) / (hd_i * hd_i);
            let r2b = (b[0] * b[0] + b[1] * b[1]) / (hd_j * hd_j);
            let chans: Vec<(usize, f64, f64)> = if st.color {
                (0..3).map(|c| (c, ia[c] as f64, ib[c] as f64)).collect()
            } else {
                let la = crate::imaging::luma(ia) as f64;
                let lb = crate::imaging::luma(ib) as f64;
                vec![(0, la, lb)]
            };
            for (c, va, vb) in chans {
                obs.push(Obs {
                    gi: p.i * per_img + c,
                    gj: p.j * per_img + c,
                    dr2: r2a - r2b,
                    dr4: r2a * r2a - r2b * r2b,
                    rhs: vb.ln() - va.ln(),
                });
            }
        }
    }
    if obs.len() < 20 {
        return ph;
    }
    let mut active: Vec<bool> = vec![true; obs.len()];
    let mut x = DVector::<f64>::zeros(nu);
    for _round in 0..3 {
        let mut a = DMatrix::<f64>::zeros(nu, nu);
        let mut bvec = DVector::<f64>::zeros(nu);
        for (k, o) in obs.iter().enumerate() {
            if !active[k] {
                continue;
            }
            let mut row: Vec<(usize, f64)> = Vec::with_capacity(4);
            if use_gain {
                row.push((o.gi, 1.0));
                row.push((o.gj, -1.0));
            }
            if st.vignetting {
                row.push((ng, o.dr2));
                row.push((ng + 1, o.dr4));
            }
            for &(i, vi) in &row {
                bvec[i] += vi * o.rhs;
                for &(j, vj) in &row {
                    a[(i, j)] += vi * vj;
                }
            }
        }
        // Priors: gains near 1 (log 0), vignetting near 0
        let prior = (obs.len() as f64 / n as f64 * 0.01).max(0.1);
        for i in 0..ng {
            a[(i, i)] += prior;
        }
        for i in ng..nu {
            a[(i, i)] += prior * 0.1;
        }
        let Some(ch) = a.cholesky() else { return ph };
        x = ch.solve(&bvec);
        // outlier rejection
        for (k, o) in obs.iter().enumerate() {
            let mut pred = 0.0;
            if use_gain {
                pred += x[o.gi] - x[o.gj];
            }
            if st.vignetting {
                pred += x[ng] * o.dr2 + x[ng + 1] * o.dr4;
            }
            active[k] = (pred - o.rhs).abs() < 0.25;
        }
    }
    if use_gain {
        // normalise each channel to mean log-gain 0 across aligned images
        let aligned: Vec<usize> = (0..n).filter(|&i| al.cameras[i].is_some()).collect();
        for c in 0..per_img {
            let mean: f64 = aligned.iter().map(|&i| x[i * per_img + c]).sum::<f64>() / aligned.len().max(1) as f64;
            for &i in &aligned {
                x[i * per_img + c] -= mean;
            }
        }
        for i in 0..n {
            let g = if st.color {
                [x[i * 3].exp(), x[i * 3 + 1].exp(), x[i * 3 + 2].exp()]
            } else {
                let v = x[i].exp();
                [v, v, v]
            };
            let mut g = [g[0] as f32, g[1] as f32, g[2] as f32];
            if !st.exposure && st.color {
                // keep only the colour balance, remove overall exposure difference
                let l = crate::imaging::luma(g);
                g = [g[0] / l, g[1] / l, g[2] / l];
            }
            for v in g.iter_mut() {
                *v = v.clamp(0.1, 10.0);
            }
            ph.gains[i] = g;
        }
    }
    if st.vignetting {
        let (a, b) = (x[ng], x[ng + 1]);
        // sanity: correction in corner between 0.8x and 3x and monotonic-ish
        let corner = a + b;
        if corner.is_finite() && (-0.2..1.1).contains(&corner) && a.is_finite() && b.is_finite() {
            ph.vig = [a, b];
        }
    }
    ph
}
