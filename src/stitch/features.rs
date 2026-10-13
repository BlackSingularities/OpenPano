//! ORB-style feature detection: multi-scale FAST-9 corners ranked by Harris response, spatially
//! distributed with a grid, oriented by intensity centroid, described by rotated BRIEF (256 bit).

use rayon::prelude::*;

#[derive(Clone, Copy, Debug)]
pub struct Keypoint {
    /// Position in level-0 pixel coordinates.
    pub x: f32,
    pub y: f32,
    pub angle: f32,
    pub response: f32,
    pub level: u8,
}

pub type Descriptor = [u64; 4];

pub struct Features {
    pub keypoints: Vec<Keypoint>,
    pub descriptors: Vec<Descriptor>,
}

/// Single-channel f32 image in perceptual (gamma) space, range ~0..255.
#[derive(Clone)]
pub struct Gray {
    pub w: usize,
    pub h: usize,
    pub d: Vec<f32>,
}

impl Gray {
    #[inline]
    fn at(&self, x: usize, y: usize) -> f32 {
        self.d[y * self.w + x]
    }

    fn downscale(&self, factor: f32) -> Gray {
        let nw = ((self.w as f32) / factor).round().max(1.0) as usize;
        let nh = ((self.h as f32) / factor).round().max(1.0) as usize;
        let mut d = vec![0f32; nw * nh];
        d.par_chunks_mut(nw).enumerate().for_each(|(y, row)| {
            let sy = (y as f32 + 0.5) * factor - 0.5;
            for (x, v) in row.iter_mut().enumerate() {
                let sx = (x as f32 + 0.5) * factor - 0.5;
                *v = bilinear(self, sx, sy);
            }
        });
        // light blur to reduce aliasing
        blur(&Gray { w: nw, h: nh, d }, 0.6)
    }
}

#[inline]
fn bilinear(g: &Gray, x: f32, y: f32) -> f32 {
    let x = x.clamp(0.0, (g.w - 1) as f32);
    let y = y.clamp(0.0, (g.h - 1) as f32);
    let x0 = x as usize;
    let y0 = y as usize;
    let x1 = (x0 + 1).min(g.w - 1);
    let y1 = (y0 + 1).min(g.h - 1);
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let a = g.at(x0, y0) + (g.at(x1, y0) - g.at(x0, y0)) * fx;
    let b = g.at(x0, y1) + (g.at(x1, y1) - g.at(x0, y1)) * fx;
    a + (b - a) * fy
}

pub fn blur(g: &Gray, sigma: f32) -> Gray {
    let r = (sigma * 3.0).ceil() as isize;
    let mut k: Vec<f32> = (-r..=r).map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp()).collect();
    let s: f32 = k.iter().sum();
    k.iter_mut().for_each(|v| *v /= s);
    let (w, h) = (g.w, g.h);
    let mut tmp = vec![0f32; w * h];
    tmp.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, v) in row.iter_mut().enumerate() {
            let mut acc = 0.0;
            for (i, kv) in k.iter().enumerate() {
                let xx = (x as isize + i as isize - r).clamp(0, w as isize - 1) as usize;
                acc += g.d[y * w + xx] * kv;
            }
            *v = acc;
        }
    });
    let mut out = vec![0f32; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, v) in row.iter_mut().enumerate() {
            let mut acc = 0.0;
            for (i, kv) in k.iter().enumerate() {
                let yy = (y as isize + i as isize - r).clamp(0, h as isize - 1) as usize;
                acc += tmp[yy * w + x] * kv;
            }
            *v = acc;
        }
    });
    Gray { w, h, d: out }
}

/// Convert linear luminance to an 8-bit-like perceptual scale with local normalisation-friendly
/// gamma. Exposure-independent detection is helped by normalising by the image median.
pub fn gray_from_linear(lum: &[f32], w: usize, h: usize) -> Gray {
    let mut sorted: Vec<f32> = lum.iter().step_by(17).cloned().collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let med = sorted.get(sorted.len() / 2).cloned().unwrap_or(0.18).max(1e-4);
    let scale = 0.18 / med;
    let d = lum.par_iter().map(|v| (v * scale).max(0.0).powf(1.0 / 2.2).min(2.0) * 180.0).collect();
    Gray { w, h, d }
}

const CIRCLE: [(isize, isize); 16] = [
    (0, -3), (1, -3), (2, -2), (3, -1), (3, 0), (3, 1), (2, 2), (1, 3),
    (0, 3), (-1, 3), (-2, 2), (-3, 1), (-3, 0), (-3, -1), (-2, -2), (-1, -3),
];

