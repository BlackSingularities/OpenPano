//! Tauri commands bridging the web UI and the stitching engine.

use base64::Engine;
use openpano_core::i18n::{self, Lang};
use openpano_core::output::OutputSettings;
use openpano_core::preview::Sphere;
use openpano_core::project::{self, Aligned, PhotoOptions, WorkImage};
use openpano_core::stitch::align::{AlignSettings, Camera, FocalSource, PairDiag};
use openpano_core::stitch::geometry::{LensType, Projection};
use openpano_core::stitch::render::{self, RenderSettings};
use openpano_core::stitch::{Cancel, Progress, StitchError};
use serde::{Deserialize, Serialize};
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
    /// matching diagnostics of the last alignment attempt (also when it failed)
    diag: Option<Arc<DiagState>>,
}

pub struct DiagState {
    ids: Vec<u32>,
    pairs: Vec<PairDiag>,
    /// final cameras by index (all None when alignment failed)
    cameras: Vec<Option<Camera>>,
    lens: LensType,
    /// full-resolution sizes by index
    sizes: Vec<(usize, usize)>,
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

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("settings.json"))
}

#[tauri::command]
pub fn load_settings(app: AppHandle) -> serde_json::Value {
    settings_path(&app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: serde_json::Value) -> Res<()> {
    let p = settings_path(&app).ok_or("no config dir")?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(err)?;
    }
    std::fs::write(p, serde_json::to_string_pretty(&settings).map_err(err)?).map_err(err)
}

/// Writable cache directory (used on mobile for imported files and rendered output).
#[tauri::command]
pub fn cache_dir(app: AppHandle) -> Res<String> {
    let d = app.path().app_cache_dir().map_err(err)?;
    std::fs::create_dir_all(&d).map_err(err)?;
    Ok(d.to_string_lossy().to_string())
}

