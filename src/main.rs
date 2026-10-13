#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod app;
mod i18n;
mod imaging;
mod loader;
mod output;
mod project;
mod raw;
mod stitch;

use std::path::PathBuf;

const ICON_PNG: &[u8] = include_bytes!("../assets/icon256.png");

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--cli" || a == "-h" || a == "--help" || a == "--version") {
        attach_console();
        std::process::exit(cli::run(&args));
    }
    let files: Vec<PathBuf> = args.iter().filter(|a| !a.starts_with('-')).map(PathBuf::from).collect();

    let icon = image::load_from_memory(ICON_PNG).ok().map(|i| {
        let i = i.to_rgba8();
        let (w, h) = i.dimensions();
        egui::IconData { rgba: i.into_raw(), width: w, height: h }
    });
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Panoramer")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([980.0, 620.0])
        .with_drag_and_drop(true);
    if let Some(i) = icon {
        viewport = viewport.with_icon(std::sync::Arc::new(i));
    }
    let options = eframe::NativeOptions { viewport, persist_window: true, ..Default::default() };
    let r = eframe::run_native(
        "Panoramer",
        options,
        Box::new(move |cc| {
            let mut a = app::App::new(cc);
            if !files.is_empty() {
                a.add_paths_initial(files);
            }
            Ok(Box::new(a))
        }),
    );
    if let Err(e) = r {
        eprintln!("Panoramer: {e}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "windows")]
fn attach_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(pid: u32) -> i32;
    }
    unsafe {
        AttachConsole(0xFFFF_FFFF);
    }
}

#[cfg(not(target_os = "windows"))]
fn attach_console() {}

mod cli {
    use crate::i18n::{self, Lang};
    use crate::output::{OutputFormat, OutputSettings};
    use crate::project;
    use crate::stitch::align::AlignSettings;
    use crate::stitch::geometry::{LensType, Projection};
    use crate::stitch::render::{BlendMode, CropMode, RenderSettings};
    use crate::stitch::{Cancel, Progress};
    use std::path::PathBuf;
    use std::sync::Arc;

    const HELP: &str = "Panoramer — panorama stitcher

USAGE:
    panoramer --cli [OPTIONS] -o <OUTPUT> <IMAGES or FOLDERS>...
    panoramer [IMAGES...]            (opens the GUI)

OPTIONS:
    -o, --output <FILE>        Output file (.jpg, .tif, .png, .dng)
    -p, --projection <NAME>    equirectangular | cylindrical | mercator | rectilinear | panini |
                               fisheye | stereographic | little-planet | orthographic |
                               transverse-mercator | sinusoidal   [default: cylindrical]
        --full-sphere          Full 360x180 canvas (equirectangular)
    -l, --lens <TYPE>          rectilinear | fisheye | equisolid | fisheye-stereo
        --fov <DEG>            Horizontal field of view of one image (overrides EXIF)
        --scale <PCT>          Output resolution in percent of native [default: 100]
        --max-mp <MP>          Output size limit in megapixels [default: 300]
    -q, --quality <1-100>      JPEG quality [default: 95]
        --format <FMT>         auto | jpeg | tiff | png | dng   [default: from extension]
        --blend <MODE>         multiband | feather | seams
        --no-crop              Disable automatic cropping
        --no-exposure          Disable exposure compensation
        --no-vignetting        Disable vignetting correction
        --no-straighten        Disable horizon levelling
        --yaw/--pitch/--roll <DEG>  View rotation
        --lang <pl|en>         Message language
    -h, --help                 Show this help
        --version              Show version";

    fn parse_projection(s: &str) -> Option<Projection> {
        Some(match s.to_ascii_lowercase().replace('_', "-").as_str() {
            "equirectangular" | "spherical" | "equirect" | "360" => Projection::Equirectangular,
            "cylindrical" | "cylinder" => Projection::Cylindrical,
            "mercator" => Projection::Mercator,
            "rectilinear" | "perspective" => Projection::Rectilinear,
            "panini" => Projection::Panini,
            "fisheye" => Projection::Fisheye,
            "stereographic" => Projection::Stereographic,
            "little-planet" | "planet" => Projection::LittlePlanet,
            "orthographic" => Projection::Orthographic,
            "transverse-mercator" | "vertical" => Projection::TransverseMercator,
            "sinusoidal" => Projection::Sinusoidal,
            _ => return None,
        })
    }

