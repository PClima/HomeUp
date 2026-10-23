#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod annot;
mod bim;
mod cadimport;
mod app;
mod camera;
mod camtools;
mod chrome;
mod clipboard;
mod desktop;
mod editops;
mod doc;
mod export;
mod filemenu;
mod hatch;
mod geom;
mod gpupt;
mod icons;
mod icons2;
mod imageimport;
mod import;
mod matchphoto;
mod library;
mod lights;
mod modops;
mod panels;
mod pathtrace;
mod pick;
mod render;
mod sandbox;
mod scene;
mod settings;
mod skp;
mod skpfile;
mod snapshot;
mod tools;
mod tools2;
mod tools3;
mod procedural;
mod report;
mod templates;
mod tex;
mod texpos;
mod thumbs;
mod trayui;
mod units;
mod watermark;

use eframe::egui;

fn main() -> eframe::Result {
    // Without a console (release), panics would vanish: keep them in a log file.
    std::panic::set_hook(Box::new(|info| {
        let bt = std::backtrace::Backtrace::force_capture();
        let msg = format!("[{:?}] {info}
{bt}
", std::time::SystemTime::now());
        eprintln!("{msg}");
        let dir = filemenu::app_dir();
        let _ = std::fs::create_dir_all(&dir);
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("crash.log")) {
            let _ = f.write_all(msg.as_bytes());
        }
    }));
    let snapshot = std::env::var("HOMEUP_SNAPSHOT").is_ok();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("HomeUp")
        .with_inner_size([1400.0, 860.0])
        .with_min_inner_size([800.0, 500.0]);
    if let Ok(img) = image::load_from_memory(include_bytes!("../assets/homeup-256.png")) {
        let img = img.to_rgba8();
        let (width, height) = img.dimensions();
        viewport = viewport.with_icon(egui::IconData { rgba: img.into_raw(), width, height });
    }
    if snapshot {
        // render off-screen, without stealing focus from the user
        viewport = viewport.with_position([-6000.0, 100.0]).with_active(false).with_taskbar(false);
    }
    // ask the GPU for its real limits (large buffers and textures for high-resolution renders)
    let setup = eframe::egui_wgpu::WgpuSetupCreateNew {
        device_descriptor: std::sync::Arc::new(|adapter: &eframe::wgpu::Adapter| {
            let a = adapter.limits();
            let base = if adapter.get_info().backend == eframe::wgpu::Backend::Gl { eframe::wgpu::Limits::downlevel_webgl2_defaults() } else { eframe::wgpu::Limits::default() };
            eframe::wgpu::DeviceDescriptor {
                label: Some("homeup device"),
                required_limits: eframe::wgpu::Limits {
                    max_texture_dimension_2d: a.max_texture_dimension_2d.max(8192),
                    max_storage_buffer_binding_size: a.max_storage_buffer_binding_size,
                    max_buffer_size: a.max_buffer_size,
                    max_storage_buffers_per_shader_stage: a.max_storage_buffers_per_shader_stage,
                    max_compute_workgroup_storage_size: a.max_compute_workgroup_storage_size,
                    ..base
                },
                ..Default::default()
            }
        }),
        ..eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle()
    };
    let wgpu_options = eframe::egui_wgpu::WgpuConfiguration { wgpu_setup: eframe::egui_wgpu::WgpuSetup::CreateNew(setup), ..Default::default() };
    let options = eframe::NativeOptions { renderer: eframe::Renderer::Wgpu, viewport, wgpu_options, ..Default::default() };
    let file = std::env::args().nth(1).map(std::path::PathBuf::from);
    eframe::run_native(
        "HomeUp",
        options,
        Box::new(move |cc| {
            let mut app = app::HomeUpApp::new(cc);
            if let Some(f) = file {
                app.open_path(f);
            }
            if app.snapshot.is_some() {
                app.show_home = false;
            }
            Ok(Box::new(app))
        }),
    )
}
