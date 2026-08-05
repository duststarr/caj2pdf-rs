//! `caj2pdf-gui` desktop application entry point.

use eframe::egui;

use caj2pdf_gui::App;

fn main() -> eframe::Result {
    // Tracing — same env-filter default as the CLI.
    use tracing_subscriber::EnvFilter;
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .try_init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([760.0, 540.0])
            .with_min_inner_size([560.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native(
        "caj2pdf",
        options,
        Box::new(|cc| {
            // Install the CJK font (if found) before the first frame.
            cc.egui_ctx.set_fonts(caj2pdf_gui::cjk_font_definitions());
            Ok(Box::new(App::default()))
        }),
    )
}

