//! Optional GPU acceleration of the warp step (feature `gpu`, wgpu compute shader).
//!
//! The CPU computes the exact canvas → source mapping on a sparse grid (every `STEP` pixels);
//! the GPU interpolates it per pixel, samples the source image and applies the photometric
//! gains. Grid cells touching an unmapped node are flagged so the caller renders those pixels
//! with the exact CPU path – the result therefore never loses coverage at image borders.

use crate::imaging::RgbImage;
use std::sync::OnceLock;
use wgpu::util::DeviceExt;

/// Grid spacing in canvas pixels.
pub const STEP: usize = 4;

/// Output value alpha: >= 1 → valid (feather weight = alpha - 1), 0 → no coverage,
/// < 0 → compute this pixel on the CPU.
pub const NEEDS_CPU: f32 = -1.0;

pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    max_tex: u32,
    max_buf: u64,
    pub name: String,
}

pub struct Source {
    tex: wgpu::Texture,
    w: u32,
    h: u32,
}

pub struct WarpParams {
    /// Tile size in canvas pixels.
    pub tw: usize,
    pub th: usize,
    /// Grid nodes per row / column (node (i, j) sits at tile pixel (i·STEP, j·STEP)).
    pub gw: usize,
    pub gh: usize,
    /// work / full scale of the source
    pub scale: f32,
    pub gains: [f32; 3],
    pub vig: [f32; 2],
    /// squared half diagonal of the camera, for the vignetting radius
    pub hd2: f32,
    pub bicubic: bool,
}

static GPU: OnceLock<Option<Gpu>> = OnceLock::new();

/// Shared GPU context, or None when no usable adapter exists (then everything runs on the CPU).
pub fn get() -> Option<&'static Gpu> {
    GPU.get_or_init(|| pollster::block_on(Gpu::new())).as_ref()
}

const SHADER: &str = r#"
struct P {
    tile: vec4<f32>,  // tw, th, gw, gh
    band: vec4<f32>,  // row offset of this dispatch, step, scale, bicubic
    gains: vec4<f32>,
    vig: vec4<f32>,   // a, b, hd2, -
    src: vec4<f32>,   // width, height, -, -
};

@group(0) @binding(0) var<uniform> p: P;
@group(0) @binding(1) var<storage, read> grid: array<vec4<f32>>;
@group(0) @binding(2) var img: texture_2d<f32>;
@group(0) @binding(3) var<storage, read_write> outp: array<vec4<f32>>;

fn px(x: i32, y: i32) -> vec3<f32> {
    return textureLoad(img, vec2<i32>(x, y), 0).rgb;
}

fn bilinear(x: f32, y: f32) -> vec4<f32> {
    let w = i32(p.src.x);
    let h = i32(p.src.y);
    if (x < 0.0 || y < 0.0 || x > f32(w - 1) || y > f32(h - 1)) {
        return vec4<f32>(0.0);
    }
    let x0 = i32(floor(x));
    let y0 = i32(floor(y));
    let x1 = min(x0 + 1, w - 1);
    let y1 = min(y0 + 1, h - 1);
    let fx = x - f32(x0);
    let fy = y - f32(y0);
    let top = mix(px(x0, y0), px(x1, y0), fx);
    let bot = mix(px(x0, y1), px(x1, y1), fx);
    return vec4<f32>(mix(top, bot, fy), 1.0);
}

fn catmull(t: f32) -> vec4<f32> {
    let t2 = t * t;
    let t3 = t2 * t;
    return vec4<f32>(
        -0.5 * t3 + t2 - 0.5 * t,
        1.5 * t3 - 2.5 * t2 + 1.0,
        -1.5 * t3 + 2.0 * t2 + 0.5 * t,
        0.5 * t3 - 0.5 * t2
    );
}

