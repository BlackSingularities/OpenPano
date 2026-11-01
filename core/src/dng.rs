//! Minimal DNG metadata reader for the parts rawloader does not expose: the OpcodeList2
//! GainMap opcodes phones and some cameras use for lens shading (vignetting / colour shading).

use std::path::Path;

/// One GainMap opcode (DNG 1.3, opcode id 9). Coordinates are in raw (stage 2) pixels.
#[derive(Clone, Debug)]
pub struct GainMap {
    pub top: u32,
    pub left: u32,
    pub bottom: u32,
    pub right: u32,
    pub row_pitch: u32,
    pub col_pitch: u32,
    pub points_v: usize,
    pub points_h: usize,
    pub spacing_v: f64,
    pub spacing_h: f64,
    pub origin_v: f64,
    pub origin_h: f64,
    pub map_planes: usize,
    pub gains: Vec<f32>,
}

impl GainMap {
    /// Gain for the raw pixel (row, col), or None when the pixel is outside this map's pattern.
    pub fn gain_at(&self, row: u32, col: u32) -> Option<f32> {
        if row < self.top || row >= self.bottom || col < self.left || col >= self.right {
            return None;
        }
        if (row - self.top) % self.row_pitch.max(1) != 0 || (col - self.left) % self.col_pitch.max(1) != 0 {
            return None;
        }
        let v = (row - self.top) as f64 / (self.bottom - self.top).max(1) as f64;
        let u = (col - self.left) as f64 / (self.right - self.left).max(1) as f64;
        let fy = if self.spacing_v > 0.0 { ((v - self.origin_v) / self.spacing_v).clamp(0.0, (self.points_v - 1) as f64) } else { 0.0 };
        let fx = if self.spacing_h > 0.0 { ((u - self.origin_h) / self.spacing_h).clamp(0.0, (self.points_h - 1) as f64) } else { 0.0 };
        let (y0, x0) = (fy.floor() as usize, fx.floor() as usize);
        let (y1, x1) = ((y0 + 1).min(self.points_v - 1), (x0 + 1).min(self.points_h - 1));
        let (ty, tx) = ((fy - y0 as f64) as f32, (fx - x0 as f64) as f32);
        let g = |y: usize, x: usize| self.gains[(y * self.points_h + x) * self.map_planes];
        let a = g(y0, x0) + (g(y0, x1) - g(y0, x0)) * tx;
        let b = g(y1, x0) + (g(y1, x1) - g(y1, x0)) * tx;
        Some(a + (b - a) * ty)
    }
}

/// All gain maps of the raw image; together they form a full-sensor gain field.
#[derive(Clone, Debug, Default)]
pub struct GainMaps(pub Vec<GainMap>);

impl GainMaps {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn gain(&self, row: u32, col: u32) -> f32 {
        let mut g = 1.0;
        for m in &self.0 {
            if let Some(v) = m.gain_at(row, col) {
                g *= v;
            }
        }
        g
    }
}

struct Tiff<'a> {
    b: &'a [u8],
    le: bool,
}

impl Tiff<'_> {
    fn u16(&self, o: usize) -> Option<u16> {
        let s = self.b.get(o..o + 2)?;
        Some(if self.le { u16::from_le_bytes([s[0], s[1]]) } else { u16::from_be_bytes([s[0], s[1]]) })
    }

    fn u32(&self, o: usize) -> Option<u32> {
        let s = self.b.get(o..o + 4)?;
        let a = [s[0], s[1], s[2], s[3]];
        Some(if self.le { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) })
    }

    /// Entries of the IFD at `off`: (tag, type, count, position of the value).
    fn entries(&self, off: usize) -> Vec<(u16, u16, u32, usize)> {
        let Some(n) = self.u16(off) else { return Vec::new() };
        (0..n as usize)
            .filter_map(|i| {
                let e = off + 2 + i * 12;
                let (tag, typ, cnt) = (self.u16(e)?, self.u16(e + 2)?, self.u32(e + 4)?);
                let unit = match typ {
                    1 | 2 | 6 | 7 => 1,
                    3 | 8 => 2,
                    4 | 9 | 11 | 13 => 4,
                    _ => 8,
                };
                let size = unit * cnt as usize;
                let pos = if size <= 4 { e + 8 } else { self.u32(e + 8)? as usize };
                Some((tag, typ, cnt, pos))
            })
            .collect()
    }

    fn longs(&self, typ: u16, cnt: u32, pos: usize) -> Vec<u32> {
        (0..cnt as usize)
            .filter_map(|i| if typ == 3 { self.u16(pos + i * 2).map(u32::from) } else { self.u32(pos + i * 4) })
            .collect()
    }
}

const TAG_SUBFILE: u16 = 254;
const TAG_SUBIFDS: u16 = 330;
const TAG_OPCODELIST2: u16 = 51009;

/// GainMap opcodes from OpcodeList2 of the main raw IFD (empty when absent or unreadable).
pub fn gain_maps(path: &Path) -> GainMaps {
    let Ok(bytes) = std::fs::read(path) else { return GainMaps::default() };
    parse_gain_maps(&bytes)
}

