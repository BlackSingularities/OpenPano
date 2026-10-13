//! Graphical user interface.

use crate::i18n::{self, tr, Key, Lang};
use crate::loader;
use crate::output::{OutputFormat, OutputSettings};
use crate::project::{self, Aligned, PhotoOptions, WorkImage};
use crate::stitch::align::{AlignSettings, FocalSource, MatchStrategy};
use crate::stitch::geometry::{LensType, Projection};
use crate::stitch::render::{self, BlendMode, CropMode, RenderOutput, RenderSettings};
use crate::stitch::{Cancel, Progress, StitchError};
use egui::{Color32, RichText, Vec2};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

const SETTINGS_KEY: &str = "panoramer.settings.v1";
const ACCENT: Color32 = Color32::from_rgb(255, 170, 60);
const PREVIEW_MP: f64 = 1.5;
const REPO_URL: &str = "https://github.com/BlackSingularities/Panoramer";

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub lang: Lang,
    pub align: AlignSettings,
    pub photo: PhotoOptions,
    pub render: RenderSettings,
    pub output: OutputSettings,
    pub manual_fov: bool,
    pub fov_deg: f64,
    pub last_dir: Option<PathBuf>,
    pub open_folder_after: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            lang: Lang::detect(),
            align: AlignSettings::default(),
            photo: PhotoOptions::default(),
            render: RenderSettings::default(),
            output: OutputSettings::default(),
            manual_fov: false,
            fov_deg: 60.0,
            last_dir: None,
            open_folder_after: true,
        }
    }
}

enum Msg {
    Loaded(PathBuf, Result<Arc<WorkImage>, String>),
    LoadFinished,
    Aligned(Result<(Arc<Aligned>, Vec<usize>), StitchError>, f64),
    Preview(Result<RenderOutput, StitchError>, u64),
    Rendered(Result<(PathBuf, OutputFormat, usize, usize), StitchError>, f64),
}

#[derive(PartialEq, Clone, Copy)]
enum Job {
    Load,
    Align,
    Preview,
    Render,
}

struct Entry {
    path: PathBuf,
    work: Option<Arc<WorkImage>>,
    error: Option<String>,
    enabled: bool,
    thumb: Option<egui::TextureHandle>,
}

struct PreviewState {
    tex: egui::TextureHandle,
    w: usize,
    h: usize,
    alpha: Vec<u8>,
    scale: f64,
    auto_crop: Option<(usize, usize, usize, usize)>,
}

