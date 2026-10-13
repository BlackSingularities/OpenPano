//! RAW development: black/white normalisation, white balance, demosaicing (Malvar-He-Cutler for
//! Bayer sensors, generic neighbourhood interpolation for X-Trans and other patterns) and
//! conversion from camera space to linear sRGB / Rec.709 primaries.

use crate::imaging::RgbImage;
use anyhow::{anyhow, Result};
use rawloader::{RawImage, RawImageData, CFA};
use rayon::prelude::*;
use std::path::Path;

pub const RAW_EXTENSIONS: &[&str] = &[
    "arw", "srf", "sr2", "cr2", "crw", "nef", "nrw", "dng", "raf", "orf", "rw2", "pef", "srw", "3fr",
    "erf", "kdc", "dcr", "dcs", "mrw", "mos", "iiq", "mef", "rwl", "ari", "x3f",
];

pub fn is_raw_path(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| RAW_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

pub struct RawInfo {
    pub make: String,
    pub model: String,
    pub orientation: u32,
    /// Full-resolution developed size (before orientation is applied).
    pub full_w: usize,
    pub full_h: usize,
}

/// Decode and develop a RAW file into linear sRGB-primaries data, white balanced, where 1.0 is
/// sensor clipping. When `half` is true a fast half-resolution development is used.
pub fn load_raw(path: &Path, half: bool) -> Result<(RgbImage, RawInfo)> {
    let raw = rawloader::decode_file(path).map_err(|e| anyhow!("RAW: {}", e))?;
    let info = RawInfo {
        make: raw.clean_make.clone(),
        model: raw.clean_model.clone(),
        orientation: orientation_to_exif(&raw.orientation),
        full_w: raw.width.saturating_sub(raw.crops[1] + raw.crops[3]),
        full_h: raw.height.saturating_sub(raw.crops[0] + raw.crops[2]),
    };
    let img = develop(&raw, half)?;
    Ok((img, info))
}

fn orientation_to_exif(o: &rawloader::Orientation) -> u32 {
    use rawloader::Orientation::*;
    match o {
        Normal | Unknown => 1,
        HorizontalFlip => 2,
        Rotate180 => 3,
        VerticalFlip => 4,
        Transpose => 5,
        Rotate90 => 6,
        Transverse => 7,
        Rotate270 => 8,
    }
}

fn wb_coeffs(raw: &RawImage) -> [f32; 4] {
    let mut wb = raw.wb_coeffs;
    if !wb[0].is_finite() || !wb[1].is_finite() || !wb[2].is_finite() || wb[0] <= 0.0 || wb[1] <= 0.0 || wb[2] <= 0.0 {
        wb = raw.neutralwb();
    }
    if !(wb[3].is_finite()) || wb[3] <= 0.0 {
        wb[3] = wb[1];
    }
    let g = wb[1].max(1e-6);
    let mut out = [wb[0] / g, 1.0, wb[2] / g, wb[3] / g];
    // Ensure the smallest coefficient is 1 so clipping to 1.0 keeps highlights neutral.
    let mn = out.iter().cloned().fold(f32::MAX, f32::min).max(1e-6);
    for v in out.iter_mut() {
        *v /= mn;
    }
    out
}

/// Camera RGB(E) -> linear sRGB matrix, rows normalised so a white-balanced neutral stays neutral.
fn cam_to_srgb(raw: &RawImage) -> [[f32; 4]; 3] {
    const XYZ_TO_SRGB: [[f32; 3]; 3] = [
        [3.240_454_2, -1.537_138_5, -0.498_531_4],
        [-0.969_266, 1.876_010_8, 0.041_556],
        [0.055_643_4, -0.204_025_9, 1.057_225_2],
    ];
    let has_matrix = raw.xyz_to_cam.iter().any(|r| r.iter().any(|v| *v != 0.0));
    if !has_matrix {
        return [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
    }
    let c2x = raw.cam_to_xyz_normalized();
    let mut m = [[0.0f32; 4]; 3];
    for i in 0..3 {
        for j in 0..4 {
            let mut s = 0.0;
            for k in 0..3 {
                s += XYZ_TO_SRGB[i][k] * c2x[k][j];
            }
            m[i][j] = s;
        }
    }
    // Columns for unused colour index (3-colour sensors) must not contribute.
    for row in m.iter_mut() {
        let sum: f32 = row.iter().sum();
        if sum.abs() > 1e-6 {
            for v in row.iter_mut() {
                *v /= sum;
            }
        }
    }
    if m.iter().flatten().any(|v| !v.is_finite()) {
        return [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
    }
    m
}

pub fn develop(raw: &RawImage, half: bool) -> Result<RgbImage> {
    let [ct, cr, cb, cl] = raw.crops;
    let (fw, fh) = (raw.width, raw.height);
    if cl + cr >= fw || ct + cb >= fh {
        return Err(anyhow!("invalid RAW crop"));
    }
    let w = fw - cl - cr;
    let h = fh - ct - cb;
    let cpp = raw.cpp.max(1);
    let wb = wb_coeffs(raw);
    let mut black = [0f32; 4];
    let mut range = [1f32; 4];
    for i in 0..4 {
        black[i] = raw.blacklevels[i] as f32;
        range[i] = (raw.whitelevels[i] as f32 - black[i]).max(1.0);
    }
    let get = |idx: usize| -> f32 {
        match &raw.data {
            RawImageData::Integer(v) => v[idx] as f32,
            RawImageData::Float(v) => v[idx],
        }
    };
    let float_data = matches!(raw.data, RawImageData::Float(_));
    let m = cam_to_srgb(raw);

    if cpp >= 3 {
        // Already demosaiced (linear DNG etc.)
        let mut out = RgbImage::new(w, h);
        out.data.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
            for (x, px) in row.iter_mut().enumerate() {
                let base = ((y + ct) * fw + (x + cl)) * cpp;
                let mut c = [0f32; 4];
                for k in 0..3 {
                    let v = get(base + k);
                    let n = if float_data && raw.whitelevels[k] <= 1 { v } else { (v - black[k]) / range[k] };
                    c[k] = (n * wb[k]).clamp(0.0, 1.0);
                }
                *px = apply_matrix(&m, c);
            }
        });
        return Ok(out);
    }

    // Normalised, white-balanced mosaic
    let cfa = raw.cropped_cfa();
    let mut mosaic = vec![0f32; w * h];
    mosaic.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, v) in row.iter_mut().enumerate() {
            let c = cfa.color_at(y, x).min(3);
            let raw_v = get((y + ct) * fw + (x + cl));
            let n = if float_data && raw.whitelevels[c] <= 1 { raw_v } else { (raw_v - black[c]) / range[c] };
            *v = (n * wb[c]).clamp(0.0, 1.0);
        }
    });

    let is_bayer = cfa.width == 2 && cfa.height == 2 && {
        let mut cnt = [0; 4];
        for y in 0..2 {
            for x in 0..2 {
                cnt[cfa.color_at(y, x).min(3)] += 1;
            }
        }
        cnt[0] == 1 && cnt[2] == 1 && (cnt[1] + cnt[3]) == 2
    };

    if half {
        return Ok(develop_half(&mosaic, w, h, &cfa, &m));
    }
    if is_bayer {
        Ok(demosaic_mhc(&mosaic, w, h, &cfa, &m))
    } else {
        Ok(demosaic_generic(&mosaic, w, h, &cfa, &m))
    }
}

