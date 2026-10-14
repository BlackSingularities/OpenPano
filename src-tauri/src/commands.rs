//! Tauri commands bridging the web UI and the stitching engine.

use base64::Engine;
use panoramer_core::i18n::{self, Lang};
use panoramer_core::output::OutputSettings;
use panoramer_core::preview::Sphere;
use panoramer_core::project::{self, Aligned, PhotoOptions, WorkImage};
use panoramer_core::stitch::align::{AlignSettings, FocalSource};
use panoramer_core::stitch::geometry::{LensType, Projection};
use panoramer_core::stitch::render::{self, RenderSettings};
use panoramer_core::stitch::{Cancel, Progress, StitchError};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Default)]
pub struct Inner {
    works: HashMap<u32, Arc<WorkImage>>,
    paths: HashMap<u32, PathBuf>,
    next_id: u32,
    aligned: Option<Arc<Aligned>>,
    aligned_ids: Vec<u32>,
    sphere: Option<Arc<Sphere>>,
}

#[derive(Default)]
pub struct Shared {
    inner: Mutex<Inner>,
    progress: Mutex<Progress>,
    cancel: Mutex<Cancel>,
}

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    stage: String,
    fraction: f32,
}

/// Start a new cancellable job and a reporter thread that emits progress events.
fn begin_job(app: &AppHandle, st: &Shared, stage: &str) -> (Progress, Cancel, Arc<AtomicBool>) {
    let p = Progress::default();
    p.set_stage(stage);
    let c = Cancel::default();
    *st.progress.lock().unwrap() = p.clone();
    *st.cancel.lock().unwrap() = c.clone();
    let done = Arc::new(AtomicBool::new(false));
    let (app2, p2, d2) = (app.clone(), p.clone(), done.clone());
    std::thread::spawn(move || {
        while !d2.load(Ordering::Relaxed) {
            let _ = app2.emit("progress", ProgressEvent { stage: p2.stage(), fraction: p2.fraction() });
            std::thread::sleep(std::time::Duration::from_millis(80));
        }
    });
    (p, c, done)
}

// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub fn set_language(lang: String) {
    if let Some(l) = Lang::from_code(&lang) {
        i18n::set_lang(l);
    }
}

#[tauri::command]
pub fn system_language() -> String {
    match Lang::detect() {
        Lang::Pl => "pl".into(),
        Lang::En => "en".into(),
    }
}

fn settings_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "redzikowski", "Panoramer").map(|d| d.config_dir().join("settings.json"))
}

#[tauri::command]
pub fn load_settings() -> serde_json::Value {
    settings_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null)
}

#[tauri::command]
pub fn save_settings(settings: serde_json::Value) -> Res<()> {
    let p = settings_path().ok_or("no config dir")?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(err)?;
    }
    std::fs::write(p, serde_json::to_string_pretty(&settings).map_err(err)?).map_err(err)
}

#[tauri::command]
pub fn cancel_job(state: State<'_, Shared>) {
    state.cancel.lock().unwrap().cancel();
}

#[tauri::command]
pub fn supported_extensions() -> Vec<String> {
    let mut v: Vec<String> = panoramer_core::loader::RASTER_EXTENSIONS.iter().map(|s| s.to_string()).collect();
    v.extend(panoramer_core::raw::RAW_EXTENSIONS.iter().map(|s| s.to_string()));
    v
}

// ------------------------------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Added {
    id: u32,
    path: String,
    name: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Loaded {
    id: u32,
    ok: bool,
    error: Option<String>,
    width: usize,
    height: usize,
    is_raw: bool,
    summary: String,
    camera: String,
    thumb: String,
}

fn thumb_data_url(w: &WorkImage) -> String {
    let (tw, th, rgba) = &w.thumb;
    let rgb: Vec<u8> = rgba.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mut buf = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 85);
    if enc.encode(&rgb, *tw as u32, *th as u32, image::ExtendedColorType::Rgb8).is_err() {
        return String::new();
    }
    format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(&buf))
}

