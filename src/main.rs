//! mbrs — Modbus Studio (Rust).
//!
//! A from-scratch Modbus master/monitor inspired by the feature surface of
//! Modbus Poll, but with a much more powerful value->colour engine
//! (N rules + 32-level / smooth ramps) and a SCADA-style tile view.
#![allow(dead_code)]

mod app;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 600.0])
            .with_title("mbrs — Modbus Studio"),
        ..Default::default()
    };
    let initial = std::env::args().nth(1);
    eframe::run_native(
        "mbrs — Modbus Studio",
        native_options,
        Box::new(move |cc| {
            let mut a = app::MbApp::new(cc);
            if let Some(p) = initial {
                a.load_workspace_file(&p);
            }
            Ok(Box::new(a))
        }),
    )
}