pub struct App {
    s: AppSettings,
    entries: Vec<Entry>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    job: Option<Job>,
    pending_loads: usize,
    progress: Progress,
    cancel: Cancel,
    aligned: Option<Arc<Aligned>>,
    /// Entry indices (into `entries`) used for the current alignment, in order.
    aligned_set: Vec<usize>,
    preview: Option<PreviewState>,
    preview_dirty: bool,
    preview_gen: u64,
    log: Vec<(Instant, String, bool)>,
    show_about: bool,
    show_log: bool,
    drag_accum: Vec2,
    last_change: Instant,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let s: AppSettings = cc.storage.and_then(|st| eframe::get_value(st, SETTINGS_KEY)).unwrap_or_default();
        i18n::set_lang(s.lang);
        setup_style(&cc.egui_ctx);
        let (tx, rx) = channel();
        Self {
            s,
            entries: Vec::new(),
            tx,
            rx,
            job: None,
            pending_loads: 0,
            progress: Progress::default(),
            cancel: Cancel::default(),
            aligned: None,
            aligned_set: Vec::new(),
            preview: None,
            preview_dirty: false,
            preview_gen: 0,
            log: Vec::new(),
            show_about: false,
            show_log: false,
            drag_accum: Vec2::ZERO,
            last_change: Instant::now(),
        }
    }

    pub fn add_paths_initial(&mut self, paths: Vec<PathBuf>) {
        self.add_paths(paths);
    }

    fn info(&mut self, s: impl Into<String>) {
        self.log.push((Instant::now(), s.into(), false));
    }

    fn error(&mut self, s: impl Into<String>) {
        self.log.push((Instant::now(), s.into(), true));
        self.show_log = true;
    }

    fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn add_paths(&mut self, paths: Vec<PathBuf>) {
        let paths = loader::expand_paths(&paths);
        let mut new = Vec::new();
        for p in paths {
            if self.entries.iter().any(|e| e.path == p) {
                continue;
            }
            self.entries.push(Entry { path: p.clone(), work: None, error: None, enabled: true, thumb: None });
            new.push(p);
        }
        if new.is_empty() {
            return;
        }
        if let Some(d) = new[0].parent() {
            self.s.last_dir = Some(d.to_path_buf());
        }
        self.invalidate_alignment();
        self.start_load(new);
    }

    fn start_load(&mut self, paths: Vec<PathBuf>) {
        self.job = Some(Job::Load);
        self.pending_loads += paths.len();
        self.cancel = Cancel::default();
        self.progress.set_stage(tr(Key::StageLoading));
        let tx = self.tx.clone();
        let cancel = self.cancel.clone();
        std::thread::spawn(move || {
            use rayon::prelude::*;
            // Limit parallel RAW decodes to keep memory bounded.
            let pool = rayon::ThreadPoolBuilder::new().num_threads(num_threads_for_loading()).build();
            let work = |p: &PathBuf| {
                if cancel.is_cancelled() {
                    return;
                }
                let r = project::load_work(p).map(Arc::new).map_err(|e| e.to_string());
                let _ = tx.send(Msg::Loaded(p.clone(), r));
            };
            match pool {
                Ok(pool) => pool.install(|| paths.par_iter().for_each(work)),
                Err(_) => paths.iter().for_each(work),
            }
            let _ = tx.send(Msg::LoadFinished);
        });
    }

    fn invalidate_alignment(&mut self) {
        self.aligned = None;
        self.aligned_set.clear();
        self.preview = None;
    }

    fn ready_entries(&self) -> Vec<usize> {
        (0..self.entries.len()).filter(|&i| self.entries[i].enabled && self.entries[i].work.is_some()).collect()
    }

    fn start_align(&mut self) {
        let set = self.ready_entries();
        if set.len() < 2 {
            self.error(tr(Key::ErrNotEnough));
            return;
        }
        let works: Vec<Arc<WorkImage>> = set.iter().map(|&i| self.entries[i].work.clone().unwrap()).collect();
        let mut st = self.s.align.clone();
        st.manual_hfov = if self.s.manual_fov { Some(self.s.fov_deg) } else { None };
        let ph = self.s.photo.clone();
        self.job = Some(Job::Align);
        self.cancel = Cancel::default();
        self.progress = Progress::default();
        let (tx, progress, cancel) = (self.tx.clone(), self.progress.clone(), self.cancel.clone());
        std::thread::spawn(move || {
            let t0 = Instant::now();
            let r = project::run_align(&works, &st, &ph, &progress, &cancel).map(|a| (Arc::new(a), set));
            let _ = tx.send(Msg::Aligned(r, t0.elapsed().as_secs_f64()));
        });
    }

    fn aligned_works(&self) -> Vec<Arc<WorkImage>> {
        self.aligned_set.iter().filter_map(|&i| self.entries.get(i).and_then(|e| e.work.clone())).collect()
    }

    fn start_preview(&mut self) {
        let Some(al) = self.aligned.clone() else { return };
        let works = self.aligned_works();
        if works.len() != self.aligned_set.len() {
            return;
        }
        let mut rs = self.s.render.clone();
        rs.crop = CropMode::None;
        self.preview_dirty = false;
        self.preview_gen += 1;
        let gen = self.preview_gen;
        self.job = Some(Job::Preview);
        self.cancel = Cancel::default();
        self.progress = Progress::default();
        self.progress.set_stage(tr(Key::StagePreview));
        let (tx, progress, cancel) = (self.tx.clone(), self.progress.clone(), self.cancel.clone());
        std::thread::spawn(move || {
            let r = project::render_preview(&works, &al, &rs, PREVIEW_MP, &progress, &cancel);
            let _ = tx.send(Msg::Preview(r, gen));
        });
    }

    fn start_render(&mut self) {
        let Some(al) = self.aligned.clone() else { return };
        let works = self.aligned_works();
        let any_raw = works.iter().any(|w| w.is_raw);
        let fmt = self.s.output.format.resolve(any_raw);
        let stem = works
            .first()
            .and_then(|w| w.path.file_stem())
            .map(|s| format!("{}_pano", s.to_string_lossy()))
            .unwrap_or_else(|| "panorama".into());
        let dir = works.first().and_then(|w| w.path.parent().map(|p| p.to_path_buf()));
        let mut dlg = rfd::FileDialog::new().set_file_name(format!("{}.{}", stem, fmt.extension()));
        dlg = match fmt {
            OutputFormat::Dng => dlg.add_filter("DNG", &["dng"]),
            OutputFormat::Tiff16 => dlg.add_filter("TIFF", &["tif", "tiff"]),
            OutputFormat::Png16 => dlg.add_filter("PNG", &["png"]),
            _ => dlg.add_filter("JPEG", &["jpg", "jpeg"]),
        };
        if let Some(d) = dir {
            dlg = dlg.set_directory(d);
        }
        let Some(mut path) = dlg.save_file() else { return };
        if path.extension().is_none() {
            path.set_extension(fmt.extension());
        }
        let rs = self.s.render.clone();
        let mut os = self.s.output.clone();
        os.format = fmt;
        self.job = Some(Job::Render);
        self.cancel = Cancel::default();
        self.progress = Progress::default();
        self.progress.set_stage(tr(Key::StageRendering));
        let (tx, progress, cancel) = (self.tx.clone(), self.progress.clone(), self.cancel.clone());
        std::thread::spawn(move || {
            let t0 = Instant::now();
            let r = (|| {
                let out = project::render_full(&works, &al, &rs, &progress, &cancel)?;
                progress.set_stage(tr(Key::StageSaving));
                let fmt = project::save_output(&path, &out, &works, &os)?;
                Ok((path, fmt, out.image.w, out.image.h))
            })();
            let _ = tx.send(Msg::Rendered(r, t0.elapsed().as_secs_f64()));
        });
    }

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Loaded(p, r) => {
                    self.pending_loads = self.pending_loads.saturating_sub(1);
                    if let Some(e) = self.entries.iter_mut().find(|e| e.path == p) {
                        match r {
                            Ok(w) => {
                                let (tw, th, ref rgba) = w.thumb;
                                let img = egui::ColorImage::from_rgba_unmultiplied([tw, th], rgba);
                                e.thumb = Some(ctx.load_texture(p.to_string_lossy(), img, egui::TextureOptions::LINEAR));
                                e.work = Some(w);
                            }
                            Err(err) => e.error = Some(err),
                        }
                    }
                    let total = self.entries.len().max(1);
                    let done = self.entries.iter().filter(|e| e.work.is_some() || e.error.is_some()).count();
                    self.progress.set_fraction(done as f32 / total as f32);
                }
                Msg::LoadFinished => {
                    if self.pending_loads == 0 {
                        self.job = None;
                        let errs: Vec<String> = self
                            .entries
                            .iter()
                            .filter_map(|e| e.error.as_ref().map(|er| format!("{}: {}", e.path.display(), er)))
                            .collect();
                        for e in errs {
                            self.error(e);
                        }
                        let n = self.entries.iter().filter(|e| e.work.is_some()).count();
                        self.info(format!("{} {}", tr(Key::LoadedImages), n));
                    }
                }
                Msg::Aligned(r, secs) => {
                    self.job = None;
                    match r {
                        Ok((a, set)) => {
                            let n = a.alignment.aligned_count();
                            let total = set.len();
                            let msg = format!(
                                "{}: {}/{} · RMS {:.2} px · {} · {:.1} s",
                                tr(Key::AlignDone),
                                n,
                                total,
                                a.alignment.rms_px,
                                focal_source_text(a.alignment.focal_source),
                                secs
                            );
                            self.info(msg);
                            if n < total {
                                self.error(tr(Key::WarnUnaligned));
                            }
                            self.aligned = Some(a);
                            self.aligned_set = set;
                            self.preview_dirty = true;
                        }
                        Err(StitchError::Cancelled) => self.info(tr(Key::Cancelled)),
                        Err(e) => self.error(format!("{}: {}", tr(Key::AlignFailed), e)),
                    }
                }
                Msg::Preview(r, gen) => {
                    self.job = None;
                    if gen != self.preview_gen {
                        continue;
                    }
                    match r {
                        Ok(out) => self.set_preview(ctx, out),
                        Err(StitchError::Cancelled) => {}
                        Err(e) => self.error(format!("{}: {}", tr(Key::PreviewFailed), e)),
                    }
                }
                Msg::Rendered(r, secs) => {
                    self.job = None;
                    match r {
                        Ok((path, fmt, w, h)) => {
                            self.info(format!(
                                "{} {} ({}×{}, {:.1} MP, {:?}) · {:.1} s",
                                tr(Key::Saved),
                                path.display(),
                                w,
                                h,
                                (w * h) as f64 / 1e6,
                                fmt,
                                secs
                            ));
                            if self.s.open_folder_after {
                                reveal_in_explorer(&path);
                            }
                        }
                        Err(StitchError::Cancelled) => self.info(tr(Key::Cancelled)),
                        Err(e) => self.error(format!("{}: {}", tr(Key::RenderFailed), e)),
                    }
                }
            }
        }
    }

    fn set_preview(&mut self, ctx: &egui::Context, out: RenderOutput) {
        let conv = crate::imaging::ToSrgb8::new();
        let (w, h) = (out.image.w, out.image.h);
        let mut rgba = Vec::with_capacity(w * h * 4);
        for (y, row) in out.image.data.chunks(w).enumerate() {
            for (x, p) in row.iter().enumerate() {
                if out.alpha[y * w + x] == 0 {
                    let c = if ((x / 8) + (y / 8)) % 2 == 0 { 38 } else { 48 };
                    rgba.extend_from_slice(&[c, c, c + 4, 255]);
                } else {
                    rgba.extend_from_slice(&[conv.conv(p[0]), conv.conv(p[1]), conv.conv(p[2]), 255]);
                }
            }
        }
        let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
        let tex = ctx.load_texture("preview", img, egui::TextureOptions::LINEAR);
        let auto_crop = render::auto_crop(&out.alpha, w, h);
        self.preview = Some(PreviewState { tex, w, h, alpha: out.alpha, scale: out.canvas.scale, auto_crop });
    }

    fn mark_changed(&mut self) {
        self.preview_dirty = true;
        self.last_change = Instant::now();
    }

    // ------------------------------------------------------------------------------------------

    fn menu(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button(tr(Key::MenuFile), |ui| {
                    if ui.button(tr(Key::AddImages)).clicked() {
                        ui.close_menu();
                        self.pick_files();
                    }
                    if ui.button(tr(Key::AddFolder)).clicked() {
                        ui.close_menu();
                        self.pick_folder();
                    }
                    ui.separator();
                    if ui.add_enabled(!self.busy(), egui::Button::new(tr(Key::ClearAll))).clicked() {
                        ui.close_menu();
                        self.entries.clear();
                        self.invalidate_alignment();
                    }
                    ui.separator();
                    if ui.button(tr(Key::Exit)).clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button(tr(Key::MenuLanguage), |ui| {
                    for l in [Lang::Pl, Lang::En] {
                        if ui.selectable_label(self.s.lang == l, l.name()).clicked() {
                            self.s.lang = l;
                            i18n::set_lang(l);
                            ui.close_menu();
                        }
                    }
                });
                ui.menu_button(tr(Key::MenuView), |ui| {
                    if ui.checkbox(&mut self.show_log, tr(Key::ShowLog)).clicked() {
                        ui.close_menu();
                    }
                });
                ui.menu_button(tr(Key::MenuHelp), |ui| {
                    if ui.button(tr(Key::About)).clicked() {
                        self.show_about = true;
                        ui.close_menu();
                    }
                    ui.hyperlink_to(tr(Key::ProjectPage), REPO_URL);
                });
            });
        });
    }

    fn pick_files(&mut self) {
        let mut exts: Vec<&str> = loader::RASTER_EXTENSIONS.to_vec();
        exts.extend_from_slice(crate::raw::RAW_EXTENSIONS);
        let upper: Vec<String> = exts.iter().map(|e| e.to_uppercase()).collect();
        let mut all: Vec<&str> = exts.clone();
        all.extend(upper.iter().map(|s| s.as_str()));
        let mut dlg = rfd::FileDialog::new()
            .add_filter(tr(Key::FilterImages), &all)
            .add_filter("RAW", crate::raw::RAW_EXTENSIONS)
            .add_filter("JPEG/TIFF/PNG", loader::RASTER_EXTENSIONS);
        if let Some(d) = &self.s.last_dir {
            dlg = dlg.set_directory(d);
        }
        if let Some(files) = dlg.pick_files() {
            self.add_paths(files);
        }
    }

    fn pick_folder(&mut self) {
        let mut dlg = rfd::FileDialog::new();
        if let Some(d) = &self.s.last_dir {
            dlg = dlg.set_directory(d);
        }
        if let Some(dir) = dlg.pick_folder() {
            self.add_paths(vec![dir]);
        }
    }

    fn images_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("images").resizable(true).default_width(310.0).min_width(240.0).show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading(tr(Key::Images));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let n = self.entries.len();
                    let en = self.entries.iter().filter(|e| e.enabled).count();
                    ui.label(RichText::new(format!("{}/{}", en, n)).weak());
                });
            });
            ui.horizontal(|ui| {
                if ui.button(format!("＋ {}", tr(Key::AddImages))).clicked() {
                    self.pick_files();
                }
                if ui.button(tr(Key::AddFolder)).clicked() {
                    self.pick_folder();
                }
            });
            ui.separator();
            if self.entries.is_empty() {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(tr(Key::DropHint)).weak());
                    ui.add_space(6.0);
                    ui.label(RichText::new(tr(Key::SupportedFormats)).small().weak());
                });
                return;
            }
            let mut remove: Option<usize> = None;
            let mut toggled = false;
            let aligned_status: Vec<Option<bool>> = (0..self.entries.len())
                .map(|i| {
                    let a = self.aligned.as_ref()?;
                    let k = self.aligned_set.iter().position(|&x| x == i)?;
                    Some(a.alignment.cameras[k].is_some())
                })
                .collect();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for (i, e) in self.entries.iter_mut().enumerate() {
                    let frame = egui::Frame::none()
                        .fill(ui.visuals().faint_bg_color)
                        .rounding(6.0)
                        .inner_margin(egui::Margin::same(6.0));
                    frame.show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut e.enabled, "").changed() {
                                toggled = true;
                            }
                            let size = Vec2::new(72.0, 48.0);
                            match &e.thumb {
                                Some(t) => {
                                    let ts = t.size_vec2();
                                    let sc = (size.x / ts.x).min(size.y / ts.y);
                                    ui.add(egui::Image::new(egui::load::SizedTexture::new(t.id(), ts * sc)));
                                }
                                None => {
                                    let (r, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                                    ui.painter().rect_filled(r, 4.0, ui.visuals().extreme_bg_color);
                                    if e.error.is_none() {
                                        ui.put(r, egui::Spinner::new());
                                    }
                                }
                            }
                            ui.vertical(|ui| {
                                let name = e.path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(name).strong());
                                    if let Some(w) = &e.work {
                                        if w.is_raw {
                                            ui.label(RichText::new("RAW").small().color(ACCENT));
                                        }
                                    }
                                });
                                if let Some(w) = &e.work {
                                    ui.label(RichText::new(format!("{}×{}  {}", w.full_w, w.full_h, w.meta.summary())).small().weak());
                                }
                                if let Some(err) = &e.error {
                                    ui.label(RichText::new(err).small().color(Color32::from_rgb(240, 90, 90)));
                                }
                                match aligned_status[i] {
                                    Some(true) => {
                                        ui.label(RichText::new(format!("✔ {}", tr(Key::StatusAligned))).small().color(Color32::from_rgb(110, 200, 120)));
                                    }
                                    Some(false) => {
                                        ui.label(RichText::new(format!("⚠ {}", tr(Key::StatusNotConnected))).small().color(Color32::from_rgb(240, 180, 60)));
                                    }
                                    None => {}
                                }
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("✖").on_hover_text(tr(Key::Remove)).clicked() {
                                    remove = Some(i);
                                }
                            });
                        });
                    });
                    ui.add_space(3.0);
                }
            });
            if !self.busy() {
                if let Some(i) = remove {
                    self.entries.remove(i);
                    self.invalidate_alignment();
                }
            }
            if toggled {
                self.invalidate_alignment();
            }
        });
    }

    fn settings_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("settings").resizable(true).default_width(330.0).min_width(280.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.add_space(6.0);
                self.section_alignment(ui);
                ui.add_space(4.0);
                self.section_projection(ui);
                ui.add_space(4.0);
                self.section_blending(ui);
                ui.add_space(4.0);
                self.section_crop(ui);
                ui.add_space(4.0);
                self.section_output(ui);
                ui.add_space(12.0);
            });
        });
    }

    fn section_alignment(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(RichText::new(format!("1. {}", tr(Key::SecAlignment))).strong())
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new("align_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    ui.label(tr(Key::LensType));
                    egui::ComboBox::from_id_salt("lens")
                        .selected_text(lens_name(self.s.align.lens))
                        .width(170.0)
                        .show_ui(ui, |ui| {
                            for l in LensType::ALL {
                                ui.selectable_value(&mut self.s.align.lens, l, lens_name(l));
                            }
                        });
                    ui.end_row();

                    ui.label(tr(Key::FieldOfView));
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.s.manual_fov, tr(Key::Manual));
                        ui.add_enabled(
                            self.s.manual_fov,
                            egui::DragValue::new(&mut self.s.fov_deg).range(5.0..=360.0).speed(0.2).suffix("°"),
                        );
                    })
                    .response
                    .on_hover_text(tr(Key::FovHint));
                    ui.end_row();

                    ui.label(tr(Key::Features));
                    ui.add(egui::Slider::new(&mut self.s.align.max_features, 500..=8000).step_by(250.0));
                    ui.end_row();

                    ui.label(tr(Key::Matching));
                    egui::ComboBox::from_id_salt("strategy")
                        .selected_text(strategy_name(self.s.align.strategy))
                        .width(170.0)
                        .show_ui(ui, |ui| {
                            for s in [MatchStrategy::Auto, MatchStrategy::AllPairs, MatchStrategy::Sequential] {
                                ui.selectable_value(&mut self.s.align.strategy, s, strategy_name(s));
                            }
                        });
                    ui.end_row();
                    if self.s.align.strategy != MatchStrategy::AllPairs {
                        ui.label(tr(Key::Neighbours));
                        ui.add(egui::Slider::new(&mut self.s.align.window, 1..=10));
                        ui.end_row();
                    }
                    ui.label(tr(Key::RansacThreshold));
                    ui.add(egui::Slider::new(&mut self.s.align.ransac_threshold_px, 1.0..=10.0).suffix(" px"));
                    ui.end_row();
                });
                ui.checkbox(&mut self.s.align.optimize_focal, tr(Key::OptimizeFocal));
                ui.checkbox(&mut self.s.align.straighten, tr(Key::Straighten));
                ui.add_space(6.0);
                let ready = self.ready_entries().len();
                let btn = egui::Button::new(RichText::new(format!("⟲  {}", tr(Key::Align))).size(15.0).strong())
                    .min_size(Vec2::new(ui.available_width(), 32.0))
                    .fill(Color32::from_rgb(52, 92, 160));
                if ui.add_enabled(!self.busy() && ready >= 2, btn).clicked() {
                    self.start_align();
                }
                if let Some(a) = &self.aligned {
                    let al = &a.alignment;
                    let hf = al
                        .cameras
                        .iter()
                        .flatten()
                        .next()
                        .map(|c| al.lens.hfov_from_focal(c.f, c.w as f64))
                        .unwrap_or(0.0);
                    ui.label(
                        RichText::new(format!(
                            "{} {}/{} · RMS {:.2} px · HFOV {:.1}°",
                            tr(Key::Aligned),
                            al.aligned_count(),
                            al.cameras.len(),
                            al.rms_px,
                            hf
                        ))
                        .small()
                        .weak(),
                    );
                }
            });
    }

    fn section_projection(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(RichText::new(format!("2. {}", tr(Key::SecProjection))).strong())
            .default_open(true)
            .show(ui, |ui| {
                let before = (self.s.render.projection, self.s.render.full_sphere, self.s.render.yaw, self.s.render.pitch, self.s.render.roll);
                egui::ComboBox::from_id_salt("proj")
                    .selected_text(projection_name(self.s.render.projection))
                    .width(ui.available_width() - 8.0)
                    .show_ui(ui, |ui| {
                        for p in Projection::ALL {
                            ui.selectable_value(&mut self.s.render.projection, p, projection_name(p)).on_hover_text(projection_desc(p));
                        }
                    });
                ui.label(RichText::new(projection_desc(self.s.render.projection)).small().weak());
                ui.add_enabled(
                    self.s.render.projection == Projection::Equirectangular,
                    egui::Checkbox::new(&mut self.s.render.full_sphere, tr(Key::FullSphere)),
                )
                .on_hover_text(tr(Key::FullSphereHint));
                ui.add_space(4.0);
                egui::Grid::new("view_grid").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    ui.label(tr(Key::Yaw));
                    ui.add(egui::Slider::new(&mut self.s.render.yaw, -180.0..=180.0).suffix("°"));
                    ui.end_row();
                    ui.label(tr(Key::Pitch));
                    ui.add(egui::Slider::new(&mut self.s.render.pitch, -90.0..=90.0).suffix("°"));
                    ui.end_row();
                    ui.label(tr(Key::Roll));
                    ui.add(egui::Slider::new(&mut self.s.render.roll, -180.0..=180.0).suffix("°"));
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    if ui.button(tr(Key::ResetView)).clicked() {
                        self.s.render.yaw = 0.0;
                        self.s.render.pitch = 0.0;
                        self.s.render.roll = 0.0;
                    }
                    ui.label(RichText::new(tr(Key::DragHint)).small().weak());
                });
                ui.checkbox(&mut self.s.render.circular_fisheye, tr(Key::CircularFisheye));
                let after = (self.s.render.projection, self.s.render.full_sphere, self.s.render.yaw, self.s.render.pitch, self.s.render.roll);
                if before != after {
                    self.mark_changed();
                }
            });
    }

    fn section_blending(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(RichText::new(format!("3. {}", tr(Key::SecBlending))).strong())
            .default_open(true)
            .show(ui, |ui| {
                let before = (self.s.render.blend, self.s.render.bands);
                egui::Grid::new("blend_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    ui.label(tr(Key::BlendMode));
                    egui::ComboBox::from_id_salt("blend")
                        .selected_text(blend_name(self.s.render.blend))
                        .width(170.0)
                        .show_ui(ui, |ui| {
                            for b in [BlendMode::Multiband, BlendMode::Feather, BlendMode::Seams] {
                                ui.selectable_value(&mut self.s.render.blend, b, blend_name(b));
                            }
                        });
                    ui.end_row();
                    if self.s.render.blend == BlendMode::Multiband {
                        ui.label(tr(Key::Bands));
                        ui.add(egui::Slider::new(&mut self.s.render.bands, 0..=10).custom_formatter(|v, _| {
                            if v == 0.0 {
                                tr(Key::AutoShort).to_string()
                            } else {
                                format!("{}", v as usize)
                            }
                        }));
                        ui.end_row();
                    }
                });
                let pb = (self.s.photo.exposure, self.s.photo.color, self.s.photo.vignetting);
                ui.checkbox(&mut self.s.photo.exposure, tr(Key::ExposureComp));
                ui.checkbox(&mut self.s.photo.color, tr(Key::ColorComp));
                ui.checkbox(&mut self.s.photo.vignetting, tr(Key::VignettingComp));
                ui.checkbox(&mut self.s.render.bicubic, tr(Key::Bicubic));
                let pa = (self.s.photo.exposure, self.s.photo.color, self.s.photo.vignetting);
                if pb != pa {
                    if let Some(a) = self.aligned.clone() {
                        let works = self.aligned_works();
                        let photo = project::recompute_photo(&works, &a.alignment, &self.s.photo);
                        self.aligned = Some(Arc::new(Aligned { alignment: a.alignment.clone(), photo }));
                    }
                    self.mark_changed();
                }
                if before != (self.s.render.blend, self.s.render.bands) {
                    self.mark_changed();
                }
            });
    }

    fn section_crop(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(RichText::new(format!("4. {}", tr(Key::SecCrop))).strong())
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.s.render.crop, CropMode::Auto, tr(Key::CropAuto));
                    ui.selectable_value(&mut self.s.render.crop, CropMode::None, tr(Key::CropNone));
                    ui.selectable_value(&mut self.s.render.crop, CropMode::Manual, tr(Key::CropManual));
                });
                if self.s.render.crop == CropMode::Manual {
                    let labels = [tr(Key::Left), tr(Key::Top), tr(Key::Right), tr(Key::Bottom)];
                    egui::Grid::new("crop_grid").num_columns(2).show(ui, |ui| {
                        for (k, l) in labels.iter().enumerate() {
                            ui.label(*l);
                            let mut pct = self.s.render.manual_crop[k] * 100.0;
                            if ui.add(egui::Slider::new(&mut pct, 0.0..=45.0).suffix(" %")).changed() {
                                self.s.render.manual_crop[k] = pct / 100.0;
                            }
                            ui.end_row();
                        }
                    });
                    if let Some(pv) = &self.preview {
                        if ui.button(tr(Key::CropFromAuto)).clicked() {
                            if let Some((x, y, w, h)) = pv.auto_crop {
                                self.s.render.manual_crop = [
                                    x as f32 / pv.w as f32,
                                    y as f32 / pv.h as f32,
                                    (pv.w - x - w) as f32 / pv.w as f32,
                                    (pv.h - y - h) as f32 / pv.h as f32,
                                ];
                            }
                        }
                    }
                }
            });
    }

    fn section_output(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(RichText::new(format!("5. {}", tr(Key::SecOutput))).strong())
            .default_open(true)
            .show(ui, |ui| {
                let any_raw = self.aligned_works().iter().any(|w| w.is_raw)
                    || (self.aligned.is_none() && self.entries.iter().any(|e| e.work.as_ref().map(|w| w.is_raw).unwrap_or(false)));
                egui::Grid::new("out_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    ui.label(tr(Key::Format));
                    egui::ComboBox::from_id_salt("fmt")
                        .selected_text(format_name(self.s.output.format, any_raw))
                        .width(190.0)
                        .show_ui(ui, |ui| {
                            for f in OutputFormat::ALL {
                                ui.selectable_value(&mut self.s.output.format, f, format_name(f, any_raw));
                            }
                        });
                    ui.end_row();
                    let fmt = self.s.output.format.resolve(any_raw);
                    if fmt == OutputFormat::Jpeg {
                        ui.label(tr(Key::Quality));
                        ui.add(egui::Slider::new(&mut self.s.output.jpeg_quality, 50..=100));
                        ui.end_row();
                    }
                    ui.label(tr(Key::Size));
                    let mut pct = self.s.render.scale * 100.0;
                    if ui.add(egui::Slider::new(&mut pct, 10.0..=150.0).suffix(" %")).changed() {
                        self.s.render.scale = pct / 100.0;
                    }
                    ui.end_row();
                    ui.label(tr(Key::MaxMegapixels));
                    ui.add(egui::DragValue::new(&mut self.s.render.max_megapixels).range(1.0..=2000.0).speed(1.0).suffix(" MP"));
                    ui.end_row();
                    if matches!(fmt, OutputFormat::Png16 | OutputFormat::Tiff16) {
                        ui.label("");
                        ui.checkbox(&mut self.s.output.transparent, tr(Key::Transparent));
                        ui.end_row();
                    }
                });
                if self.s.render.projection == Projection::Equirectangular {
                    ui.checkbox(&mut self.s.output.gpano, tr(Key::Gpano)).on_hover_text(tr(Key::GpanoHint));
                }
                ui.checkbox(&mut self.s.open_folder_after, tr(Key::OpenFolder));
                if let Some(a) = &self.aligned {
                    if let Some((w, h)) = project::estimate_canvas(a, &self.s.render) {
                        let mp = (w * h) as f64 / 1e6;
                        ui.label(
                            RichText::new(format!("{}: {}×{} ({:.1} MP) · ~{:.1} GB RAM", tr(Key::Estimated), w, h, mp, mp * 40.0 / 1024.0))
                                .small()
                                .weak(),
                        );
                    }
                }
                ui.add_space(6.0);
                let btn = egui::Button::new(RichText::new(format!("⬇  {}", tr(Key::RenderSave))).size(15.0).strong().color(Color32::BLACK))
                    .min_size(Vec2::new(ui.available_width(), 34.0))
                    .fill(ACCENT);
                if ui.add_enabled(!self.busy() && self.aligned.is_some(), btn).clicked() {
                    self.start_render();
                }
            });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                if let Some(job) = self.job {
                    let f = self.progress.fraction();
                    let stage = self.progress.stage();
                    ui.add(egui::Spinner::new());
                    ui.add(egui::ProgressBar::new(f).desired_width(260.0).show_percentage());
                    ui.label(stage);
                    if job != Job::Load || self.pending_loads > 0 {
                        if ui.button(tr(Key::Cancel)).clicked() {
                            self.cancel.cancel();
                        }
                    }
                } else if let Some((_, msg, err)) = self.log.last() {
                    let c = if *err { Color32::from_rgb(240, 110, 100) } else { ui.visuals().weak_text_color() };
                    ui.label(RichText::new(msg).color(c));
                } else {
                    ui.label(RichText::new(tr(Key::Ready)).weak());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("Panoramer {}", env!("CARGO_PKG_VERSION"))).small().weak());
                });
            });
            ui.add_space(2.0);
        });
    }

    fn central(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().frame(egui::Frame::none().fill(Color32::from_rgb(22, 24, 30))).show(ctx, |ui| {
            let avail = ui.available_rect_before_wrap();
            if let Some(pv) = &self.preview {
                let ts = Vec2::new(pv.w as f32, pv.h as f32);
                let margin = 16.0;
                let sc = ((avail.width() - 2.0 * margin) / ts.x).min((avail.height() - 2.0 * margin) / ts.y).max(0.01);
                let size = ts * sc;
                let rect = egui::Rect::from_center_size(avail.center(), size);
                let resp = ui.allocate_rect(rect, egui::Sense::drag());
                ui.painter().image(pv.tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                // crop overlay
                let crop = match self.s.render.crop {
                    CropMode::None => None,
                    CropMode::Auto => pv.auto_crop.map(|(x, y, w, h)| (x as f32, y as f32, (x + w) as f32, (y + h) as f32)),
                    CropMode::Manual => {
                        let [l, t, r, b] = self.s.render.manual_crop;
                        Some((l * ts.x, t * ts.y, (1.0 - r) * ts.x, (1.0 - b) * ts.y))
                    }
                };
                if let Some((x0, y0, x1, y1)) = crop {
                    let cr = egui::Rect::from_min_max(rect.min + Vec2::new(x0, y0) * sc, rect.min + Vec2::new(x1, y1) * sc);
                    let shade = Color32::from_black_alpha(140);
                    let p = ui.painter();
                    p.rect_filled(egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, cr.min.y)), 0.0, shade);
                    p.rect_filled(egui::Rect::from_min_max(egui::pos2(rect.min.x, cr.max.y), rect.max), 0.0, shade);
                    p.rect_filled(egui::Rect::from_min_max(egui::pos2(rect.min.x, cr.min.y), egui::pos2(cr.min.x, cr.max.y)), 0.0, shade);
                    p.rect_filled(egui::Rect::from_min_max(egui::pos2(cr.max.x, cr.min.y), egui::pos2(rect.max.x, cr.max.y)), 0.0, shade);
                    p.rect_stroke(cr, 0.0, egui::Stroke::new(1.5_f32, ACCENT));
                }
                // horizon guide while dragging
                if resp.dragged() {
                    self.drag_accum += resp.drag_delta();
                    let p = ui.painter();
                    p.line_segment([egui::pos2(rect.min.x, rect.center().y), egui::pos2(rect.max.x, rect.center().y)], egui::Stroke::new(1.0_f32, Color32::from_white_alpha(90)));
                    p.line_segment([egui::pos2(rect.center().x, rect.min.y), egui::pos2(rect.center().x, rect.max.y)], egui::Stroke::new(1.0_f32, Color32::from_white_alpha(60)));
                    let deg_per_px = (1.0 / pv.scale).to_degrees() / sc as f64;
                    p.text(
                        rect.left_top() + Vec2::new(10.0, 10.0),
                        egui::Align2::LEFT_TOP,
                        format!("Δ yaw {:+.1}°  Δ pitch {:+.1}°", -self.drag_accum.x as f64 * deg_per_px, self.drag_accum.y as f64 * deg_per_px),
                        egui::FontId::proportional(14.0),
                        Color32::WHITE,
                    );
                }
                if resp.drag_stopped() {
                    let deg_per_px = (1.0 / pv.scale).to_degrees() / sc as f64;
                    let r = &mut self.s.render;
                    r.yaw = wrap_deg(r.yaw - self.drag_accum.x as f64 * deg_per_px);
                    r.pitch = (r.pitch + self.drag_accum.y as f64 * deg_per_px).clamp(-90.0, 90.0);
                    self.drag_accum = Vec2::ZERO;
                    self.preview_dirty = true;
                    self.last_change = Instant::now();
                }
                let info = format!("{} {}×{}", tr(Key::PreviewLabel), pv.w, pv.h);
                ui.painter().text(
                    avail.right_bottom() - Vec2::new(10.0, 8.0),
                    egui::Align2::RIGHT_BOTTOM,
                    info,
                    egui::FontId::proportional(12.0),
                    Color32::from_white_alpha(120),
                );
            } else {
                ui.allocate_ui_at_rect(avail, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(avail.height() * 0.35);
                        ui.label(RichText::new("Panoramer").size(34.0).strong().color(ACCENT));
                        ui.add_space(8.0);
                        let hint = if self.entries.is_empty() {
                            tr(Key::WelcomeAdd)
                        } else if self.aligned.is_none() {
                            tr(Key::WelcomeAlign)
                        } else {
                            tr(Key::StagePreview)
                        };
                        ui.label(RichText::new(hint).size(16.0).weak());
                    });
                });
            }
        });
    }

    fn windows(&mut self, ctx: &egui::Context) {
        if self.show_about {
            let mut open = true;
            egui::Window::new(tr(Key::About)).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
                ui.label(RichText::new(format!("Panoramer {}", env!("CARGO_PKG_VERSION"))).size(20.0).strong().color(ACCENT));
                ui.label(tr(Key::AboutText));
                ui.add_space(6.0);
                ui.label(RichText::new("License: GPL-3.0-or-later").small());
                ui.hyperlink(REPO_URL);
            });
            self.show_about = open;
        }
        if self.show_log {
            let mut open = true;
            egui::Window::new(tr(Key::Log)).open(&mut open).default_size([520.0, 260.0]).show(ctx, |ui| {
                egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                    for (_, m, err) in &self.log {
                        let c = if *err { Color32::from_rgb(240, 110, 100) } else { ui.visuals().text_color() };
                        ui.label(RichText::new(m).color(c).monospace());
                    }
                });
                if ui.button(tr(Key::ClearLog)).clicked() {
                    self.log.clear();
                }
            });
            self.show_log = open;
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        // drag & drop
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() {
            self.add_paths(dropped);
        }
        self.menu(ctx);
        self.status_bar(ctx);
        self.images_panel(ctx);
        self.settings_panel(ctx);
        self.central(ctx);
        self.windows(ctx);
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let r = ctx.screen_rect();
            let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("dnd")));
            p.rect_filled(r, 0.0, Color32::from_black_alpha(160));
            p.text(r.center(), egui::Align2::CENTER_CENTER, tr(Key::DropHere), egui::FontId::proportional(26.0), ACCENT);
        }

        // Debounced preview refresh
        if self.preview_dirty && self.aligned.is_some() {
            if self.job == Some(Job::Preview) {
                self.cancel.cancel();
            } else if self.job.is_none() && self.last_change.elapsed().as_millis() > 250 {
                self.start_preview();
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if self.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SETTINGS_KEY, &self.s);
    }
}