#[inline]
fn is_fast(g: &Gray, x: usize, y: usize, t: f32) -> bool {
    let c = g.at(x, y);
    let hi = c + t;
    let lo = c - t;
    let px = |i: usize| -> f32 {
        let (dx, dy) = CIRCLE[i];
        g.d[(y as isize + dy) as usize * g.w + (x as isize + dx) as usize]
    };
    // quick rejection using pixels 0,4,8,12
    let p0 = px(0);
    let p8 = px(8);
    let p4 = px(4);
    let p12 = px(12);
    let nb = (p0 > hi) as u8 + (p4 > hi) as u8 + (p8 > hi) as u8 + (p12 > hi) as u8;
    let nd = (p0 < lo) as u8 + (p4 < lo) as u8 + (p8 < lo) as u8 + (p12 < lo) as u8;
    if nb < 2 && nd < 2 {
        return false;
    }
    let mut vals = [0f32; 16];
    for (i, v) in vals.iter_mut().enumerate() {
        *v = px(i);
    }
    for sign in [1.0f32, -1.0] {
        let mut run = 0;
        for i in 0..25 {
            let v = vals[i % 16];
            let ok = if sign > 0.0 { v > hi } else { v < lo };
            if ok {
                run += 1;
                if run >= 9 {
                    return true;
                }
            } else {
                run = 0;
            }
        }
    }
    false
}

fn harris(g: &Gray, x: usize, y: usize) -> f32 {
    let r = 3isize;
    let (mut a, mut b, mut c) = (0f32, 0f32, 0f32);
    for dy in -r..=r {
        for dx in -r..=r {
            let xx = (x as isize + dx) as usize;
            let yy = (y as isize + dy) as usize;
            let ix = g.at(xx + 1, yy) - g.at(xx - 1, yy);
            let iy = g.at(xx, yy + 1) - g.at(xx, yy - 1);
            a += ix * ix;
            b += iy * iy;
            c += ix * iy;
        }
    }
    a * b - c * c - 0.04 * (a + b) * (a + b)
}

const PATCH: isize = 15;
const BORDER: usize = 20;

fn orientation(g: &Gray, x: usize, y: usize) -> f32 {
    let (mut m01, mut m10) = (0f32, 0f32);
    for dy in -PATCH..=PATCH {
        let span = ((PATCH * PATCH - dy * dy) as f32).sqrt() as isize;
        for dx in -span..=span {
            let v = g.at((x as isize + dx) as usize, (y as isize + dy) as usize);
            m10 += dx as f32 * v;
            m01 += dy as f32 * v;
        }
    }
    m01.atan2(m10)
}

fn brief_pattern() -> &'static [(f32, f32, f32, f32); 256] {
    use std::sync::OnceLock;
    static P: OnceLock<[(f32, f32, f32, f32); 256]> = OnceLock::new();
    P.get_or_init(|| {
        // Deterministic xorshift + Box-Muller, isotropic Gaussian sampling (sigma = S/5)
        let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut rnd = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut gauss = || {
            let u1: f64 = rnd().max(1e-12);
            let u2: f64 = rnd();
            (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
        };
        let mut out = [(0f32, 0f32, 0f32, 0f32); 256];
        let sigma = 31.0 / 5.0;
        let lim = 13.0;
        for p in out.iter_mut() {
            let mut c = || (gauss() * sigma).clamp(-lim, lim) as f32;
            *p = (c(), c(), c(), c());
        }
        out
    })
}

fn describe(g: &Gray, x: f32, y: f32, angle: f32) -> Descriptor {
    let (s, c) = angle.sin_cos();
    let pat = brief_pattern();
    let mut d = [0u64; 4];
    for (i, (x1, y1, x2, y2)) in pat.iter().enumerate() {
        let ax = x + c * x1 - s * y1;
        let ay = y + s * x1 + c * y1;
        let bx = x + c * x2 - s * y2;
        let by = y + s * x2 + c * y2;
        let va = g.at(ax.round() as usize, ay.round() as usize);
        let vb = g.at(bx.round() as usize, by.round() as usize);
        if va < vb {
            d[i / 64] |= 1u64 << (i % 64);
        }
    }
    d
}

