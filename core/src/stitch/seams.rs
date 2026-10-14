//! Ghost-aware seam finding. Starting from distance-based (Voronoi-like) labels, every image is
//! cut against the rest with a binary graph cut on a reduced canvas: seams are routed where the
//! overlapping images agree, so moving objects (people, cars, waves) are taken from one image
//! instead of being blended into ghosts.

use super::align::Alignment;
use super::photometric::Photometric;
use super::render::{Canvas, Source, NONE_LABEL};
use super::{Cancel, StitchError};
use rayon::prelude::*;

struct LowImg {
    x0: usize,
    y0: usize,
    w: usize,
    h: usize,
    col: Vec<[f32; 3]>,
    weight: Vec<f32>, // 0 = invalid
}

impl LowImg {
    #[inline]
    fn at(&self, x: usize, y: usize) -> Option<([f32; 3], f32)> {
        if x < self.x0 || y < self.y0 || x >= self.x0 + self.w || y >= self.y0 + self.h {
            return None;
        }
        let i = (y - self.y0) * self.w + (x - self.x0);
        let w = self.weight[i];
        if w > 0.0 {
            Some((self.col[i], w))
        } else {
            None
        }
    }
}

/// Refines `label` (full-resolution, one image index per canvas pixel) in place.
#[allow(clippy::too_many_arguments)]
pub fn refine(
    label: &mut [u16],
    canvas: &Canvas,
    al: &Alignment,
    photo: &Photometric,
    circular: bool,
    order: &[usize],
    src: &(dyn Fn(usize) -> Option<Source> + Sync),
    cancel: &Cancel,
) -> Result<(), StitchError> {
    let (cw, ch) = (canvas.w, canvas.h);
    if order.len() < 2 {
        return Ok(());
    }
    let q = (((cw * ch) as f64 / 0.5e6).sqrt().ceil() as usize).max(1);
    let lw = cw.div_ceil(q);
    let lh = ch.div_ceil(q);
    let off = (q as f64 - 1.0) / 2.0;

    // ---- warp every image to the low-res canvas (gamma-encoded, photometrically corrected)
    let mut low: Vec<Option<LowImg>> = (0..al.cameras.len()).map(|_| None).collect();
    for &i in order {
        cancel.check()?;
        let (Some(cam), Some(bx), Some(s)) = (al.cameras[i].as_ref(), canvas.boxes[i], src(i)) else { continue };
        let mapper = super::render::Mapper::new(cam, al.lens, circular);
        let (bx0, by0, bx1, by1) = bx;
        let (x0, y0) = (bx0 / q, by0 / q);
        let (x1, y1) = (bx1.div_ceil(q).min(lw), by1.div_ceil(q).min(lh));
        let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
        if w == 0 || h == 0 {
            continue;
        }
        let gains = photo.gains[i];
        let hd2 = ((cam.w * cam.w + cam.h * cam.h) as f64) / 4.0;
        let (sw, sh) = (s.img.w as f64, s.img.h as f64);
        let mut col = vec![[0f32; 3]; w * h];
        let mut weight = vec![0f32; w * h];
        col.par_chunks_mut(w).zip(weight.par_chunks_mut(w)).enumerate().for_each(|(ry, (cr, wr))| {
            let fy = (y0 + ry) as f64 * q as f64 + off;
            for rx in 0..w {
                let fx = (x0 + rx) as f64 * q as f64 + off;
                let Some(d) = canvas.pixel_dir(fx, fy) else { continue };
                let Some((ix, iy, wt)) = mapper.map(&d) else { continue };
                let px = s.img.sample((ix * s.scale + (sw - 1.0) / 2.0) as f32, (iy * s.scale + (sh - 1.0) / 2.0) as f32);
                let Some(p) = px else { continue };
                let vf = photo.vig_factor((ix * ix + iy * iy) / hd2);
                cr[rx] = [
                    (p[0] * gains[0] * vf).max(0.0).sqrt(),
                    (p[1] * gains[1] * vf).max(0.0).sqrt(),
                    (p[2] * gains[2] * vf).max(0.0).sqrt(),
                ];
                wr[rx] = wt.max(1e-4);
            }
        });
        low[i] = Some(LowImg { x0, y0, w, h, col, weight });
    }

    // ---- initial low-res labels from the full-res distance labels
    let mut lab = vec![NONE_LABEL; lw * lh];
    let vor: Vec<u16>;
    {
        lab.par_chunks_mut(lw).enumerate().for_each(|(y, row)| {
            let fy = (y * q + q / 2).min(ch - 1);
            for (x, l) in row.iter_mut().enumerate() {
                let fx = (x * q + q / 2).min(cw - 1);
                *l = label[fy * cw + fx];
            }
        });
        vor = lab.clone();
    }

    // ---- one sweep of binary cuts: image k against the best alternative
    let lambda = 0.02f32; // preference for the distance-based label
    let eps = 0.004f32;
    for &k in order {
        cancel.check()?;
        let Some(lk) = low[k].as_ref() else { continue };
        let (bx0, by0, bw, bh) = (lk.x0, lk.y0, lk.w, lk.h);
        // per pixel in bbox: state 0 = fixed other/keep, 1 = fixed k, 2 = free
        let n = bw * bh;
        let mut state = vec![0u8; n];
        let mut other = vec![NONE_LABEL; n];
        let mut diff = vec![0f32; n];
        for ry in 0..bh {
            for rx in 0..bw {
                let (x, y) = (bx0 + rx, by0 + ry);
                let i = ry * bw + rx;
                let Some((ck, _)) = lk.at(x, y) else { continue };
                // best alternative: current label if not k, else highest-weight other image
                let cur = lab[y * lw + x];
                let mut alt = NONE_LABEL;
                if cur != NONE_LABEL && cur as usize != k {
                    alt = cur;
                } else {
                    let mut bw_ = 0.0;
                    for &j in order {
                        if j == k {
                            continue;
                        }
                        if let Some(Some((_, w))) = low[j].as_ref().map(|l| l.at(x, y)) {
                            if w > bw_ {
                                bw_ = w;
                                alt = j as u16;
                            }
                        }
                    }
                }
                if alt == NONE_LABEL {
                    state[i] = 1;
                    continue;
                }
                let co = low[alt as usize].as_ref().and_then(|l| l.at(x, y)).map(|v| v.0);
                let Some(co) = co else {
                    state[i] = 1;
                    continue;
                };
                state[i] = 2;
                other[i] = alt;
                diff[i] = (ck[0] - co[0]).abs() + (ck[1] - co[1]).abs() + (ck[2] - co[2]).abs();
            }
        }
        let free: Vec<usize> = (0..n).filter(|&i| state[i] == 2).collect();
        if free.is_empty() {
            for i in 0..n {
                if state[i] == 1 {
                    lab[(by0 + i / bw) * lw + bx0 + i % bw] = k as u16;
                }
            }
            continue;
        }
        let mut node = vec![u32::MAX; n];
        for (ni, &i) in free.iter().enumerate() {
            node[i] = ni as u32;
        }
        let nn = free.len();
        let (s_node, t_node) = (nn, nn + 1);
        let mut g = FlowGraph::new(nn + 2);
        let mut cap_s = vec![0f32; nn];
        let mut cap_t = vec![0f32; nn];
        for (ni, &i) in free.iter().enumerate() {
            let (x, y) = (bx0 + i % bw, by0 + i / bw);
            let v = vor[y * lw + x];
            if v as usize == k {
                cap_s[ni] += lambda; // choosing "other" costs lambda
            } else {
                cap_t[ni] += lambda;
            }
        }
        let smooth = |a: usize, b: usize| -> f32 { diff[a] + diff[b] + eps };
        for ry in 0..bh {
            for rx in 0..bw {
                let a = ry * bw + rx;
                for (dx, dy) in [(1usize, 0usize), (0, 1)] {
                    let (bx, by) = (rx + dx, ry + dy);
                    if bx >= bw || by >= bh {
                        continue;
                    }
                    let b = by * bw + bx;
                    let (sa, sb) = (state[a], state[b]);
                    if sa == 2 && sb == 2 {
                        let w = smooth(a, b);
                        g.add_edge(node[a] as usize, node[b] as usize, w, w);
                    } else if sa == 2 || sb == 2 {
                        let (f, fixed) = if sa == 2 { (a, sb) } else { (b, sa) };
                        let w = diff[f] * 2.0 + eps;
                        if fixed == 1 {
                            cap_s[node[f] as usize] += w; // neighbour is k
                        } else {
                            cap_t[node[f] as usize] += w; // neighbour keeps other
                        }
                    }
                }
            }
        }
        for ni in 0..nn {
            if cap_s[ni] > 0.0 {
                g.add_edge(s_node, ni, cap_s[ni], 0.0);
            }
            if cap_t[ni] > 0.0 {
                g.add_edge(ni, t_node, cap_t[ni], 0.0);
            }
        }
        g.max_flow(s_node, t_node);
        let src_side = g.source_side(s_node);
        for i in 0..n {
            let (x, y) = (bx0 + i % bw, by0 + i / bw);
            match state[i] {
                1 => lab[y * lw + x] = k as u16,
                2 => {
                    lab[y * lw + x] = if src_side[node[i] as usize] { k as u16 } else { other[i] };
                }
                _ => {}
            }
        }
    }

    // ---- upsample: adopt the low-res label where that image is valid at full resolution
    let mappers: Vec<Option<super::render::Mapper>> = al
        .cameras
        .iter()
        .map(|c| c.as_ref().map(|c| super::render::Mapper::new(c, al.lens, circular)))
        .collect();
    label.par_chunks_mut(cw).enumerate().for_each(|(y, row)| {
        let ly = (y / q).min(lh - 1);
        for (x, l) in row.iter_mut().enumerate() {
            if *l == NONE_LABEL {
                continue;
            }
            let c = lab[ly * lw + (x / q).min(lw - 1)];
            if c == NONE_LABEL || c == *l {
                continue;
            }
            if let (Some(m), Some(d)) = (mappers[c as usize].as_ref(), canvas.pixel_dir(x as f64, y as f64)) {
                if m.map(&d).is_some() {
                    *l = c;
                }
            }
        }
    });
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// Dinic max-flow

struct FlowGraph {
    head: Vec<u32>,
    to: Vec<u32>,
    next: Vec<u32>,
    cap: Vec<f32>,
}

impl FlowGraph {
    fn new(n: usize) -> Self {
        Self { head: vec![u32::MAX; n], to: Vec::new(), next: Vec::new(), cap: Vec::new() }
    }

    fn add_edge(&mut self, a: usize, b: usize, c_ab: f32, c_ba: f32) {
        for (u, v, c) in [(a, b, c_ab), (b, a, c_ba)] {
            self.to.push(v as u32);
            self.cap.push(c);
            self.next.push(self.head[u]);
            self.head[u] = (self.to.len() - 1) as u32;
        }
    }

    fn bfs(&self, s: usize, t: usize, level: &mut [i32]) -> bool {
        level.iter_mut().for_each(|l| *l = -1);
        level[s] = 0;
        let mut q = std::collections::VecDeque::from([s]);
        while let Some(u) = q.pop_front() {
            let mut e = self.head[u];
            while e != u32::MAX {
                let v = self.to[e as usize] as usize;
                if self.cap[e as usize] > 1e-9 && level[v] < 0 {
                    level[v] = level[u] + 1;
                    q.push_back(v);
                }
                e = self.next[e as usize];
            }
        }
        level[t] >= 0
    }

    fn max_flow(&mut self, s: usize, t: usize) -> f32 {
        let n = self.head.len();
        let mut level = vec![-1i32; n];
        let mut total = 0f32;
        let mut it = vec![0u32; n];
        let mut rounds = 0;
        while self.bfs(s, t, &mut level) && rounds < 10_000 {
            rounds += 1;
            it.copy_from_slice(&self.head);
            loop {
                let f = self.dfs(s, t, f32::INFINITY, &level, &mut it);
                if f <= 1e-9 {
                    break;
                }
                total += f;
            }
        }
        total
    }

    /// Iterative DFS pushing one augmenting path along the level graph.
    fn dfs(&mut self, s: usize, t: usize, _limit: f32, level: &[i32], it: &mut [u32]) -> f32 {
        let mut path: Vec<u32> = Vec::new(); // edges
        let mut u = s;
        loop {
            if u == t {
                let mut f = f32::INFINITY;
                for &e in &path {
                    f = f.min(self.cap[e as usize]);
                }
                for &e in &path {
                    self.cap[e as usize] -= f;
                    self.cap[(e ^ 1) as usize] += f;
                }
                return f;
            }
            let mut advanced = false;
            while it[u] != u32::MAX {
                let e = it[u] as usize;
                let v = self.to[e] as usize;
                if self.cap[e] > 1e-9 && level[v] == level[u] + 1 {
                    path.push(e as u32);
                    u = v;
                    advanced = true;
                    break;
                }
                it[u] = self.next[e];
            }
            if !advanced {
                // dead end: retreat
                match path.pop() {
                    Some(e) => {
                        let prev = self.to[(e ^ 1) as usize] as usize;
                        it[prev] = self.next[e as usize];
                        u = prev;
                    }
                    None => return 0.0,
                }
            }
        }
    }

    fn source_side(&self, s: usize) -> Vec<bool> {
        let mut seen = vec![false; self.head.len()];
        seen[s] = true;
        let mut st = vec![s];
        while let Some(u) = st.pop() {
            let mut e = self.head[u];
            while e != u32::MAX {
                let v = self.to[e as usize] as usize;
                if !seen[v] && self.cap[e as usize] > 1e-9 {
                    seen[v] = true;
                    st.push(v);
                }
                e = self.next[e as usize];
            }
        }
        seen
    }
}