fn bicubic(x: f32, y: f32) -> vec4<f32> {
    let xi = i32(floor(x));
    let yi = i32(floor(y));
    if (xi < 1 || yi < 1 || xi + 2 >= i32(p.src.x) || yi + 2 >= i32(p.src.y)) {
        return bilinear(x, y);
    }
    let wx = catmull(x - f32(xi));
    let wy = catmull(y - f32(yi));
    var o = vec3<f32>(0.0);
    for (var j = 0; j < 4; j++) {
        let yy = yi - 1 + j;
        let r = px(xi - 1, yy) * wx.x + px(xi, yy) * wx.y + px(xi + 1, yy) * wx.z + px(xi + 2, yy) * wx.w;
        o += r * wy[j];
    }
    return vec4<f32>(max(o, vec3<f32>(0.0)), 1.0);
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let tw = u32(p.tile.x);
    let row = id.y + u32(p.band.x);
    if (id.x >= tw || row >= u32(p.tile.y)) {
        return;
    }
    let o = (row - u32(p.band.x)) * tw + id.x;
    let gw = u32(p.tile.z);
    let step = p.band.y;
    let gx = f32(id.x) / step;
    let gy = f32(row) / step;
    let i0 = u32(floor(gx));
    let j0 = u32(floor(gy));
    let i1 = min(i0 + 1u, gw - 1u);
    let j1 = min(j0 + 1u, u32(p.tile.w) - 1u);
    let n00 = grid[j0 * gw + i0];
    let n10 = grid[j0 * gw + i1];
    let n01 = grid[j1 * gw + i0];
    let n11 = grid[j1 * gw + i1];
    if (min(min(n00.w, n10.w), min(n01.w, n11.w)) < 0.5) {
        outp[o] = vec4<f32>(0.0, 0.0, 0.0, -1.0);
        return;
    }
    let fx = gx - f32(i0);
    let fy = gy - f32(j0);
    let m = mix(mix(n00.xyz, n10.xyz, fx), mix(n01.xyz, n11.xyz, fx), fy);
    let sx = m.x * p.band.z + (p.src.x - 1.0) / 2.0;
    let sy = m.y * p.band.z + (p.src.y - 1.0) / 2.0;
    var s: vec4<f32>;
    if (p.band.w > 0.5) {
        s = bicubic(sx, sy);
    } else {
        s = bilinear(sx, sy);
    }
    if (s.w < 0.5) {
        outp[o] = vec4<f32>(0.0);
        return;
    }
    let r2 = (m.x * m.x + m.y * m.y) / p.vig.z;
    let vf = exp(p.vig.x * r2 + p.vig.y * r2 * r2);
    outp[o] = vec4<f32>(s.rgb * p.gains.rgb * vf, m.z + 1.0);
}
"#;