/// Register paths (folders are expanded) and load them in the background. Returns the entries
/// immediately; `image-loaded` events follow, then `load-finished`.
#[tauri::command]
pub fn add_images(app: AppHandle, state: State<'_, Shared>, paths: Vec<String>) -> Vec<Added> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let paths = panoramer_core::loader::expand_paths(&paths);
    let mut added = Vec::new();
    {
        let mut g = state.inner.lock().unwrap();
        for p in paths {
            if g.paths.values().any(|x| *x == p) {
                continue;
            }
            let id = g.next_id;
            g.next_id += 1;
            g.paths.insert(id, p.clone());
            added.push(Added {
                id,
                path: p.to_string_lossy().to_string(),
                name: p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
            });
        }
    }
    if added.is_empty() {
        return added;
    }
    let jobs: Vec<(u32, PathBuf)> = added.iter().map(|a| (a.id, PathBuf::from(&a.path))).collect();
    let (_p, cancel, done) = begin_job(&app, &state, panoramer_core::i18n::tr(panoramer_core::i18n::Key::StageLoading));
    let app2 = app.clone();
    std::thread::spawn(move || {
        use rayon::prelude::*;
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 6);
        let pool = rayon::ThreadPoolBuilder::new().num_threads(n).build().expect("pool");
        let total = jobs.len();
        let counter = std::sync::atomic::AtomicUsize::new(0);
        pool.install(|| {
            jobs.par_iter().for_each(|(id, path)| {
                let ev = if cancel.is_cancelled() {
                    Loaded { id: *id, ok: false, error: Some(panoramer_core::i18n::tr(panoramer_core::i18n::Key::Cancelled).into()), width: 0, height: 0, is_raw: false, summary: String::new(), camera: String::new(), thumb: String::new() }
                } else {
                    match project::load_work(path) {
                        Ok(w) => {
                            let w = Arc::new(w);
                            let ev = Loaded {
                                id: *id,
                                ok: true,
                                error: None,
                                width: w.full_w,
                                height: w.full_h,
                                is_raw: w.is_raw,
                                summary: w.meta.summary(),
                                camera: format!("{} {}", w.meta.make, w.meta.model).trim().to_string(),
                                thumb: thumb_data_url(&w),
                            };
                            let st: State<'_, Shared> = app2.state();
                            st.inner.lock().unwrap().works.insert(*id, w);
                            ev
                        }
                        Err(e) => Loaded { id: *id, ok: false, error: Some(e.to_string()), width: 0, height: 0, is_raw: false, summary: String::new(), camera: String::new(), thumb: String::new() },
                    }
                };
                let c = counter.fetch_add(1, Ordering::Relaxed) + 1;
                let st: State<'_, Shared> = app2.state();
                st.progress.lock().unwrap().set_fraction(c as f32 / total as f32);
                let _ = app2.emit("image-loaded", ev);
            });
        });
        done.store(true, Ordering::Relaxed);
        let _ = app2.emit("load-finished", ());
    });
    added
}

#[tauri::command]
pub fn remove_images(state: State<'_, Shared>, ids: Vec<u32>) {
    let mut g = state.inner.lock().unwrap();
    for id in ids {
        g.works.remove(&id);
        g.paths.remove(&id);
    }
}

// ------------------------------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CamInfo {
    id: u32,
    aligned: bool,
    f: f64,
    /// row-major camera->world rotation
    rot: [f64; 9],
    w: usize,
    h: usize,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AlignInfo {
    cameras: Vec<CamInfo>,
    lens: LensType,
    rms: f64,
    focal_source: String,
    pairs: usize,
    hfov: f64,
    coverage: (f64, f64, f64),
    auto_projection: Projection,
    seconds: f64,
}

fn align_info(al: &Aligned, ids: &[u32], secs: f64) -> AlignInfo {
    let a = &al.alignment;
    let cameras = a
        .cameras
        .iter()
        .zip(ids)
        .map(|(c, id)| match c {
            Some(c) => {
                let m = c.rot.matrix();
                CamInfo {
                    id: *id,
                    aligned: true,
                    f: c.f,
                    rot: [m[(0, 0)], m[(0, 1)], m[(0, 2)], m[(1, 0)], m[(1, 1)], m[(1, 2)], m[(2, 0)], m[(2, 1)], m[(2, 2)]],
                    w: c.w,
                    h: c.h,
                }
            }
            None => CamInfo { id: *id, aligned: false, f: 0.0, rot: [0.0; 9], w: 0, h: 0 },
        })
        .collect();
    let hfov = a.cameras.iter().flatten().next().map(|c| a.lens.hfov_from_focal(c.f, c.w as f64)).unwrap_or(0.0);
    AlignInfo {
        cameras,
        lens: a.lens,
        rms: a.rms_px,
        focal_source: match a.focal_source {
            FocalSource::Manual => "manual",
            FocalSource::Exif => "exif",
            FocalSource::Estimated => "estimated",
            FocalSource::Default => "default",
        }
        .into(),
        pairs: a.pairs.len(),
        hfov,
        coverage: render::coverage(a),
        auto_projection: render::auto_projection(a),
        seconds: secs,
    }
}

#[tauri::command]
pub async fn align(app: AppHandle, state: State<'_, Shared>, ids: Vec<u32>, settings: AlignSettings, photo: PhotoOptions) -> Res<AlignInfo> {
    let works: Vec<Arc<WorkImage>> = {
        let g = state.inner.lock().unwrap();
        ids.iter().map(|id| g.works.get(id).cloned().ok_or_else(|| format!("image {id} not loaded"))).collect::<Res<_>>()?
    };
    let (progress, cancel, done) = begin_job(&app, &state, panoramer_core::i18n::tr(panoramer_core::i18n::Key::StageFeatures));
    let t0 = std::time::Instant::now();
    let r = tauri::async_runtime::spawn_blocking(move || project::run_align(&works, &settings, &photo, &progress, &cancel))
        .await
        .map_err(err)?;
    done.store(true, Ordering::Relaxed);
    let al = Arc::new(r.map_err(err)?);
    let info = align_info(&al, &ids, t0.elapsed().as_secs_f64());
    let mut g = state.inner.lock().unwrap();
    g.aligned = Some(al);
    g.aligned_ids = ids;
    g.sphere = None;
    Ok(info)
}