/// Detect up to `max_features` features over a pyramid.
pub fn detect(base: &Gray, max_features: usize) -> Features {
    let levels = 4usize;
    let factor = 1.4f32;
    let mut pyr = vec![blur(base, 0.5)];
    for l in 1..levels {
        let prev = &pyr[l - 1];
        if prev.w < 80 || prev.h < 80 {
            break;
        }
        pyr.push(prev.downscale(factor));
    }
    let total_area: f32 = pyr.iter().map(|g| (g.w * g.h) as f32).sum();

    let mut kps: Vec<Keypoint> = Vec::new();
    let mut descs: Vec<Descriptor> = Vec::new();
    for (l, g) in pyr.iter().enumerate() {
        let budget = ((max_features as f32) * (g.w * g.h) as f32 / total_area).ceil() as usize;
        if g.w <= 2 * BORDER || g.h <= 2 * BORDER {
            continue;
        }
        // Adaptive FAST threshold: retry lower thresholds if too few corners.
        let mut cands: Vec<(usize, usize, f32)> = Vec::new();
        for t in [20.0f32, 12.0, 7.0, 4.0] {
            cands = (BORDER..g.h - BORDER)
                .into_par_iter()
                .flat_map_iter(|y| {
                    let mut v = Vec::new();
                    for x in BORDER..g.w - BORDER {
                        if is_fast(g, x, y, t) {
                            v.push((x, y, harris(g, x, y)));
                        }
                    }
                    v
                })
                .collect();
            if cands.len() > budget * 3 {
                break;
            }
        }
        // 3x3 non-maximum suppression via hashmap of scores
        let mut score = vec![f32::MIN; g.w * g.h];
        for &(x, y, r) in &cands {
            score[y * g.w + x] = r;
        }
        let cands: Vec<(usize, usize, f32)> = cands
            .into_iter()
            .filter(|&(x, y, r)| {
                for dy in -1isize..=1 {
                    for dx in -1isize..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let s = score[(y as isize + dy) as usize * g.w + (x as isize + dx) as usize];
                        if s > r {
                            return false;
                        }
                    }
                }
                true
            })
            .collect();
        // Grid bucketing for even spatial distribution
        let gx = 8usize;
        let gy = ((gx as f32) * g.h as f32 / g.w as f32).round().max(2.0) as usize;
        let per_cell = (budget / (gx * gy)).max(2);
        let mut cells: Vec<Vec<(usize, usize, f32)>> = vec![Vec::new(); gx * gy];
        for c in cands {
            let cx = (c.0 * gx / g.w).min(gx - 1);
            let cy = (c.1 * gy / g.h).min(gy - 1);
            cells[cy * gx + cx].push(c);
        }
        let mut chosen = Vec::new();
        let mut rest = Vec::new();
        for mut cell in cells {
            cell.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
            let k = per_cell.min(cell.len());
            chosen.extend_from_slice(&cell[..k]);
            rest.extend_from_slice(&cell[k..]);
        }
        if chosen.len() < budget {
            rest.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
            let need = budget - chosen.len();
            chosen.extend(rest.into_iter().take(need));
        }
        chosen.truncate(budget);
        let smooth = blur(g, 1.6);
        let scale = factor.powi(l as i32);
        let sx = base.w as f32 / g.w as f32;
        let sy = base.h as f32 / g.h as f32;
        let _ = scale;
        let res: Vec<(Keypoint, Descriptor)> = chosen
            .par_iter()
            .map(|&(x, y, r)| {
                let a = orientation(&smooth, x, y);
                let d = describe(&smooth, x as f32, y as f32, a);
                (
                    Keypoint {
                        x: (x as f32 + 0.5) * sx - 0.5,
                        y: (y as f32 + 0.5) * sy - 0.5,
                        angle: a,
                        response: r,
                        level: l as u8,
                    },
                    d,
                )
            })
            .collect();
        for (k, d) in res {
            kps.push(k);
            descs.push(d);
        }
    }
    Features { keypoints: kps, descriptors: descs }
}

#[inline]
pub fn hamming(a: &Descriptor, b: &Descriptor) -> u32 {
    (a[0] ^ b[0]).count_ones() + (a[1] ^ b[1]).count_ones() + (a[2] ^ b[2]).count_ones() + (a[3] ^ b[3]).count_ones()
}

/// Ratio-test + cross-check matching. Returns pairs (index in a, index in b).
pub fn match_features(a: &Features, b: &Features, ratio: f32) -> Vec<(usize, usize)> {
    if a.descriptors.is_empty() || b.descriptors.is_empty() {
        return Vec::new();
    }
    let best = |from: &[Descriptor], to: &[Descriptor]| -> Vec<(usize, u32, u32)> {
        from.par_iter()
            .map(|d| {
                let (mut b1, mut b2, mut i1) = (u32::MAX, u32::MAX, 0usize);
                for (j, e) in to.iter().enumerate() {
                    let h = hamming(d, e);
                    if h < b1 {
                        b2 = b1;
                        b1 = h;
                        i1 = j;
                    } else if h < b2 {
                        b2 = h;
                    }
                }
                (i1, b1, b2)
            })
            .collect()
    };
    let ab = best(&a.descriptors, &b.descriptors);
    let ba = best(&b.descriptors, &a.descriptors);
    let mut out = Vec::new();
    for (i, &(j, d1, d2)) in ab.iter().enumerate() {
        if d1 > 80 {
            continue;
        }
        if (d1 as f32) >= ratio * d2 as f32 {
            continue;
        }
        if ba[j].0 != i {
            continue;
        }
        out.push((i, j));
    }
    out
}