pub fn parse_gain_maps(bytes: &[u8]) -> GainMaps {
    let le = match bytes.get(0..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return GainMaps::default(),
    };
    let t = Tiff { b: bytes, le };
    let Some(ifd0) = t.u32(4) else { return GainMaps::default() };
    // IFD0 plus its SubIFDs; the raw image is the one with NewSubFileType == 0.
    let mut ifds = vec![ifd0 as usize];
    for (tag, typ, cnt, pos) in t.entries(ifd0 as usize) {
        if tag == TAG_SUBIFDS {
            ifds.extend(t.longs(typ, cnt, pos).into_iter().map(|v| v as usize));
        }
    }
    for off in ifds {
        let es = t.entries(off);
        let subfile = es.iter().find(|e| e.0 == TAG_SUBFILE).and_then(|e| t.longs(e.1, 1, e.3).first().copied()).unwrap_or(0);
        if subfile != 0 {
            continue;
        }
        if let Some(&(_, _, cnt, pos)) = es.iter().find(|e| e.0 == TAG_OPCODELIST2) {
            if let Some(data) = bytes.get(pos..pos + cnt as usize) {
                return GainMaps(parse_opcodes(data));
            }
        }
    }
    GainMaps::default()
}

/// Opcode lists are always big-endian.
fn parse_opcodes(d: &[u8]) -> Vec<GainMap> {
    let be32 = |o: usize| d.get(o..o + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]));
    let bef32 = |o: usize| d.get(o..o + 4).map(|s| f32::from_be_bytes([s[0], s[1], s[2], s[3]]));
    let bef64 = |o: usize| {
        d.get(o..o + 8).map(|s| {
            let mut a = [0u8; 8];
            a.copy_from_slice(s);
            f64::from_be_bytes(a)
        })
    };
    let mut out = Vec::new();
    let Some(n) = be32(0) else { return out };
    let mut o = 4;
    for _ in 0..n {
        let (Some(id), Some(size)) = (be32(o), be32(o + 12)) else { break };
        let p = o + 16;
        if id == 9 {
            let m = (|| {
                let points_v = be32(p + 32)? as usize;
                let points_h = be32(p + 36)? as usize;
                let map_planes = (be32(p + 72)? as usize).max(1);
                let count = points_v.checked_mul(points_h)?.checked_mul(map_planes)?;
                if points_v == 0 || points_h == 0 || count > 1 << 22 {
                    return None;
                }
                let gains = (0..count).map(|i| bef32(p + 76 + i * 4)).collect::<Option<Vec<f32>>>()?;
                Some(GainMap {
                    top: be32(p)?,
                    left: be32(p + 4)?,
                    bottom: be32(p + 8)?,
                    right: be32(p + 12)?,
                    row_pitch: be32(p + 24)?,
                    col_pitch: be32(p + 28)?,
                    points_v,
                    points_h,
                    spacing_v: bef64(p + 40)?,
                    spacing_h: bef64(p + 48)?,
                    origin_v: bef64(p + 56)?,
                    origin_h: bef64(p + 64)?,
                    map_planes,
                    gains,
                })
            })();
            if let Some(m) = m {
                out.push(m);
            }
        }
        o = p + size as usize;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opcode_gainmap(top: u32, left: u32, pitch: u32, gains: [f32; 4]) -> Vec<u8> {
        let mut p = Vec::new();
        for v in [top, left, 100, 100, 0, 1, pitch, pitch, 2, 2] {
            p.extend(v.to_be_bytes());
        }
        for v in [1.0f64, 1.0, 0.0, 0.0] {
            p.extend(v.to_be_bytes());
        }
        p.extend(1u32.to_be_bytes());
        for g in gains {
            p.extend(g.to_be_bytes());
        }
        let mut o = Vec::new();
        for v in [9u32, 0x0103_0000, 0, p.len() as u32] {
            o.extend(v.to_be_bytes());
        }
        o.extend(p);
        o
    }

    #[test]
    fn parses_gain_map_from_tiff() {
        let mut list = 1u32.to_be_bytes().to_vec();
        list.extend(opcode_gainmap(0, 0, 1, [1.0, 2.0, 1.0, 2.0]));
        // little-endian TIFF: header, IFD0 at 8 with one entry (OpcodeList2, UNDEFINED), data at 26
        let mut f = b"II*\0".to_vec();
        f.extend(8u32.to_le_bytes());
        f.extend(1u16.to_le_bytes());
        f.extend(TAG_OPCODELIST2.to_le_bytes());
        f.extend(7u16.to_le_bytes());
        f.extend((list.len() as u32).to_le_bytes());
        f.extend(26u32.to_le_bytes());
        f.extend(0u32.to_le_bytes());
        f.extend(list);
        let gm = parse_gain_maps(&f);
        assert_eq!(gm.0.len(), 1);
        assert!((gm.gain(0, 0) - 1.0).abs() < 1e-6);
        assert!((gm.gain(0, 99) - 1.99).abs() < 1e-3);
        assert!((gm.gain(50, 50) - 1.5).abs() < 1e-3);
    }

    #[test]
    fn pitch_selects_cfa_position() {
        let data = [1u32.to_be_bytes().to_vec(), opcode_gainmap(1, 0, 2, [3.0; 4])].concat();
        let m = &parse_opcodes(&data)[0];
        assert!(m.gain_at(0, 0).is_none());
        assert!(m.gain_at(1, 0).is_some());
        assert!(m.gain_at(1, 1).is_none());
        assert!(m.gain_at(3, 2).is_some());
    }
}
