//! Global registration: pairwise matching with robust rotation estimation, maximum spanning tree
//! initialisation, Levenberg-Marquardt bundle adjustment of rotations and focal lengths, and
//! horizon straightening (wave correction).

use super::features::{match_features, Features};
use super::geometry::{LensType, Rot, V3};
use super::{Cancel, Progress, StitchError};
use nalgebra::{DMatrix, DVector, Matrix3};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchStrategy {
    Auto,
    AllPairs,
    Sequential,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AlignSettings {
    pub lens: LensType,
    /// Manual horizontal field of view in degrees (overrides EXIF / estimation).
    pub manual_hfov: Option<f64>,
    pub max_features: usize,
    pub strategy: MatchStrategy,
    /// Neighbour window for sequential matching.
    pub window: usize,
    pub optimize_focal: bool,
    pub straighten: bool,
    pub ransac_threshold_px: f64,
}

impl Default for AlignSettings {
    fn default() -> Self {
        Self {
            lens: LensType::Rectilinear,
            manual_hfov: None,
            max_features: 2500,
            strategy: MatchStrategy::Auto,
            window: 3,
            optimize_focal: true,
            straighten: true,
            ransac_threshold_px: 3.0,
        }
    }
}

/// Per-image input to the aligner: features detected at working scale.
pub struct AlignImage {
    pub features: Features,
    /// working / full resolution scale
    pub scale: f64,
    pub work_w: usize,
    pub work_h: usize,
    pub full_w: usize,
    pub full_h: usize,
    /// focal length in full-resolution pixels from EXIF, if known
    pub exif_focal: Option<f64>,
    /// grouping key for shared focal length (same camera/lens/size)
    pub focal_key: String,
}

#[derive(Clone, Debug)]
pub struct Camera {
    pub f: f64,
    pub rot: Rot,
    pub w: usize,
    pub h: usize,
}

impl Camera {
    #[inline]
    pub fn ray_world(&self, lens: LensType, x: f64, y: f64) -> V3 {
        let cx = (self.w as f64 - 1.0) / 2.0;
        let cy = (self.h as f64 - 1.0) / 2.0;
        self.rot * lens.ray(x - cx, y - cy, self.f)
    }
}

#[derive(Clone, Debug)]
pub struct PairMatch {
    pub i: usize,
    pub j: usize,
    /// Inlier correspondences in full-resolution pixel coordinates centred at principal point.
    pub pts: Vec<([f64; 2], [f64; 2])>,
    pub confidence: f64,
}

#[derive(Clone, Debug)]
pub struct Alignment {
    pub cameras: Vec<Option<Camera>>,
    pub lens: LensType,
    pub pairs: Vec<PairMatch>,
    pub rms_px: f64,
    pub focal_source: FocalSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocalSource {
    Manual,
    Exif,
    Estimated,
    Default,
}

impl Alignment {
    pub fn aligned_count(&self) -> usize {
        self.cameras.iter().filter(|c| c.is_some()).count()
    }
}

fn centered(img: &AlignImage, kx: f32, ky: f32) -> [f64; 2] {
    let cx = (img.work_w as f64 - 1.0) / 2.0;
    let cy = (img.work_h as f64 - 1.0) / 2.0;
    [(kx as f64 - cx) / img.scale, (ky as f64 - cy) / img.scale]
}

pub fn align(imgs: &[AlignImage], st: &AlignSettings, progress: &Progress, cancel: &Cancel) -> Result<Alignment, StitchError> {
    let n = imgs.len();
    if n < 2 {
        return Err(StitchError::NotEnoughImages);
    }
    // ---------- candidate pairs ----------
    let strategy = match st.strategy {
        MatchStrategy::Auto => {
            if n <= 40 {
                MatchStrategy::AllPairs
            } else {
                MatchStrategy::Sequential
            }
        }
        s => s,
    };
    let mut cand: Vec<(usize, usize)> = Vec::new();
    match strategy {
        MatchStrategy::AllPairs | MatchStrategy::Auto => {
            for i in 0..n {
                for j in i + 1..n {
                    cand.push((i, j));
                }
            }
        }
        MatchStrategy::Sequential => {
            let w = st.window.max(1);
            for i in 0..n {
                for k in 1..=w {
                    let j = i + k;
                    if j < n {
                        cand.push((i, j));
                    }
                }
            }
            // loop closure for 360° sequences
            for k in 0..w.min(n) {
                for m in 0..w.min(n) {
                    let a = k;
                    let b = n - 1 - m;
                    if b > a + w {
                        cand.push((a, b));
                    }
                }
            }
            cand.sort();
            cand.dedup();
        }
    }

    // ---------- raw matching ----------
    let total = cand.len().max(1);
    let done = std::sync::atomic::AtomicUsize::new(0);
    let raw: Vec<(usize, usize, Vec<([f64; 2], [f64; 2])>)> = cand
        .par_iter()
        .filter_map(|&(i, j)| {
            if cancel.is_cancelled() {
                return None;
            }
            let m = match_features(&imgs[i].features, &imgs[j].features, 0.8);
            let c = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            progress.set_fraction(0.35 * c as f32 / total as f32);
            if m.len() < 10 {
                return None;
            }
            let pts = m
                .iter()
                .map(|&(a, b)| {
                    let ka = imgs[i].features.keypoints[a];
                    let kb = imgs[j].features.keypoints[b];
                    (centered(&imgs[i], ka.x, ka.y), centered(&imgs[j], kb.x, kb.y))
                })
                .collect();
            Some((i, j, pts))
        })
        .collect();
    cancel.check()?;

    // ---------- focal lengths ----------
    let mut focal = vec![0f64; n];
    let mut source = FocalSource::Default;
    if let Some(hfov) = st.manual_hfov {
        for (k, im) in imgs.iter().enumerate() {
            focal[k] = st.lens.focal_from_hfov(hfov, im.full_w as f64);
        }
        source = FocalSource::Manual;
    } else if imgs.iter().all(|im| im.exif_focal.is_some()) {
        for (k, im) in imgs.iter().enumerate() {
            focal[k] = im.exif_focal.unwrap();
        }
        source = FocalSource::Exif;
    } else {
        let mut est = Vec::new();
        if st.lens == LensType::Rectilinear {
            let thr = st.ransac_threshold_px;
            let ests: Vec<f64> = raw
                .par_iter()
                .filter_map(|(i, j, pts)| {
                    let thr_full = thr / imgs[*i].scale.min(imgs[*j].scale);
                    let (h, inl) = ransac_homography(pts, thr_full, 1500)?;
                    if inl.len() < 20 {
                        return None;
                    }
                    focal_from_homography(&h)
                })
                .collect();
            est.extend(ests);
        }
        let known: Vec<f64> = imgs.iter().filter_map(|im| im.exif_focal).collect();
        for (k, im) in imgs.iter().enumerate() {
            focal[k] = if let Some(f) = im.exif_focal {
                f
            } else if !est.is_empty() {
                let mut e = est.clone();
                e.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let med = e[e.len() / 2];
                // estimates are in full-res pixels of the pair; rescale to this image width
                med
            } else if !known.is_empty() {
                known[0]
            } else {
                let hfov = if st.lens.is_fisheye() { 150.0 } else { 50.0 };
                st.lens.focal_from_hfov(hfov, im.full_w as f64)
            };
        }
        source = if !est.is_empty() { FocalSource::Estimated } else { FocalSource::Default };
    }
    progress.set_fraction(0.4);

    // ---------- robust rotation estimation per pair ----------
    let pairs: Vec<PairMatch> = raw
        .par_iter()
        .filter_map(|(i, j, pts)| {
            let thr = st.ransac_threshold_px / imgs[*i].scale.min(imgs[*j].scale);
            let (fi, fj) = (focal[*i], focal[*j]);
            let ang_thr = thr / fi.min(fj);
            let rays: Vec<(V3, V3)> = pts.iter().map(|(a, b)| (st.lens.ray(a[0], a[1], fi), st.lens.ray(b[0], b[1], fj))).collect();
            let (_r, inl) = ransac_rotation(&rays, ang_thr, 800)?;
            let need = (8.0 + 0.3 * pts.len() as f64).max(15.0);
            if (inl.len() as f64) < need {
                return None;
            }
            let conf = inl.len() as f64 / (8.0 + 0.3 * pts.len() as f64);
            let mut sel: Vec<([f64; 2], [f64; 2])> = inl.iter().map(|&k| pts[k]).collect();
            subsample_spread(&mut sel, 150);
            Some(PairMatch { i: *i, j: *j, pts: sel, confidence: conf })
        })
        .collect();
    cancel.check()?;
    progress.set_fraction(0.5);
    if pairs.is_empty() {
        return Err(StitchError::NoOverlap);
    }

    // ---------- maximum spanning tree on largest component ----------
    let mut weight = vec![vec![0f64; n]; n];
    for (pi, p) in pairs.iter().enumerate() {
        let w = p.pts.len() as f64 * p.confidence.min(3.0) + 1e-3 * pi as f64 * 0.0;
        weight[p.i][p.j] = w;
        weight[p.j][p.i] = w;
    }
    let comps = components(n, &pairs);
    let best_comp = comps.iter().max_by_key(|c| c.len()).cloned().unwrap_or_default();
    if best_comp.len() < 2 {
        return Err(StitchError::NoOverlap);
    }
    let in_comp: Vec<bool> = (0..n).map(|k| best_comp.contains(&k)).collect();
    let pairs: Vec<PairMatch> = pairs.into_iter().filter(|p| in_comp[p.i] && in_comp[p.j]).collect();

    let reference = *best_comp
        .iter()
        .max_by(|&&a, &&b| {
            let sa: f64 = weight[a].iter().sum();
            let sb: f64 = weight[b].iter().sum();
            sa.partial_cmp(&sb).unwrap()
        })
        .unwrap();

    let mut rots: Vec<Option<Rot>> = vec![None; n];
    rots[reference] = Some(Rot::identity());
    // Prim's algorithm
    loop {
        let mut best: Option<(usize, usize, f64)> = None;
        for a in 0..n {
            if rots[a].is_none() {
                continue;
            }
            for b in 0..n {
                if rots[b].is_some() || weight[a][b] <= 0.0 {
                    continue;
                }
                if best.map(|x| weight[a][b] > x.2).unwrap_or(true) {
                    best = Some((a, b, weight[a][b]));
                }
            }
        }
        let Some((a, b, _)) = best else { break };
        let p = pairs.iter().find(|p| (p.i == a && p.j == b) || (p.i == b && p.j == a)).unwrap();
        let rays: Vec<(V3, V3)> = p
            .pts
            .iter()
            .map(|(x, y)| (st.lens.ray(x[0], x[1], focal[p.i]), st.lens.ray(y[0], y[1], focal[p.j])))
            .collect();
        // R_rel maps rays of p.j onto rays of p.i
        let r_rel = kabsch(&rays, None);
        let ra = rots[a].unwrap();
        let rb = if p.i == a { ra * r_rel } else { ra * r_rel.inverse() };
        rots[b] = Some(rb);
    }

    // ---------- bundle adjustment ----------
    let mut groups: Vec<String> = Vec::new();
    let mut group_of = vec![0usize; n];
    for k in 0..n {
        let key = imgs[k].focal_key.clone();
        let g = match groups.iter().position(|x| *x == key) {
            Some(g) => g,
            None => {
                groups.push(key);
                groups.len() - 1
            }
        };
        group_of[k] = g;
    }
    let mut gfocal = vec![0f64; groups.len()];
    for g in 0..groups.len() {
        let v: Vec<f64> = (0..n).filter(|&k| group_of[k] == g).map(|k| focal[k]).collect();
        gfocal[g] = v.iter().sum::<f64>() / v.len() as f64;
    }
    let opt_focal = st.optimize_focal && source != FocalSource::Manual;
    let rms = bundle_adjust(&pairs, &mut rots, &mut gfocal, &group_of, reference, st.lens, opt_focal, progress, cancel)?;

    let mut cameras: Vec<Option<Camera>> = (0..n)
        .map(|k| {
            rots[k].map(|r| Camera { f: gfocal[group_of[k]], rot: r, w: imgs[k].full_w, h: imgs[k].full_h })
        })
        .collect();

    if st.straighten {
        wave_correct(&mut cameras);
    }
    center_yaw(&mut cameras);
    progress.set_fraction(1.0);
    Ok(Alignment { cameras, lens: st.lens, pairs, rms_px: rms, focal_source: source })
}

fn subsample_spread(v: &mut Vec<([f64; 2], [f64; 2])>, max: usize) {
    if v.len() <= max {
        return;
    }
    // deterministic stride subsampling after sorting by position for spatial spread
    v.sort_by(|a, b| (a.0[1] * 1e4 + a.0[0]).partial_cmp(&(b.0[1] * 1e4 + b.0[0])).unwrap());
    let step = v.len() as f64 / max as f64;
    let out: Vec<_> = (0..max).map(|k| v[(k as f64 * step) as usize]).collect();
    *v = out;
}

fn components(n: usize, pairs: &[PairMatch]) -> Vec<Vec<usize>> {
    let mut adj = vec![Vec::new(); n];
    for p in pairs {
        adj[p.i].push(p.j);
        adj[p.j].push(p.i);
    }
    let mut seen = vec![false; n];
    let mut out = Vec::new();
    for s in 0..n {
        if seen[s] {
            continue;
        }
        let mut comp = Vec::new();
        let mut q = VecDeque::from([s]);
        seen[s] = true;
        while let Some(a) = q.pop_front() {
            comp.push(a);
            for &b in &adj[a] {
                if !seen[b] {
                    seen[b] = true;
                    q.push_back(b);
                }
            }
        }
        out.push(comp);
    }
    out
}

/// Least-squares rotation R such that R * b ≈ a for ray pairs (a, b).
pub fn kabsch(rays: &[(V3, V3)], subset: Option<&[usize]>) -> Rot {
    let mut h = Matrix3::<f64>::zeros();
    let mut add = |a: &V3, b: &V3| {
        h += b * a.transpose();
    };
    match subset {
        Some(s) => s.iter().for_each(|&k| add(&rays[k].0, &rays[k].1)),
        None => rays.iter().for_each(|(a, b)| add(a, b)),
    }
    let svd = h.svd(true, true);
    let (Some(u), Some(vt)) = (svd.u, svd.v_t) else { return Rot::identity() };
    let v = vt.transpose();
    let mut d = Matrix3::identity();
    if (v * u.transpose()).determinant() < 0.0 {
        d[(2, 2)] = -1.0;
    }
    let r = v * d * u.transpose();
    Rot::from_matrix_unchecked(r)
}

fn lcg(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed >> 33
}

fn ransac_rotation(rays: &[(V3, V3)], thr: f64, iters: usize) -> Option<(Rot, Vec<usize>)> {
    let n = rays.len();
    if n < 3 {
        return None;
    }
    let mut seed = 0x1234_5678u64 ^ n as u64;
    let mut best: Vec<usize> = Vec::new();
    let mut max_iter = iters;
    let mut it = 0;
    while it < max_iter {
        it += 1;
        let a = (lcg(&mut seed) as usize) % n;
        let b = (lcg(&mut seed) as usize) % n;
        if a == b {
            continue;
        }
        if rays[a].0.dot(&rays[b].0) > 0.99995 {
            continue;
        }
        let r = kabsch(rays, Some(&[a, b]));
        let inl: Vec<usize> = (0..n).filter(|&k| (r * rays[k].1 - rays[k].0).norm() < thr).collect();
        if inl.len() > best.len() {
            best = inl;
            let ratio = best.len() as f64 / n as f64;
            let p_fail = 1.0 - ratio * ratio;
            if p_fail < 1e-9 {
                break;
            }
            let need = ((1e-3f64).ln() / p_fail.ln()).ceil() as usize;
            max_iter = max_iter.min(need.max(30));
        }
    }
    if best.len() < 3 {
        return None;
    }
    // refine twice
    let mut r = kabsch(rays, Some(&best));
    for _ in 0..2 {
        let inl: Vec<usize> = (0..n).filter(|&k| (r * rays[k].1 - rays[k].0).norm() < thr).collect();
        if inl.len() < 3 {
            break;
        }
        r = kabsch(rays, Some(&inl));
        best = inl;
    }
    Some((r, best))
}

/// Normalised DLT homography from >=4 point pairs mapping b -> a.
fn homography_dlt(pts: &[([f64; 2], [f64; 2])], idx: &[usize]) -> Option<Matrix3<f64>> {
    let norm = |sel: &dyn Fn(usize) -> [f64; 2]| -> Matrix3<f64> {
        let (mut mx, mut my) = (0.0, 0.0);
        for &k in idx {
            let p = sel(k);
            mx += p[0];
            my += p[1];
        }
        let c = idx.len() as f64;
        mx /= c;
        my /= c;
        let mut d = 0.0;
        for &k in idx {
            let p = sel(k);
            d += ((p[0] - mx).powi(2) + (p[1] - my).powi(2)).sqrt();
        }
        let s = (2f64).sqrt() / (d / c).max(1e-9);
        Matrix3::new(s, 0.0, -s * mx, 0.0, s, -s * my, 0.0, 0.0, 1.0)
    };
    let ta = norm(&|k| pts[k].0);
    let tb = norm(&|k| pts[k].1);
    let mut ata = nalgebra::SMatrix::<f64, 9, 9>::zeros();
    for &k in idx {
        let a = ta * nalgebra::Vector3::new(pts[k].0[0], pts[k].0[1], 1.0);
        let b = tb * nalgebra::Vector3::new(pts[k].1[0], pts[k].1[1], 1.0);
        let (x, y) = (b[0], b[1]);
        let (u, v) = (a[0], a[1]);
        let r1 = [-x, -y, -1.0, 0.0, 0.0, 0.0, u * x, u * y, u];
        let r2 = [0.0, 0.0, 0.0, -x, -y, -1.0, v * x, v * y, v];
        for r in [r1, r2] {
            for i in 0..9 {
                for j in 0..9 {
                    ata[(i, j)] += r[i] * r[j];
                }
            }
        }
    }
    let eig = ata.symmetric_eigen();
    let (mut mi, mut mv) = (0, f64::MAX);
    for i in 0..9 {
        if eig.eigenvalues[i] < mv {
            mv = eig.eigenvalues[i];
            mi = i;
        }
    }
    let h = eig.eigenvectors.column(mi);
    let hn = Matrix3::new(h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], h[8]);
    let ta_inv = ta.try_inverse()?;
    let hm = ta_inv * hn * tb;
    if hm[(2, 2)].abs() < 1e-12 {
        return None;
    }
    Some(hm / hm[(2, 2)])
}

fn h_err(h: &Matrix3<f64>, p: &([f64; 2], [f64; 2])) -> f64 {
    let b = h * nalgebra::Vector3::new(p.1[0], p.1[1], 1.0);
    if b[2].abs() < 1e-12 {
        return f64::MAX;
    }
    let (x, y) = (b[0] / b[2], b[1] / b[2]);
    ((x - p.0[0]).powi(2) + (y - p.0[1]).powi(2)).sqrt()
}

fn ransac_homography(pts: &[([f64; 2], [f64; 2])], thr: f64, iters: usize) -> Option<(Matrix3<f64>, Vec<usize>)> {
    let n = pts.len();
    if n < 8 {
        return None;
    }
    let mut seed = 0x9876_5432u64 ^ n as u64;
    let mut best: Vec<usize> = Vec::new();
    let mut max_iter = iters;
    let mut it = 0;
    while it < max_iter {
        it += 1;
        let mut s = [0usize; 4];
        let mut ok = true;
        for k in 0..4 {
            s[k] = (lcg(&mut seed) as usize) % n;
            for m in 0..k {
                if s[m] == s[k] {
                    ok = false;
                }
            }
        }
        if !ok {
            continue;
        }
        let Some(h) = homography_dlt(pts, &s) else { continue };
        let inl: Vec<usize> = (0..n).filter(|&k| h_err(&h, &pts[k]) < thr).collect();
        if inl.len() > best.len() {
            best = inl;
            let ratio = best.len() as f64 / n as f64;
            let p_fail = 1.0 - ratio.powi(4);
            if p_fail < 1e-9 {
                break;
            }
            let need = ((1e-3f64).ln() / p_fail.ln()).ceil() as usize;
            max_iter = max_iter.min(need.max(50));
        }
    }
    if best.len() < 8 {
        return None;
    }
    let h = homography_dlt(pts, &best)?;
    let inl: Vec<usize> = (0..n).filter(|&k| h_err(&h, &pts[k]) < thr).collect();
    Some((h, inl))
}

/// Focal estimate from a homography between centred image coordinates (OpenCV method).
fn focal_from_homography(h: &Matrix3<f64>) -> Option<f64> {
    let h = h / h[(2, 2)];
    let g = |r: usize, c: usize| h[(r, c)];
    let pick = |d1: f64, d2: f64, v1: f64, v2: f64| -> Option<f64> {
        let (v1, v2) = if v1 < v2 { (v2, v1) } else { (v1, v2) };
        if v1 > 0.0 && v2 > 0.0 {
            Some(if d1.abs() > d2.abs() { v1.sqrt() } else { v2.sqrt() })
        } else if v1 > 0.0 {
            Some(v1.sqrt())
        } else {
            None
        }
    };
    // f1
    let d1 = g(2, 0) * g(2, 1);
    let d2 = (g(2, 1) - g(2, 0)) * (g(2, 1) + g(2, 0));
    let v1 = -(g(0, 0) * g(0, 1) + g(1, 0) * g(1, 1)) / d1;
    let v2 = (g(0, 0) * g(0, 0) + g(1, 0) * g(1, 0) - g(0, 1) * g(0, 1) - g(1, 1) * g(1, 1)) / d2;
    let f1 = pick(d1, d2, v1, v2);
    // f0
    let d1 = g(0, 0) * g(1, 0) + g(0, 1) * g(1, 1);
    let d2 = g(0, 0) * g(0, 0) + g(0, 1) * g(0, 1) - g(1, 0) * g(1, 0) - g(1, 1) * g(1, 1);
    let v1 = -g(0, 2) * g(1, 2) / d1;
    let v2 = (g(1, 2) * g(1, 2) - g(0, 2) * g(0, 2)) / d2;
    let f0 = pick(d1, d2, v1, v2);
    match (f0, f1) {
        (Some(a), Some(b)) if a.is_finite() && b.is_finite() => Some((a * b).sqrt()),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn bundle_adjust(
    pairs: &[PairMatch],
    rots: &mut [Option<Rot>],
    gfocal: &mut [f64],
    group_of: &[usize],
    reference: usize,
    lens: LensType,
    opt_focal: bool,
    progress: &Progress,
    cancel: &Cancel,
) -> Result<f64, StitchError> {
    let n = rots.len();
    // parameter indices
    let mut rot_idx = vec![usize::MAX; n];
    let mut np = 0;
    for k in 0..n {
        if rots[k].is_some() && k != reference {
            rot_idx[k] = np;
            np += 3;
        }
    }
    let used_groups: Vec<usize> = {
        let mut v: Vec<usize> = (0..n).filter(|&k| rots[k].is_some()).map(|k| group_of[k]).collect();
        v.sort();
        v.dedup();
        v
    };
    let mut f_idx = vec![usize::MAX; gfocal.len()];
    if opt_focal {
        for &g in &used_groups {
            f_idx[g] = np;
            np += 1;
        }
    }
    let fscale = {
        let v: Vec<f64> = used_groups.iter().map(|&g| gfocal[g]).collect();
        v.iter().sum::<f64>() / v.len().max(1) as f64
    };
    let huber = 3.0; // pixels

    let residual = |rots: &[Option<Rot>], gf: &[f64], p: &PairMatch, m: &([f64; 2], [f64; 2])| -> V3 {
        let ri = rots[p.i].unwrap();
        let rj = rots[p.j].unwrap();
        let a = ri * lens.ray(m.0[0], m.0[1], gf[group_of[p.i]]);
        let b = rj * lens.ray(m.1[0], m.1[1], gf[group_of[p.j]]);
        (a - b) * fscale
    };
    let cost = |rots: &[Option<Rot>], gf: &[f64]| -> f64 {
        pairs
            .par_iter()
            .map(|p| {
                p.pts
                    .iter()
                    .map(|m| {
                        let e = residual(rots, gf, p, m).norm();
                        if e < huber {
                            e * e
                        } else {
                            2.0 * huber * e - huber * huber
                        }
                    })
                    .sum::<f64>()
            })
            .sum()
    };
    let count: usize = pairs.iter().map(|p| p.pts.len()).sum();
    let apply = |rots: &[Option<Rot>], gf: &[f64], delta: &DVector<f64>| -> (Vec<Option<Rot>>, Vec<f64>) {
        let mut r2 = rots.to_vec();
        for k in 0..n {
            if rot_idx[k] != usize::MAX {
                let i = rot_idx[k];
                let d = V3::new(delta[i], delta[i + 1], delta[i + 2]);
                r2[k] = Some(Rot::from_scaled_axis(d) * r2[k].unwrap());
            }
        }
        let mut f2 = gf.to_vec();
        for g in 0..gf.len() {
            if f_idx[g] != usize::MAX {
                f2[g] = gf[g] * delta[f_idx[g]].exp();
            }
        }
        (r2, f2)
    };

    let mut cur_cost = cost(rots, gfocal);
    let mut lambda = 1e-3;
    let eps = 1e-6;
    let max_iter = 60;
    for iter in 0..max_iter {
        cancel.check()?;
        progress.set_fraction(0.5 + 0.45 * iter as f32 / max_iter as f32);
        // Build normal equations
        let (jtj, jtr) = pairs
            .par_iter()
            .fold(
                || (DMatrix::<f64>::zeros(np, np), DVector::<f64>::zeros(np)),
                |(mut jtj, mut jtr), p| {
                    // local parameter list: (global index, kind)
                    let mut locals: Vec<(usize, u8, usize)> = Vec::new(); // (gidx, kind 0=rot i,1=rot j,2=f i,3=f j), axis
                    if rot_idx[p.i] != usize::MAX {
                        for a in 0..3 {
                            locals.push((rot_idx[p.i] + a, 0, a));
                        }
                    }
                    if rot_idx[p.j] != usize::MAX {
                        for a in 0..3 {
                            locals.push((rot_idx[p.j] + a, 1, a));
                        }
                    }
                    let gi = group_of[p.i];
                    let gj = group_of[p.j];
                    if f_idx[gi] != usize::MAX {
                        locals.push((f_idx[gi], 2, 0));
                    }
                    if f_idx[gj] != usize::MAX && gj != gi {
                        locals.push((f_idx[gj], 3, 0));
                    }
                    let ri = rots[p.i].unwrap();
                    let rj = rots[p.j].unwrap();
                    for m in &p.pts {
                        let fi = gfocal[gi];
                        let fj = gfocal[gj];
                        let ray_i = lens.ray(m.0[0], m.0[1], fi);
                        let ray_j = lens.ray(m.1[0], m.1[1], fj);
                        let a = ri * ray_i;
                        let b = rj * ray_j;
                        let r = (a - b) * fscale;
                        let e = r.norm();
                        let w = if e < huber { 1.0 } else { huber / e };
                        let mut cols: Vec<(usize, V3)> = Vec::with_capacity(locals.len());
                        for &(gidx, kind, axis) in &locals {
                            let mut dv = V3::zeros();
                            dv[axis] = eps;
                            let dr = Rot::from_scaled_axis(dv);
                            let col = match kind {
                                0 => ((dr * a) - a) * fscale / eps,
                                1 => -((dr * b) - b) * fscale / eps,
                                2 => {
                                    let fi2 = fi * eps.exp();
                                    let a2 = ri * lens.ray(m.0[0], m.0[1], fi2);
                                    let mut c = (a2 - a) * fscale / eps;
                                    if gj == gi {
                                        let b2 = rj * lens.ray(m.1[0], m.1[1], fi2);
                                        c -= (b2 - b) * fscale / eps;
                                    }
                                    c
                                }
                                _ => {
                                    let fj2 = fj * eps.exp();
                                    let b2 = rj * lens.ray(m.1[0], m.1[1], fj2);
                                    -(b2 - b) * fscale / eps
                                }
                            };
                            cols.push((gidx, col));
                        }
                        for (x, (gx, cx)) in cols.iter().enumerate() {
                            jtr[*gx] += w * cx.dot(&r);
                            for (gy, cy) in cols.iter().skip(x) {
                                let v = w * cx.dot(cy);
                                jtj[(*gx, *gy)] += v;
                                if gx != gy {
                                    jtj[(*gy, *gx)] += v;
                                }
                            }
                        }
                    }
                    (jtj, jtr)
                },
            )
            .reduce(
                || (DMatrix::<f64>::zeros(np, np), DVector::<f64>::zeros(np)),
                |(a1, b1), (a2, b2)| (a1 + a2, b1 + b2),
            );
        if np == 0 {
            break;
        }
        let mut improved = false;
        for _ in 0..10 {
            let mut a = jtj.clone();
            for d in 0..np {
                a[(d, d)] += lambda * (jtj[(d, d)] + 1e-9);
            }
            let Some(chol) = a.cholesky() else {
                lambda *= 10.0;
                continue;
            };
            let delta = -chol.solve(&jtr);
            let (r2, f2) = apply(rots, gfocal, &delta);
            let c2 = cost(&r2, &f2);
            if c2 < cur_cost {
                let rel = (cur_cost - c2) / cur_cost.max(1e-12);
                rots.copy_from_slice(&r2);
                gfocal.copy_from_slice(&f2);
                cur_cost = c2;
                lambda = (lambda * 0.3).max(1e-9);
                improved = true;
                if rel < 1e-7 {
                    return Ok((cur_cost / count.max(1) as f64).sqrt());
                }
                break;
            } else {
                lambda *= 10.0;
            }
        }
        if !improved {
            break;
        }
    }
    Ok((cur_cost / count.max(1) as f64).sqrt())
}

/// Straighten the panorama so the horizon is level (OpenCV-style horizontal wave correction).
pub fn wave_correct(cams: &mut [Option<Camera>]) {
    let list: Vec<&Camera> = cams.iter().flatten().collect();
    if list.len() < 2 {
        return;
    }
    let mut moment = Matrix3::<f64>::zeros();
    let mut ysum = V3::zeros();
    let mut zsum = V3::zeros();
    for c in &list {
        let m = c.rot.matrix();
        let x = m.column(0).into_owned();
        moment += x * x.transpose();
        ysum += m.column(1).into_owned();
        zsum += m.column(2).into_owned();
    }
    let eig = moment.symmetric_eigen();
    let (mut mi, mut mv) = (0, f64::MAX);
    for i in 0..3 {
        if eig.eigenvalues[i] < mv {
            mv = eig.eigenvalues[i];
            mi = i;
        }
    }
    let mut up: V3 = eig.eigenvectors.column(mi).into_owned();
    // When all cameras share nearly the same x axis (single image column), keep current vertical.
    let sorted = {
        let mut e: Vec<f64> = eig.eigenvalues.iter().cloned().collect();
        e.sort_by(|a, b| a.partial_cmp(b).unwrap());
        e
    };
    if sorted[1] < 1e-3 * sorted[2] {
        up = ysum.normalize();
    }
    if up.dot(&ysum) < 0.0 {
        up = -up;
    }
    let mut zh = zsum;
    zh -= up * up.dot(&zh);
    if zh.norm() < 1e-6 {
        zh = list[0].rot.matrix().column(2).into_owned();
        zh -= up * up.dot(&zh);
    }
    let z = zh.normalize();
    let x = up.cross(&z).normalize();
    let z = x.cross(&up).normalize();
    // rows of the correction matrix are the new axes
    let rg = Matrix3::from_rows(&[x.transpose(), up.transpose(), z.transpose()]);
    let rg = Rot::from_matrix_unchecked(rg);
    for c in cams.iter_mut().flatten() {
        c.rot = rg * c.rot;
    }
}

/// Rotate around the vertical axis so the panorama's coverage is centred (largest gap at ±180°).
pub fn center_yaw(cams: &mut [Option<Camera>]) {
    let mut lons: Vec<f64> = cams
        .iter()
        .flatten()
        .map(|c| {
            let z = c.rot * V3::new(0.0, 0.0, 1.0);
            z.x.atan2(z.z)
        })
        .collect();
    if lons.is_empty() {
        return;
    }
    lons.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let tau = std::f64::consts::TAU;
    let mut best_gap = -1.0;
    let mut gap_mid = 0.0;
    for k in 0..lons.len() {
        let a = lons[k];
        let b = if k + 1 < lons.len() { lons[k + 1] } else { lons[0] + tau };
        let g = b - a;
        if g > best_gap {
            best_gap = g;
            gap_mid = a + g / 2.0;
        }
    }
    let center = gap_mid + std::f64::consts::PI;
    let r = Rot::from_axis_angle(&V3::y_axis(), -center);
    for c in cams.iter_mut().flatten() {
        c.rot = r * c.rot;
    }
}