fn wrap_deg(v: f64) -> f64 {
    let mut v = v % 360.0;
    if v > 180.0 {
        v -= 360.0;
    }
    if v < -180.0 {
        v += 360.0;
    }
    v
}

fn num_threads_for_loading() -> usize {
    let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    n.clamp(1, 6)
}

fn reveal_in_explorer(path: &std::path::Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(d) = path.parent() {
            let _ = std::process::Command::new("xdg-open").arg(d).spawn();
        }
    }
}

fn setup_style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.selection.bg_fill = Color32::from_rgb(52, 92, 160);
    v.hyperlink_color = ACCENT;
    v.window_rounding = egui::Rounding::same(8.0);
    v.panel_fill = Color32::from_rgb(30, 33, 41);
    v.faint_bg_color = Color32::from_rgb(38, 42, 52);
    v.extreme_bg_color = Color32::from_rgb(20, 22, 28);
    v.widgets.noninteractive.bg_fill = Color32::from_rgb(30, 33, 41);
    ctx.set_visuals(v);
    let mut st = (*ctx.style()).clone();
    st.spacing.item_spacing = Vec2::new(8.0, 6.0);
    st.spacing.button_padding = Vec2::new(10.0, 5.0);
    ctx.set_style(st);
}

pub fn lens_name(l: LensType) -> &'static str {
    match l {
        LensType::Rectilinear => tr(Key::LensRectilinear),
        LensType::FisheyeEquidistant => tr(Key::LensFisheyeEquidistant),
        LensType::FisheyeEquisolid => tr(Key::LensFisheyeEquisolid),
        LensType::FisheyeStereographic => tr(Key::LensFisheyeStereographic),
    }
}