    pub fn run(args: &[String]) -> i32 {
        if args.iter().any(|a| a == "--version") {
            println!("Panoramer {}", env!("CARGO_PKG_VERSION"));
            return 0;
        }
        if args.iter().any(|a| a == "-h" || a == "--help") {
            println!("{HELP}");
            return 0;
        }
        i18n::set_lang(Lang::detect());
        let mut st = AlignSettings::default();
        let mut rs = RenderSettings::default();
        let mut os = OutputSettings::default();
        let mut ph = project::PhotoOptions::default();
        let mut out: Option<PathBuf> = None;
        let mut fmt: Option<OutputFormat> = None;
        let mut inputs: Vec<PathBuf> = Vec::new();
        let mut it = args.iter().peekable();
        macro_rules! val {
            ($name:expr) => {
                match it.next() {
                    Some(v) => v.clone(),
                    None => {
                        eprintln!("missing value for {}", $name);
                        return 2;
                    }
                }
            };
        }
        macro_rules! num {
            ($name:expr) => {{
                let v = val!($name);
                match v.parse::<f64>() {
                    Ok(x) => x,
                    Err(_) => {
                        eprintln!("invalid number for {}: {}", $name, v);
                        return 2;
                    }
                }
            }};
        }
        while let Some(a) = it.next() {
            match a.as_str() {
                "--cli" => {}
                "-o" | "--output" => out = Some(PathBuf::from(val!(a))),
                "-p" | "--projection" => {
                    let v = val!(a);
                    match parse_projection(&v) {
                        Some(p) => rs.projection = p,
                        None => {
                            eprintln!("unknown projection: {v}");
                            return 2;
                        }
                    }
                }
                "--full-sphere" => rs.full_sphere = true,
                "-l" | "--lens" => {
                    st.lens = match val!(a).to_ascii_lowercase().as_str() {
                        "fisheye" | "equidistant" => LensType::FisheyeEquidistant,
                        "equisolid" => LensType::FisheyeEquisolid,
                        "fisheye-stereo" | "stereographic" => LensType::FisheyeStereographic,
                        _ => LensType::Rectilinear,
                    }
                }
                "--fov" => st.manual_hfov = Some(num!(a)),
                "--scale" => rs.scale = num!(a) / 100.0,
                "--max-mp" => rs.max_megapixels = num!(a),
                "-q" | "--quality" => os.jpeg_quality = num!(a).clamp(1.0, 100.0) as u8,
                "--format" => {
                    fmt = Some(match val!(a).to_ascii_lowercase().as_str() {
                        "jpeg" | "jpg" => OutputFormat::Jpeg,
                        "tiff" | "tif" => OutputFormat::Tiff16,
                        "png" => OutputFormat::Png16,
                        "dng" => OutputFormat::Dng,
                        _ => OutputFormat::Auto,
                    })
                }
                "--blend" => {
                    rs.blend = match val!(a).to_ascii_lowercase().as_str() {
                        "feather" => BlendMode::Feather,
                        "seams" | "none" => BlendMode::Seams,
                        _ => BlendMode::Multiband,
                    }
                }
                "--no-crop" => rs.crop = CropMode::None,
                "--no-exposure" => {
                    ph.exposure = false;
                    ph.color = false;
                }
                "--no-vignetting" => ph.vignetting = false,
                "--no-straighten" => st.straighten = false,
                "--yaw" => rs.yaw = num!(a),
                "--pitch" => rs.pitch = num!(a),
                "--roll" => rs.roll = num!(a),
                "--lang" => {
                    if let Some(l) = Lang::from_code(&val!(a)) {
                        i18n::set_lang(l);
                    }
                }
                s if s.starts_with('-') => {
                    eprintln!("unknown option: {s}\n\n{HELP}");
                    return 2;
                }
                _ => inputs.push(PathBuf::from(a)),
            }
        }
        let Some(out) = out else {
            eprintln!("missing --output\n\n{HELP}");
            return 2;
        };
        let inputs = crate::loader::expand_paths(&inputs);
        if inputs.len() < 2 {
            eprintln!("{}", crate::i18n::tr(crate::i18n::Key::ErrNotEnough));
            return 2;
        }
        os.format = fmt.unwrap_or_else(|| match out.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
            Some("dng") => OutputFormat::Dng,
            Some("tif") | Some("tiff") => OutputFormat::Tiff16,
            Some("png") => OutputFormat::Png16,
            Some("jpg") | Some("jpeg") => OutputFormat::Jpeg,
            _ => OutputFormat::Auto,
        });
        let t0 = std::time::Instant::now();
        let progress = Progress::default();
        let cancel = Cancel::default();
        eprintln!("[1/4] {} ({})", crate::i18n::tr(crate::i18n::Key::StageLoading), inputs.len());
        let works: Vec<Arc<project::WorkImage>> = {
            use rayon::prelude::*;
            let r: Vec<_> = inputs.par_iter().map(|p| (p, project::load_work(p))).collect();
            let mut v = Vec::new();
            for (p, w) in r {
                match w {
                    Ok(w) => v.push(Arc::new(w)),
                    Err(e) => eprintln!("  ! {}: {}", p.display(), e),
                }
            }
            v
        };
        eprintln!("[2/4] {}", crate::i18n::tr(crate::i18n::Key::StageMatching));
        let al = match project::run_align(&works, &st, &ph, &progress, &cancel) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("{}: {}", crate::i18n::tr(crate::i18n::Key::AlignFailed), e);
                return 1;
            }
        };
        eprintln!(
            "      {}/{} · RMS {:.2} px",
            al.alignment.aligned_count(),
            works.len(),
            al.alignment.rms_px
        );
        eprintln!("[3/4] {}", crate::i18n::tr(crate::i18n::Key::StageRendering));
        let res = match project::render_full(&works, &al, &rs, &progress, &cancel) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("{}: {}", crate::i18n::tr(crate::i18n::Key::RenderFailed), e);
                return 1;
            }
        };
        eprintln!("[4/4] {} {}", crate::i18n::tr(crate::i18n::Key::StageSaving), out.display());
        match project::save_output(&out, &res, &works, &os) {
            Ok(f) => {
                eprintln!(
                    "{} {} ({}×{}, {:?}) · {:.1} s",
                    crate::i18n::tr(crate::i18n::Key::Saved),
                    out.display(),
                    res.image.w,
                    res.image.h,
                    f,
                    t0.elapsed().as_secs_f64()
                );
                0
            }
            Err(e) => {
                eprintln!("{}", e);
                1
            }
        }
    }
}
