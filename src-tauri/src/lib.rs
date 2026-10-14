//! Panoramer application (desktop + mobile).

mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let files: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with('-')).collect();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .manage(commands::Shared::default())
        .manage(StartupFiles(files))
        .invoke_handler(tauri::generate_handler![
            commands::set_language,
            commands::system_language,
            commands::load_settings,
            commands::save_settings,
            commands::cancel_job,
            commands::supported_extensions,
            commands::add_images,
            commands::remove_images,
            commands::align,
            commands::set_photometric,
            commands::build_sphere,
            commands::sphere_pixels,
            commands::layout,
            commands::render_and_save,
            commands::any_raw,
            commands::add_image_bytes,
            commands::read_file_bytes,
            commands::cache_dir,
            commands::work_preview,
            startup_files,
            auto_align_flag,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Panoramer");
}

struct StartupFiles(Vec<String>);

#[tauri::command]
fn startup_files(state: tauri::State<'_, StartupFiles>) -> Vec<String> {
    state.0.clone()
}

/// Hidden helper for automated UI checks.
#[tauri::command]
fn auto_align_flag() -> bool {
    std::env::var_os("PANORAMER_AUTOALIGN").is_some()
}