fn strategy_name(s: MatchStrategy) -> &'static str {
    match s {
        MatchStrategy::Auto => tr(Key::StrategyAuto),
        MatchStrategy::AllPairs => tr(Key::StrategyAll),
        MatchStrategy::Sequential => tr(Key::StrategySequential),
    }
}

fn blend_name(b: BlendMode) -> &'static str {
    match b {
        BlendMode::Multiband => tr(Key::BlendMultiband),
        BlendMode::Feather => tr(Key::BlendFeather),
        BlendMode::Seams => tr(Key::BlendSeams),
    }
}

fn format_name(f: OutputFormat, any_raw: bool) -> String {
    match f {
        OutputFormat::Auto => format!("{} ({})", tr(Key::FormatAuto), if any_raw { "DNG" } else { "JPEG" }),
        OutputFormat::Jpeg => "JPEG (8-bit sRGB)".into(),
        OutputFormat::Tiff16 => "TIFF (16-bit sRGB)".into(),
        OutputFormat::Png16 => "PNG (16-bit sRGB)".into(),
        OutputFormat::Dng => format!("DNG ({})", tr(Key::DngDesc)),
    }
}

fn focal_source_text(f: FocalSource) -> &'static str {
    match f {
        FocalSource::Manual => tr(Key::FocalManual),
        FocalSource::Exif => tr(Key::FocalExif),
        FocalSource::Estimated => tr(Key::FocalEstimated),
        FocalSource::Default => tr(Key::FocalDefault),
    }
}

