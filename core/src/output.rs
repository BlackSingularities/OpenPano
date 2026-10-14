//! Writing results: JPEG (with optional GPano XMP for 360° viewers), 16-bit TIFF/PNG and linear
//! floating-point DNG.

use crate::imaging::{linear_to_srgb, RgbImage, ToSrgb8};
use crate::stitch::geometry::Projection;
use crate::stitch::render::RenderOutput;
use anyhow::{Context, Result};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputFormat {
    /// DNG when any source is RAW, JPEG otherwise.
    Auto,
    Jpeg,
    Tiff16,
    Png16,
    Dng,
}

impl OutputFormat {
    pub const ALL: [OutputFormat; 5] = [OutputFormat::Auto, OutputFormat::Jpeg, OutputFormat::Tiff16, OutputFormat::Png16, OutputFormat::Dng];

    pub fn resolve(self, any_raw: bool) -> OutputFormat {
        match self {
            OutputFormat::Auto => {
                if any_raw {
                    OutputFormat::Dng
                } else {
                    OutputFormat::Jpeg
                }
            }
            f => f,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Auto | OutputFormat::Jpeg => "jpg",
            OutputFormat::Tiff16 => "tif",
            OutputFormat::Png16 => "png",
            OutputFormat::Dng => "dng",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputSettings {
    pub format: OutputFormat,
    pub jpeg_quality: u8,
    pub transparent: bool,
    pub gpano: bool,
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self { format: OutputFormat::Auto, jpeg_quality: 95, transparent: false, gpano: true }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SourceInfo {
    pub make: String,
    pub model: String,
}

/// GPano XMP packet for equirectangular results (full or partial spheres).
pub fn gpano_xmp(out: &RenderOutput) -> Option<String> {
    let c = &out.canvas;
    if c.proj != Projection::Equirectangular {
        return None;
    }
    // GPano assumes a level horizon: only yaw rotations of the view are allowed.
    let up = c.view * crate::stitch::geometry::V3::new(0.0, -1.0, 0.0);
    if up.y > -0.9999 {
        return None;
    }
    let full_w = (2.0 * std::f64::consts::PI * c.scale).round() as i64;
    let full_h = (std::f64::consts::PI * c.scale).round() as i64;
    let left = ((c.u0 + std::f64::consts::PI) * c.scale).round() as i64 + out.crop.0 as i64;
    let top = ((c.v0 + std::f64::consts::FRAC_PI_2) * c.scale).round() as i64 + out.crop.1 as i64;
    let (w, h) = (out.image.w as i64, out.image.h as i64);
    let full_w = full_w.max(w);
    let full_h = full_h.max(h);
    let left = left.clamp(0, full_w - w);
    let top = top.clamp(0, full_h - h);
    Some(format!(
        concat!(
            "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>",
            "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Panoramer\">",
            "<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">",
            "<rdf:Description rdf:about=\"\" xmlns:GPano=\"http://ns.google.com/photos/1.0/panorama/\"",
            " GPano:UsePanoramaViewer=\"True\" GPano:ProjectionType=\"equirectangular\"",
            " GPano:CroppedAreaImageWidthPixels=\"{w}\" GPano:CroppedAreaImageHeightPixels=\"{h}\"",
            " GPano:FullPanoWidthPixels=\"{fw}\" GPano:FullPanoHeightPixels=\"{fh}\"",
            " GPano:CroppedAreaLeftPixels=\"{l}\" GPano:CroppedAreaTopPixels=\"{t}\"",
            " GPano:PoseHeadingDegrees=\"0.0\" GPano:StitchingSoftware=\"Panoramer {ver}\"/>",
            "</rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>"
        ),
        w = w,
        h = h,
        fw = full_w,
        fh = full_h,
        l = left,
        t = top,
        ver = env!("CARGO_PKG_VERSION")
    ))
}

pub fn save(path: &Path, out: &RenderOutput, fmt: OutputFormat, st: &OutputSettings, info: &SourceInfo) -> Result<()> {
    let xmp = if st.gpano { gpano_xmp(out) } else { None };
    let alpha = if st.transparent && out.alpha.iter().any(|a| *a == 0) { Some(out.alpha.as_slice()) } else { None };
    match fmt {
        OutputFormat::Auto | OutputFormat::Jpeg => save_jpeg(path, &out.image, st.jpeg_quality, xmp.as_deref()),
        OutputFormat::Png16 => save_16(path, &out.image, alpha, image::ImageFormat::Png),
        OutputFormat::Tiff16 => save_16(path, &out.image, alpha, image::ImageFormat::Tiff),
        OutputFormat::Dng => save_dng(path, &out.image, info, xmp.as_deref()),
    }
}

pub fn to_srgb8(img: &RgbImage) -> Vec<u8> {
    let conv = ToSrgb8::new();
    let mut buf = vec![0u8; img.w * img.h * 3];
    buf.par_chunks_mut(3).zip(img.data.par_iter()).for_each(|(o, p)| {
        o[0] = conv.conv(p[0]);
        o[1] = conv.conv(p[1]);
        o[2] = conv.conv(p[2]);
    });
    buf
}

pub fn save_jpeg(path: &Path, img: &RgbImage, quality: u8, xmp: Option<&str>) -> Result<()> {
    let buf = to_srgb8(img);
    let mut bytes: Vec<u8> = Vec::new();
    {
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality.clamp(1, 100));
        enc.encode(&buf, img.w as u32, img.h as u32, image::ExtendedColorType::Rgb8)?;
    }
    if let Some(x) = xmp {
        bytes = insert_jpeg_xmp(&bytes, x);
    }
    std::fs::write(path, &bytes).with_context(|| format!("{}", path.display()))?;
    Ok(())
}

/// Insert an XMP APP1 segment right after SOI (and after an APP0/JFIF segment if present).
fn insert_jpeg_xmp(jpeg: &[u8], xmp: &str) -> Vec<u8> {
    const NS: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
    let payload_len = NS.len() + xmp.len() + 2;
    if payload_len > 65535 || jpeg.len() < 4 {
        return jpeg.to_vec();
    }
    let mut pos = 2;
    if jpeg[2] == 0xFF && jpeg[3] == 0xE0 {
        let l = u16::from_be_bytes([jpeg[4], jpeg[5]]) as usize;
        pos = 4 + l;
    }
    let mut out = Vec::with_capacity(jpeg.len() + payload_len + 2);
    out.extend_from_slice(&jpeg[..pos]);
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&(payload_len as u16).to_be_bytes());
    out.extend_from_slice(NS);
    out.extend_from_slice(xmp.as_bytes());
    out.extend_from_slice(&jpeg[pos..]);
    out
}

fn save_16(path: &Path, img: &RgbImage, alpha: Option<&[u8]>, fmt: image::ImageFormat) -> Result<()> {
    let lut: Vec<u16> = (0..=65535u32).map(|i| (linear_to_srgb(i as f32 / 65535.0) * 65535.0 + 0.5) as u16).collect();
    let conv = |v: f32| lut[(v.clamp(0.0, 1.0) * 65535.0) as usize];
    let (w, h) = (img.w as u32, img.h as u32);
    match alpha {
        Some(a) => {
            let mut buf = vec![0u16; img.w * img.h * 4];
            buf.par_chunks_mut(4).zip(img.data.par_iter()).zip(a.par_iter()).for_each(|((o, p), al)| {
                o[0] = conv(p[0]);
                o[1] = conv(p[1]);
                o[2] = conv(p[2]);
                o[3] = if *al > 0 { 65535 } else { 0 };
            });
            let ib: image::ImageBuffer<image::Rgba<u16>, Vec<u16>> = image::ImageBuffer::from_raw(w, h, buf).context("buffer")?;
            ib.save_with_format(path, fmt).with_context(|| format!("{}", path.display()))?;
        }
        None => {
            let mut buf = vec![0u16; img.w * img.h * 3];
            buf.par_chunks_mut(3).zip(img.data.par_iter()).for_each(|(o, p)| {
                o[0] = conv(p[0]);
                o[1] = conv(p[1]);
                o[2] = conv(p[2]);
            });
            let ib: image::ImageBuffer<image::Rgb<u16>, Vec<u16>> = image::ImageBuffer::from_raw(w, h, buf).context("buffer")?;
            ib.save_with_format(path, fmt).with_context(|| format!("{}", path.display()))?;
        }
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// Minimal TIFF/DNG writer

enum Val {
    Byte(Vec<u8>),
    Ascii(String),
    Short(Vec<u16>),
    Long(Vec<u32>),
    Rational(Vec<(u32, u32)>),
    SRational(Vec<(i32, i32)>),
}

impl Val {
    fn type_count(&self) -> (u16, u32) {
        match self {
            Val::Byte(v) => (1, v.len() as u32),
            Val::Ascii(s) => (2, s.len() as u32 + 1),
            Val::Short(v) => (3, v.len() as u32),
            Val::Long(v) => (4, v.len() as u32),
            Val::Rational(v) => (5, v.len() as u32),
            Val::SRational(v) => (10, v.len() as u32),
        }
    }
    fn bytes(&self) -> Vec<u8> {
        let mut b = Vec::new();
        match self {
            Val::Byte(v) => b.extend_from_slice(v),
            Val::Ascii(s) => {
                b.extend_from_slice(s.as_bytes());
                b.push(0);
            }
            Val::Short(v) => v.iter().for_each(|x| b.extend_from_slice(&x.to_le_bytes())),
            Val::Long(v) => v.iter().for_each(|x| b.extend_from_slice(&x.to_le_bytes())),
            Val::Rational(v) => v.iter().for_each(|(n, d)| {
                b.extend_from_slice(&n.to_le_bytes());
                b.extend_from_slice(&d.to_le_bytes());
            }),
            Val::SRational(v) => v.iter().for_each(|(n, d)| {
                b.extend_from_slice(&n.to_le_bytes());
                b.extend_from_slice(&d.to_le_bytes());
            }),
        }
        b
    }
}

fn ascii_clean(s: &str) -> String {
    let t: String = s.chars().filter(|c| c.is_ascii() && !c.is_ascii_control()).collect();
    if t.is_empty() {
        "Unknown".into()
    } else {
        t
    }
}

pub fn save_dng(path: &Path, img: &RgbImage, info: &SourceInfo, xmp: Option<&str>) -> Result<()> {
    let (w, h) = (img.w, img.h);
    let rows_per_strip = 32usize;
    let nstrips = h.div_ceil(rows_per_strip);
    // Compress strips in parallel: half-float RGB, little endian, deflate.
    let strips: Vec<Vec<u8>> = (0..nstrips)
        .into_par_iter()
        .map(|s| {
            let y0 = s * rows_per_strip;
            let y1 = (y0 + rows_per_strip).min(h);
            let mut raw = Vec::with_capacity((y1 - y0) * w * 6);
            for p in &img.data[y0 * w..y1 * w] {
                for c in p {
                    raw.extend_from_slice(&half::f16::from_f32(c.max(0.0).min(65000.0)).to_le_bytes());
                }
            }
            let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
            enc.write_all(&raw).expect("deflate");
            enc.finish().expect("deflate")
        })
        .collect();

    let make = ascii_clean(&info.make);
    let model = ascii_clean(&info.model);
    // XYZ(D65) -> linear sRGB, i.e. the "camera" space of our data.
    let cm: [f64; 9] = [3.2404542, -1.5371385, -0.4985314, -0.9692660, 1.8760108, 0.0415560, 0.0556434, -0.2040259, 1.0572252];
    let srat = |v: f64| ((v * 10000.0).round() as i32, 10000);

    let mut entries: Vec<(u16, Val)> = vec![
        (254, Val::Long(vec![0])),
        (256, Val::Long(vec![w as u32])),
        (257, Val::Long(vec![h as u32])),
        (258, Val::Short(vec![16, 16, 16])),
        (259, Val::Short(vec![8])),
        (262, Val::Short(vec![34892])),
        (271, Val::Ascii(make.clone())),
        (272, Val::Ascii(model.clone())),
        (273, Val::Long(vec![0; nstrips])), // patched below
        (274, Val::Short(vec![1])),
        (277, Val::Short(vec![3])),
        (278, Val::Long(vec![rows_per_strip as u32])),
        (279, Val::Long(strips.iter().map(|s| s.len() as u32).collect())),
        (284, Val::Short(vec![1])),
        (305, Val::Ascii(format!("Panoramer {}", env!("CARGO_PKG_VERSION")))),
        (339, Val::Short(vec![3, 3, 3])),
        (50706, Val::Byte(vec![1, 4, 0, 0])),
        (50707, Val::Byte(vec![1, 4, 0, 0])),
        (50708, Val::Ascii(format!("{} {} (Panoramer)", make, model))),
        (50721, Val::SRational(cm.iter().map(|v| srat(*v)).collect())),
        (50728, Val::Rational(vec![(1, 1), (1, 1), (1, 1)])),
        (50730, Val::SRational(vec![(0, 1)])),
        (50778, Val::Short(vec![21])),
        (50936, Val::Ascii("Panoramer Linear sRGB".into())),
    ];
    if let Some(x) = xmp {
        entries.push((700, Val::Byte(x.as_bytes().to_vec())));
    }
    entries.sort_by_key(|e| e.0);

    // Layout: header(8) | IFD | out-of-line values | strips
    let n = entries.len();
    let ifd_size = 2 + n * 12 + 4;
    let mut extra: Vec<u8> = Vec::new();
    let extra_base = 8 + ifd_size;
    // compute value placement
    let mut value_offsets: Vec<Option<u32>> = Vec::with_capacity(n);
    for (_, v) in &entries {
        let b = v.bytes();
        if b.len() > 4 {
            if extra.len() % 2 == 1 {
                extra.push(0);
            }
            value_offsets.push(Some((extra_base + extra.len()) as u32));
            extra.extend_from_slice(&b);
        } else {
            value_offsets.push(None);
        }
    }
    if extra.len() % 2 == 1 {
        extra.push(0);
    }
    let data_base = extra_base + extra.len();
    // strip offsets
    let mut offs = Vec::with_capacity(nstrips);
    let mut cur = data_base as u64;
    for s in &strips {
        offs.push(cur);
        cur += s.len() as u64;
    }
    if cur > u32::MAX as u64 {
        anyhow::bail!("DNG > 4 GB is not supported; reduce output size");
    }
    let offs32: Vec<u32> = offs.iter().map(|v| *v as u32).collect();
    // patch StripOffsets value (it is out-of-line when nstrips > 1)
    let so_idx = entries.iter().position(|e| e.0 == 273).unwrap();
    entries[so_idx].1 = Val::Long(offs32.clone());
    if let Some(off) = value_offsets[so_idx] {
        let start = off as usize - extra_base;
        let b = entries[so_idx].1.bytes();
        extra[start..start + b.len()].copy_from_slice(&b);
    }

    let mut f = std::io::BufWriter::new(std::fs::File::create(path).with_context(|| format!("{}", path.display()))?);
    f.write_all(b"II")?;
    f.write_all(&42u16.to_le_bytes())?;
    f.write_all(&8u32.to_le_bytes())?;
    f.write_all(&(n as u16).to_le_bytes())?;
    for (k, (tag, v)) in entries.iter().enumerate() {
        let (ty, cnt) = v.type_count();
        f.write_all(&tag.to_le_bytes())?;
        f.write_all(&ty.to_le_bytes())?;
        f.write_all(&cnt.to_le_bytes())?;
        match value_offsets[k] {
            Some(o) => f.write_all(&o.to_le_bytes())?,
            None => {
                let mut b = v.bytes();
                b.resize(4, 0);
                f.write_all(&b)?;
            }
        }
    }
    f.write_all(&0u32.to_le_bytes())?;
    f.write_all(&extra)?;
    for s in &strips {
        f.write_all(s)?;
    }
    f.flush()?;
    Ok(())
}