/// Import an image sent as raw bytes (mobile content URIs). Header `x-name` carries the file
/// name. The file is stored in the cache directory and loaded like a normal path.
#[tauri::command]
pub fn add_image_bytes(app: AppHandle, state: State<'_, Shared>, request: tauri::ipc::Request<'_>) -> Res<Vec<Added>> {
    let name = request
        .headers()
        .get("x-name")
        .and_then(|v| v.to_str().ok())
        .map(|s| percent_decode(s))
        .unwrap_or_else(|| "image.jpg".into());
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else { return Err("expected raw body".into()) };
    let dir = app.path().app_cache_dir().map_err(err)?.join("import");
    std::fs::create_dir_all(&dir).map_err(err)?;
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' }).collect();
    let mut path = dir.join(&safe);
    let mut k = 1;
    while path.exists() {
        path = dir.join(format!("{k}_{safe}"));
        k += 1;
    }
    std::fs::write(&path, bytes).map_err(err)?;
    Ok(register_and_load(&app, &state, vec![path]))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Raw bytes of a file (used on mobile to copy rendered output to a user-chosen location).
#[tauri::command]
pub fn read_file_bytes(path: String) -> Res<tauri::ipc::Response> {
    Ok(tauri::ipc::Response::new(std::fs::read(path).map_err(err)?))
}

#[tauri::command]
pub fn cancel_job(state: State<'_, Shared>) {
    state.cancel.lock().unwrap().cancel();
}

#[tauri::command]
pub fn supported_extensions() -> Vec<String> {
    let mut v: Vec<String> = openpano_core::loader::RASTER_EXTENSIONS.iter().map(|s| s.to_string()).collect();
    v.extend(openpano_core::raw::RAW_EXTENSIONS.iter().map(|s| s.to_string()));
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
    let paths = openpano_core::loader::expand_paths(&paths);
    register_and_load(&app, &state, paths)
}

fn register_and_load(app: &AppHandle, state: &Shared, paths: Vec<PathBuf>) -> Vec<Added> {
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
    let (_p, cancel, done) = begin_job(app, state, openpano_core::i18n::tr(openpano_core::i18n::Key::StageLoading));
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
                    Loaded { id: *id, ok: false, error: Some(openpano_core::i18n::tr(openpano_core::i18n::Key::Cancelled).into()), width: 0, height: 0, is_raw: false, summary: String::new(), camera: String::new(), thumb: String::new() }
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
    /// 0..100 alignment quality score
    quality: f64,
    /// "good" | "fair" | "poor"
    quality_level: String,
    /// problem codes: "unaligned", "highError", "weakPairs"
    issues: Vec<String>,
    /// image pairs with their inlier counts (ids)
    pair_list: Vec<PairInfo>,
    /// suggested image pair (ids) for manual control points
    suggest: Option<(u32, u32)>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PairInfo {
    a: u32,
    b: u32,
    points: usize,
    confidence: f64,
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
    // ---- quality score
    let total = a.cameras.len().max(1);
    let aligned = a.cameras.iter().filter(|c| c.is_some()).count();
    let frac = aligned as f64 / total as f64;
    let rms_f = (1.0 - (a.rms_px - 1.5) / 6.0).clamp(0.25, 1.0);
    let weak = a.pairs.iter().filter(|p| p.confidence < 1.3).count();
    let tree_edges = aligned.saturating_sub(1).max(1);
    let weak_f = if a.pairs.len() <= tree_edges && weak > 0 { 0.8 } else { 1.0 };
    let quality = (100.0 * frac.powf(1.5) * rms_f * weak_f).round();
    let mut issues = Vec::new();
    if aligned < total {
        issues.push("unaligned".to_string());
    }
    if a.rms_px > 4.0 {
        issues.push("highError".to_string());
    }
    if weak_f < 1.0 {
        issues.push("weakPairs".to_string());
    }
    let quality_level = if quality >= 80.0 && issues.is_empty() {
        "good"
    } else if quality >= 55.0 {
        "fair"
    } else {
        "poor"
    }
    .to_string();
    let pair_list: Vec<PairInfo> = a
        .pairs
        .iter()
        .map(|p| PairInfo { a: ids[p.i], b: ids[p.j], points: p.pts.len(), confidence: p.confidence })
        .collect();
    // suggestion: first unaligned image with its neighbour in file order, else the weakest pair
    let suggest = (0..a.cameras.len())
        .find(|&k| a.cameras[k].is_none())
        .map(|k| {
            let nb = if k > 0 { k - 1 } else { (k + 1).min(a.cameras.len() - 1) };
            (ids[nb], ids[k])
        })
        .or_else(|| {
            a.pairs
                .iter()
                .min_by(|x, y| x.confidence.partial_cmp(&y.confidence).unwrap())
                .map(|p| (ids[p.i], ids[p.j]))
        });
    AlignInfo {
        quality,
        quality_level,
        issues,
        pair_list,
        suggest,
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

#[derive(Deserialize, Clone)]
pub struct ManualDto {
    a: u32,
    b: u32,
    ax: f64,
    ay: f64,
    bx: f64,
    by: f64,
}

#[tauri::command]
pub async fn align(
    app: AppHandle,
    state: State<'_, Shared>,
    ids: Vec<u32>,
    settings: AlignSettings,
    photo: PhotoOptions,
    manual: Option<Vec<ManualDto>>,
) -> Res<AlignInfo> {
    let manual: Vec<openpano_core::stitch::align::ManualPoint> = manual
        .unwrap_or_default()
        .into_iter()
        .filter_map(|m| {
            let i = ids.iter().position(|x| *x == m.a)?;
            let j = ids.iter().position(|x| *x == m.b)?;
            Some(openpano_core::stitch::align::ManualPoint { i, j, a: [m.ax, m.ay], b: [m.bx, m.by] })
        })
        .collect();
    let works: Vec<Arc<WorkImage>> = {
        let g = state.inner.lock().unwrap();
        ids.iter().map(|id| g.works.get(id).cloned().ok_or_else(|| format!("image {id} not loaded"))).collect::<Res<_>>()?
    };
    let (progress, cancel, done) = begin_job(&app, &state, openpano_core::i18n::tr(openpano_core::i18n::Key::StageFeatures));
    let t0 = std::time::Instant::now();
    let sizes: Vec<(usize, usize)> = works.iter().map(|w| (w.full_w, w.full_h)).collect();
    let settings_lens = settings.lens;
    let (r, pairs) = tauri::async_runtime::spawn_blocking(move || {
        let mut diag = Vec::new();
        let r = project::run_align_diag(&works, &settings, &photo, &manual, &progress, &cancel, &mut diag);
        (r, diag)
    })
    .await
    .map_err(err)?;
    done.store(true, Ordering::Relaxed);
    let (cameras, lens) = match &r {
        Ok(al) => (al.alignment.cameras.clone(), al.alignment.lens),
        Err(_) => (vec![None; ids.len()], settings_lens),
    };
    state.inner.lock().unwrap().diag = Some(Arc::new(DiagState { ids: ids.clone(), pairs, cameras, lens, sizes }));
    let al = Arc::new(r.map_err(err)?);
    let info = align_info(&al, &ids, t0.elapsed().as_secs_f64());
    let mut g = state.inner.lock().unwrap();
    g.aligned = Some(al);
    g.aligned_ids = ids;
    g.sphere = None;
    Ok(info)
}

// ------------------------------------------------------------------------- match diagnostics

/// Reprojection error (full-resolution px) above which a pair counts as inconsistent.
const PAIR_ERR_PX: f64 = 4.0;

impl DiagState {
    fn idx(&self, id: u32) -> Option<usize> {
        self.ids.iter().position(|x| *x == id)
    }
    fn center(&self, k: usize) -> (f64, f64) {
        ((self.sizes[k].0 as f64 - 1.0) / 2.0, (self.sizes[k].1 as f64 - 1.0) / 2.0)
    }
    /// Error in px (on image i) of a correspondence given in top-left full-res coordinates.
    fn residual(&self, i: usize, j: usize, a: [f64; 2], b: [f64; 2]) -> Option<f64> {
        let (ci, cj) = (self.cameras[i].as_ref()?, self.cameras[j].as_ref()?);
        let ra = ci.ray_world(self.lens, a[0], a[1]).normalize();
        let rb = cj.ray_world(self.lens, b[0], b[1]).normalize();
        Some(ra.dot(&rb).clamp(-1.0, 1.0).acos() * ci.f)
    }
    /// Sampled raw matches as (top-left a, top-left b, inlier), flipped to (j, i) when asked.
    fn pts(&self, d: &PairDiag, flip: bool) -> Vec<([f64; 2], [f64; 2], bool)> {
        let (ci, cj) = (self.center(d.i), self.center(d.j));
        d.pts
            .iter()
            .map(|(a, b, f)| {
                let pa = [a[0] + ci.0, a[1] + ci.1];
                let pb = [b[0] + cj.0, b[1] + cj.1];
                if flip { (pb, pa, *f) } else { (pa, pb, *f) }
            })
            .collect()
    }
    /// Median error of the verified matches of a pair under the final cameras.
    fn pair_error(&self, d: &PairDiag) -> Option<f64> {
        let mut e: Vec<f64> = self.pts(d, false).iter().filter(|p| p.2).filter_map(|p| self.residual(d.i, d.j, p.0, p.1)).collect();
        if e.is_empty() {
            return None;
        }
        e.sort_by(|x, y| x.partial_cmp(y).unwrap());
        Some(e[e.len() / 2])
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairReport {
    a: u32,
    b: u32,
    matches: usize,
    inliers: usize,
    accepted: bool,
    /// median reprojection error of the pair (px) when both images are aligned
    error: Option<f64>,
    /// "ok" | "highError" | "rejected" | "weak"
    status: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchReport {
    failed: bool,
    unaligned: Vec<u32>,
    pairs: Vec<PairReport>,
    suggest: Option<(u32, u32)>,
}

/// Which pairs matched, which were rejected or disagree with the final solution, and the most
/// useful pair for manual control points.
#[tauri::command]
pub fn match_report(state: State<'_, Shared>) -> Res<MatchReport> {
    let d = state.inner.lock().unwrap().diag.clone().ok_or("no alignment yet")?;
    let n = d.ids.len();
    let failed = d.cameras.iter().all(|c| c.is_none());
    let unaligned_idx: Vec<usize> = (0..n).filter(|&k| d.cameras[k].is_none()).collect();
    let mut pairs: Vec<PairReport> = d
        .pairs
        .iter()
        .map(|p| {
            let error = if p.accepted { d.pair_error(p) } else { None };
            let status = if !p.accepted {
                // a real overlap leaves a sizeable consistent subset; non-overlapping pairs only
                // produce random matches with (almost) no consistent ones
                if p.inliers >= 12 {
                    "rejected"
                } else {
                    "weak"
                }
            } else if error.map(|e| e > PAIR_ERR_PX).unwrap_or(false) {
                "highError"
            } else {
                "ok"
            };
            PairReport { a: d.ids[p.i], b: d.ids[p.j], matches: p.matches, inliers: p.inliers, accepted: p.accepted, error, status }
        })
        .collect();
    let rank = |s: &str| match s {
        "highError" => 0,
        "rejected" => 1,
        "ok" => 2,
        _ => 3,
    };
    pairs.sort_by(|x, y| rank(x.status).cmp(&rank(y.status)).then(y.matches.cmp(&x.matches)));
    let between = |i: usize, j: usize| d.pairs.iter().find(|p| (p.i == i && p.j == j) || (p.i == j && p.j == i)).map(|p| (p.inliers, p.matches)).unwrap_or((0, 0));
    let suggest = if failed {
        // the pair with the most candidate matches is the easiest one to fix by hand
        d.pairs.iter().max_by_key(|p| (p.inliers, p.matches)).map(|p| (d.ids[p.i], d.ids[p.j])).or_else(|| (n >= 2).then(|| (d.ids[0], d.ids[1])))
    } else if !unaligned_idx.is_empty() {
        // the unaligned image with the strongest link to the aligned set, with that partner
        let aligned: Vec<usize> = (0..n).filter(|k| d.cameras[*k].is_some()).collect();
        unaligned_idx
            .iter()
            .flat_map(|&u| aligned.iter().map(move |&a| (a, u)))
            .max_by_key(|&(a, u)| (between(a, u), usize::MAX - a.abs_diff(u)))
            .map(|(a, u)| (d.ids[a], d.ids[u]))
    } else {
        pairs
            .iter()
            .find(|p| p.status == "highError")
            .map(|p| (p.a, p.b))
            .or_else(|| pairs.iter().filter(|p| p.accepted).min_by_key(|p| p.inliers).map(|p| (p.a, p.b)))
    };
    Ok(MatchReport { failed, unaligned: unaligned_idx.iter().map(|&k| d.ids[k]).collect(), pairs, suggest })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchPt {
    ax: f64,
    ay: f64,
    bx: f64,
    by: f64,
    /// passed geometric verification for this pair
    inlier: bool,
    /// reprojection error under the final cameras (px)
    error: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairMatches {
    tested: bool,
    accepted: bool,
    /// both images have a solved camera (errors and predictions are available)
    solved: bool,
    auto: Vec<MatchPt>,
    /// error of each given manual point (px)
    manual: Vec<Option<f64>>,
}

/// Automatic matches of an image pair (top-left full-res coordinates, oriented a→b) with their
/// verdicts, plus the residual error of the user's control points for that pair.
#[tauri::command]
pub fn pair_matches(state: State<'_, Shared>, a: u32, b: u32, manual: Vec<ManualDto>) -> Res<PairMatches> {
    let d = state.inner.lock().unwrap().diag.clone().ok_or("no alignment yet")?;
    let (Some(i), Some(j)) = (d.idx(a), d.idx(b)) else {
        return Ok(PairMatches { tested: false, accepted: false, solved: false, auto: vec![], manual: manual.iter().map(|_| None).collect() });
    };
    let solved = d.cameras[i].is_some() && d.cameras[j].is_some();
    let pd = d.pairs.iter().find(|p| (p.i == i && p.j == j) || (p.i == j && p.j == i));
    let auto = pd
        .map(|p| {
            d.pts(p, p.i != i)
                .into_iter()
                .map(|(pa, pb, inlier)| MatchPt { ax: pa[0], ay: pa[1], bx: pb[0], by: pb[1], inlier, error: d.residual(i, j, pa, pb) })
                .collect()
        })
        .unwrap_or_default();
    let manual = manual
        .iter()
        .map(|m| {
            let (pa, pb) = if m.a == a { ([m.ax, m.ay], [m.bx, m.by]) } else { ([m.bx, m.by], [m.ax, m.ay]) };
            d.residual(i, j, pa, pb)
        })
        .collect();
    Ok(PairMatches { tested: pd.is_some(), accepted: pd.map(|p| p.accepted).unwrap_or(false), solved, auto, manual })
}

/// Where a point of image `a` should appear in image `b` according to the current alignment.
#[tauri::command]
pub fn predict_point(state: State<'_, Shared>, a: u32, b: u32, x: f64, y: f64) -> Option<[f64; 2]> {
    let d = state.inner.lock().unwrap().diag.clone()?;
    let (i, j) = (d.idx(a)?, d.idx(b)?);
    let (ci, cj) = (d.cameras[i].as_ref()?, d.cameras[j].as_ref()?);
    let ray = ci.ray_world(d.lens, x, y);
    let (px, py) = cj.project_centered(d.lens, &(cj.rot.inverse() * ray))?;
    let (cx, cy) = d.center(j);
    let (px, py) = (px + cx, py + cy);
    let (w, h) = d.sizes[j];
    (px >= 0.0 && py >= 0.0 && px < w as f64 && py < h as f64).then_some([px, py])
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
    let (progress, cancel, done) = begin_job(&app, &state, openpano_core::i18n::tr(openpano_core::i18n::Key::StagePreview));
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
    let (progress, cancel, done) = begin_job(&app, &state, openpano_core::i18n::tr(openpano_core::i18n::Key::StageRendering));
    let t0 = std::time::Instant::now();
    let path2 = PathBuf::from(&path);
    let r = tauri::async_runtime::spawn_blocking(move || -> Result<(String, usize, usize), StitchError> {
        let out = project::render_full(&works, &al, &render, &progress, &cancel)?;
        progress.set_stage(openpano_core::i18n::tr(openpano_core::i18n::Key::StageSaving));
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

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorkPreview {
    data: String,
    w: usize,
    h: usize,
    full_w: usize,
    full_h: usize,
}

/// Working-resolution image (JPEG data URL) for the control point editor.
#[tauri::command]
pub fn work_preview(state: State<'_, Shared>, id: u32) -> Res<WorkPreview> {
    let w = state.inner.lock().unwrap().works.get(&id).cloned().ok_or("image not loaded")?;
    let conv = openpano_core::imaging::ToSrgb8::new();
    let rgb: Vec<u8> = w.img.data.iter().flat_map(|p| [conv.conv(p[0]), conv.conv(p[1]), conv.conv(p[2])]).collect();
    let mut buf = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 88)
        .encode(&rgb, w.img.w as u32, w.img.h as u32, image::ExtendedColorType::Rgb8)
        .map_err(err)?;
    Ok(WorkPreview {
        data: format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(&buf)),
        w: w.img.w,
        h: w.img.h,
        full_w: w.full_w,
        full_h: w.full_h,
    })
}