pub fn projection_name(p: Projection) -> &'static str {
    use Projection::*;
    match p {
        Equirectangular => tr(Key::ProjEquirect),
        Cylindrical => tr(Key::ProjCylindrical),
        Mercator => tr(Key::ProjMercator),
        Rectilinear => tr(Key::ProjRectilinear),
        Panini => tr(Key::ProjPanini),
        Fisheye => tr(Key::ProjFisheye),
        Stereographic => tr(Key::ProjStereographic),
        LittlePlanet => tr(Key::ProjLittlePlanet),
        Orthographic => tr(Key::ProjOrthographic),
        TransverseMercator => tr(Key::ProjTransverseMercator),
        Sinusoidal => tr(Key::ProjSinusoidal),
    }
}

fn projection_desc(p: Projection) -> &'static str {
    use Projection::*;
    match p {
        Equirectangular => tr(Key::DescEquirect),
        Cylindrical => tr(Key::DescCylindrical),
        Mercator => tr(Key::DescMercator),
        Rectilinear => tr(Key::DescRectilinear),
        Panini => tr(Key::DescPanini),
        Fisheye => tr(Key::DescFisheye),
        Stereographic => tr(Key::DescStereographic),
        LittlePlanet => tr(Key::DescLittlePlanet),
        Orthographic => tr(Key::DescOrthographic),
        TransverseMercator => tr(Key::DescTransverseMercator),
        Sinusoidal => tr(Key::DescSinusoidal),
    }
}