impl Gpu {
    async fn new() -> Option<Gpu> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() })
            .await?;
        let limits = adapter.limits();
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("openpano"),
                    required_features: wgpu::Features::empty(),
                    required_limits: limits.clone(),
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await
            .ok()?;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("warp"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let storage = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("warp"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                storage(1, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                storage(3, false),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("warp"), bind_group_layouts: &[&layout], push_constant_ranges: &[] });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("warp"),
            layout: Some(&pl),
            module: &module,
            entry_point: "main",
            compilation_options: Default::default(),
            cache: None,
        });
        let name = adapter.get_info().name;
        let max_buf = (limits.max_storage_buffer_binding_size as u64).min(limits.max_buffer_size);
        Some(Gpu { device, queue, pipeline, layout, max_tex: limits.max_texture_dimension_2d, max_buf, name })
    }

    /// Upload a source image as an RGBA32F texture (None when it exceeds the texture limit).
    pub fn upload(&self, img: &RgbImage) -> Option<Source> {
        let (w, h) = (img.w as u32, img.h as u32);
        if w == 0 || h == 0 || w > self.max_tex || h > self.max_tex {
            return None;
        }
        let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("source"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut rgba: Vec<f32> = Vec::with_capacity(img.data.len() * 4);
        for p in &img.data {
            rgba.extend_from_slice(&[p[0], p[1], p[2], 1.0]);
        }
        self.queue.write_texture(
            wgpu::ImageCopyTexture { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            bytemuck::cast_slice(&rgba),
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(w * 16), rows_per_image: Some(h) },
            size,
        );
        Some(Source { tex, w, h })
    }

    /// Warp one tile. `grid` holds (ix, iy, weight, valid) per node in centred full-res source
    /// coordinates. Returns RGBA per tile pixel (see [`NEEDS_CPU`]).
    pub fn warp(&self, src: &Source, grid: &[[f32; 4]], p: &WarpParams) -> Option<Vec<[f32; 4]>> {
        if grid.len() != p.gw * p.gh || p.tw == 0 || p.th == 0 {
            return None;
        }
        let row_bytes = (p.tw * 16) as u64;
        let band_rows = ((self.max_buf / row_bytes) as usize).clamp(1, p.th);
        let grid_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grid"),
            contents: bytemuck::cast_slice(grid),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let view = src.tex.create_view(&wgpu::TextureViewDescriptor::default());
        let mut out: Vec<[f32; 4]> = Vec::with_capacity(p.tw * p.th);
        let mut r0 = 0;
        while r0 < p.th {
            let rows = band_rows.min(p.th - r0);
            let size = row_bytes * rows as u64;
            let uni: [f32; 20] = [
                p.tw as f32,
                p.th as f32,
                p.gw as f32,
                p.gh as f32,
                r0 as f32,
                STEP as f32,
                p.scale,
                if p.bicubic { 1.0 } else { 0.0 },
                p.gains[0],
                p.gains[1],
                p.gains[2],
                0.0,
                p.vig[0],
                p.vig[1],
                p.hd2.max(1e-6),
                0.0,
                src.w as f32,
                src.h as f32,
                0.0,
                0.0,
            ];
            let ubuf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("params"),
                contents: bytemuck::cast_slice(&uni),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let obuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("out"),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let read = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("read"),
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("warp"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: grid_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 3, resource: obuf.as_entire_binding() },
                ],
            });
            let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("warp") });
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("warp"), timestamp_writes: None });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(p.tw.div_ceil(16) as u32, rows.div_ceil(16) as u32, 1);
            }
            enc.copy_buffer_to_buffer(&obuf, 0, &read, 0, size);
            self.queue.submit(Some(enc.finish()));
            let slice = read.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r.is_ok());
            });
            self.device.poll(wgpu::Maintain::Wait);
            if !rx.recv().unwrap_or(false) {
                return None;
            }
            out.extend_from_slice(bytemuck::cast_slice::<u8, [f32; 4]>(&slice.get_mapped_range()));
            read.unmap();
            r0 += rows;
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs only where an adapter exists (e.g. the WARP software device on Windows CI).
    #[test]
    fn warp_matches_cpu_sampling() {
        let Some(g) = get() else {
            eprintln!("no GPU adapter – skipped");
            return;
        };
        let (w, h) = (40usize, 30usize);
        let mut img = RgbImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                img.data[y * w + x] = [x as f32 / w as f32, y as f32 / h as f32, ((x * y) % 7) as f32 / 7.0];
            }
        }
        let src = g.upload(&img).expect("upload");
        // identity-like mapping: tile pixel (x, y) -> source pixel (x + 3.25, y + 2.5)
        let (tw, th) = (24usize, 18usize);
        let (gw, gh) = ((tw - 1) / STEP + 2, (th - 1) / STEP + 2);
        let cx = (w as f32 - 1.0) / 2.0;
        let cy = (h as f32 - 1.0) / 2.0;
        let mut grid = Vec::new();
        for j in 0..gh {
            for i in 0..gw {
                let (x, y) = ((i * STEP) as f32, (j * STEP) as f32);
                grid.push([x + 3.25 - cx, y + 2.5 - cy, 0.5, if i == 0 && j == 0 { 0.0 } else { 1.0 }]);
            }
        }
        let p = WarpParams { tw, th, gw, gh, scale: 1.0, gains: [1.0; 3], vig: [0.0; 2], hd2: 1.0, bicubic: false };
        let out = g.warp(&src, &grid, &p).expect("warp");
        assert_eq!(out.len(), tw * th);
        assert!(out[0][3] < 0.0, "cell next to an invalid node must fall back to the CPU");
        for y in STEP..th {
            for x in STEP..tw {
                let o = out[y * tw + x];
                let e = img.sample(x as f32 + 3.25, y as f32 + 2.5).unwrap();
                assert!((o[3] - 1.5).abs() < 1e-4);
                for k in 0..3 {
                    assert!((o[k] - e[k]).abs() < 1e-3, "pixel {x},{y}: {o:?} vs {e:?}");
                }
            }
        }
    }
}