#[inline]
fn apply_matrix(m: &[[f32; 4]; 3], c: [f32; 4]) -> [f32; 3] {
    let mut o = [0f32; 3];
    for i in 0..3 {
        o[i] = (m[i][0] * c[0] + m[i][1] * c[1] + m[i][2] * c[2] + m[i][3] * c[3]).max(0.0);
    }
    o
}

/// Map colour index: green duplicates (index 3 in some RGBG descriptions) fold into green when
/// the matrix has no 4th colour.
#[inline]
fn color_idx(cfa: &CFA, y: usize, x: usize) -> usize {
    cfa.color_at(y, x).min(3)
}

fn develop_half(mosaic: &[f32], w: usize, h: usize, cfa: &CFA, m: &[[f32; 4]; 3]) -> RgbImage {
    let pw = cfa.width.max(2);
    let ph = cfa.height.max(2);
    // Use 2x2 blocks for Bayer; for larger patterns use pattern-sized blocks with partial averages.
    let bw = if pw == 2 { 2 } else { 3 };
    let bh = if ph == 2 { 2 } else { 3 };
    let nw = w / bw;
    let nh = h / bh;
    let mut out = RgbImage::new(nw, nh);
    out.data.par_chunks_mut(nw).enumerate().for_each(|(by, row)| {
        for (bx, px) in row.iter_mut().enumerate() {
            let mut acc = [0f32; 4];
            let mut cnt = [0f32; 4];
            for dy in 0..bh {
                for dx in 0..bw {
                    let y = by * bh + dy;
                    let x = bx * bw + dx;
                    let c = color_idx(cfa, y, x);
                    acc[c] += mosaic[y * w + x];
                    cnt[c] += 1.0;
                }
            }
            let mut c4 = [0f32; 4];
            for k in 0..4 {
                c4[k] = if cnt[k] > 0.0 { acc[k] / cnt[k] } else { 0.0 };
            }
            if cnt[3] == 0.0 {
                c4[3] = 0.0;
            }
            *px = apply_matrix(m, c4);
        }
    });
    out
}


