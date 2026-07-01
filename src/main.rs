mod app;
mod archive;
mod export;
mod model;

use eframe::egui;

rust_i18n::i18n!("src/locales", fallback = "en");

const INITIAL_WINDOW_SIZE: [f32; 2] = [1280.0, 820.0];

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("GDS3D")
            .with_inner_size(INITIAL_WINDOW_SIZE),
        renderer: eframe::Renderer::Wgpu,
        depth_buffer: 24,
        multisampling: gds3d_viewport::RECOMMENDED_MSAA_SAMPLES,
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        "GDS3D",
        options,
        Box::new(|cc| Ok(Box::new(app::Gds3dApp::new(cc)))),
    )
}