fn aligned_works(g: &Inner) -> Res<(Arc<Aligned>, Vec<Arc<WorkImage>>)> {
    let al = g.aligned.clone().ok_or("not aligned")?;
    let works = g.aligned_ids.iter().map(|id| g.works.get(id).cloned().ok_or("image removed")).collect::<Result<Vec<_>, _>>()?;
    Ok((al, works))
}

#[tauri::command]
pub fn set_photometric(state: State<'_, Shared>, photo: PhotoOptions) -> Res<()> {
    let mut g = state.inner.lock().unwrap();
    let (al, works) = aligned_works(&g)?;
    let p = project::recompute_photo(&works, &al.alignment, &photo);
    g.aligned = Some(Arc::new(Aligned { alignment: al.alignment.clone(), photo: p }));
    Ok(())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SphereInfo {
    u0: f64,
    v0: f64,
    scale: f64,
    w: usize,
    h: usize,
    wraps: bool,
}

#[tauri::command]
pub async fn build_sphere(app: AppHandle, state: State<'_, Shared>, render: RenderSettings, megapixels: f64) -> Res<SphereInfo> {
    let (al, works) = aligned_works(&state.inner.lock().unwrap())?;
    let (progress, cancel, done) = begin_job(&app, &state, panoramer_core::i18n::tr(panoramer_core::i18n::Key::StagePreview));
    let r = tauri::async_runtime::spawn_blocking(move || project::build_sphere(&works, &al, &render, megapixels, &progress, &cancel))
        .await
        .map_err(err)?;
    done.store(true, Ordering::Relaxed);
    let sp = r.map_err(err)?;
    let info = SphereInfo { u0: sp.u0, v0: sp.v0, scale: sp.scale, w: sp.w, h: sp.h, wraps: sp.wraps };
    state.inner.lock().unwrap().sphere = Some(Arc::new(sp));
    Ok(info)
}

/// Raw RGBA bytes of the current sphere map.
#[tauri::command]
pub fn sphere_pixels(state: State<'_, Shared>) -> Res<tauri::ipc::Response> {
    let sp = state.inner.lock().unwrap().sphere.clone().ok_or("no preview")?;
    let mut bytes = Vec::with_capacity(sp.rgba.len() * 4);
    for p in &sp.rgba {
        bytes.extend_from_slice(p);
    }
    Ok(tauri::ipc::Response::new(bytes))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LayoutInfo {
    projection: Projection,
    u0: f64,
    v0: f64,
    u1: f64,
    v1: f64,
    /// final output canvas size (before cropping)
    out_w: usize,
    out_h: usize,
}

#[tauri::command]
pub fn layout(state: State<'_, Shared>, render: RenderSettings) -> Res<LayoutInfo> {
    let al = state.inner.lock().unwrap().aligned.clone().ok_or("not aligned")?;
    let mut rs = render.clone();
    rs.crop = render::CropMode::None;
    let c = render::layout(&al.alignment, &rs, Some(4000.0)).map_err(err)?;
    let out = render::layout(&al.alignment, &rs, None).map_err(err)?;
    Ok(LayoutInfo {
        projection: c.proj,
        u0: c.u0,
        v0: c.v0,
        u1: c.u0 + c.w as f64 / c.scale,
        v1: c.v0 + c.h as f64 / c.scale,
        out_w: out.w,
        out_h: out.h,
    })
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RenderInfo {
    path: String,
    format: String,
    width: usize,
    height: usize,
    seconds: f64,
}

#[tauri::command]
pub async fn render_and_save(app: AppHandle, state: State<'_, Shared>, path: String, render: RenderSettings, output: OutputSettings) -> Res<RenderInfo> {
    let (al, works) = aligned_works(&state.inner.lock().unwrap())?;
    let (progress, cancel, done) = begin_job(&app, &state, panoramer_core::i18n::tr(panoramer_core::i18n::Key::StageRendering));
    let t0 = std::time::Instant::now();
    let path2 = PathBuf::from(&path);
    let r = tauri::async_runtime::spawn_blocking(move || -> Result<(String, usize, usize), StitchError> {
        let out = project::render_full(&works, &al, &render, &progress, &cancel)?;
        progress.set_stage(panoramer_core::i18n::tr(panoramer_core::i18n::Key::StageSaving));
        let fmt = project::save_output(&path2, &out, &works, &output)?;
        Ok((format!("{:?}", fmt), out.image.w, out.image.h))
    })
    .await
    .map_err(err)?;
    done.store(true, Ordering::Relaxed);
    let (format, width, height) = r.map_err(err)?;
    Ok(RenderInfo { path, format, width, height, seconds: t0.elapsed().as_secs_f64() })
}

/// Whether any of the aligned (or loaded) images is a RAW file.
#[tauri::command]
pub fn any_raw(state: State<'_, Shared>, ids: Vec<u32>) -> bool {
    let g = state.inner.lock().unwrap();
    ids.iter().filter_map(|id| g.works.get(id)).any(|w| w.is_raw)
}