#[inline]
fn clampi(v: isize, max: usize) -> usize {
    if v < 0 {
        (-v).min(max as isize - 1) as usize
    } else if v as usize >= max {
        let r = 2 * (max as isize - 1) - v;
        r.max(0) as usize
    } else {
        v as usize
    }
}

fn demosaic_mhc(mosaic: &[f32], w: usize, h: usize, cfa: &CFA, m: &[[f32; 4]; 3]) -> RgbImage {
    let mut out = RgbImage::new(w, h);
    let at = |x: isize, y: isize| -> f32 { mosaic[clampi(y, h) * w + clampi(x, w)] };
    // Determine which channel index holds the "second green" for 4-colour descriptions.
    out.data.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let yi = y as isize;
        for (x, px) in row.iter_mut().enumerate() {
            let xi = x as isize;
            let c = color_idx(cfa, y, x);
            let c0 = at(xi, yi);
            let n = at(xi, yi - 1);
            let s = at(xi, yi + 1);
            let e = at(xi + 1, yi);
            let wv = at(xi - 1, yi);
            let nn = at(xi, yi - 2);
            let ss = at(xi, yi + 2);
            let ee = at(xi + 2, yi);
            let ww = at(xi - 2, yi);
            let ne = at(xi + 1, yi - 1);
            let nw = at(xi - 1, yi - 1);
            let se = at(xi + 1, yi + 1);
            let sw = at(xi - 1, yi + 1);
            let diag = ne + nw + se + sw;
            let axial2 = nn + ss + ee + ww;
            let (r, g, b);
            match c {
                0 | 2 => {
                    let gg = (4.0 * c0 + 2.0 * (n + s + e + wv) - axial2) / 8.0;
                    let other = (6.0 * c0 + 2.0 * diag - 1.5 * axial2) / 8.0;
                    g = gg;
                    if c == 0 {
                        r = c0;
                        b = other;
                    } else {
                        b = c0;
                        r = other;
                    }
                }
                _ => {
                    let horiz = (5.0 * c0 + 4.0 * (wv + e) - diag - (ww + ee) + 0.5 * (nn + ss)) / 8.0;
                    let vert = (5.0 * c0 + 4.0 * (n + s) - diag - (nn + ss) + 0.5 * (ww + ee)) / 8.0;
                    g = c0;
                    let left = color_idx(cfa, y, clampi(xi - 1, w));
                    if left == 0 {
                        r = horiz;
                        b = vert;
                    } else {
                        b = horiz;
                        r = vert;
                    }
                }
            }
            let c4 = [r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0), 0.0];
            // Fold any 4th-colour weight into green for Bayer sensors described as RGBG/ERGB.
            let mm = [
                [m[0][0], m[0][1] + m[0][3], m[0][2], 0.0],
                [m[1][0], m[1][1] + m[1][3], m[1][2], 0.0],
                [m[2][0], m[2][1] + m[2][3], m[2][2], 0.0],
            ];
            *px = apply_matrix(&mm, c4);
        }
    });
    out
}

fn demosaic_generic(mosaic: &[f32], w: usize, h: usize, cfa: &CFA, m: &[[f32; 4]; 3]) -> RgbImage {
    let mut out = RgbImage::new(w, h);
    out.data.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, px) in row.iter_mut().enumerate() {
            let own = color_idx(cfa, y, x);
            let mut acc = [0f32; 4];
            let mut wsum = [0f32; 4];
            for radius in [1isize, 2] {
                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        if radius == 2 && dx.abs() < 2 && dy.abs() < 2 {
                            continue;
                        }
                        let yy = clampi(y as isize + dy, h);
                        let xx = clampi(x as isize + dx, w);
                        let c = color_idx(cfa, yy, xx);
                        let wt = 1.0 / (1.0 + (dx * dx + dy * dy) as f32);
                        acc[c] += mosaic[yy * w + xx] * wt;
                        wsum[c] += wt;
                    }
                }
                if wsum[0] > 0.0 && wsum[1] > 0.0 && wsum[2] > 0.0 {
                    break;
                }
            }
            let mut c4 = [0f32; 4];
            for k in 0..4 {
                c4[k] = if wsum[k] > 0.0 { acc[k] / wsum[k] } else { 0.0 };
            }
            c4[own] = mosaic[y * w + x];
            *px = apply_matrix(m, c4);
        }
    });
    out
}
