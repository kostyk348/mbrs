//! mbrs — Modbus Studio (Rust).
//!
//! A from-scratch Modbus master/monitor inspired by the feature surface of
//! Modbus Poll, but with a much more powerful value->colour engine
//! (N rules + 32-level / smooth ramps) and a SCADA-style tile view.
#![allow(dead_code)]

mod app;
mod colors;
mod formats;
mod modbus;
mod scada;
mod store;
mod workspace;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 600.0])
            .with_title("mbrs — Modbus Studio"),
        ..Default::default()
    };
    eframe::run_native(
        "mbrs — Modbus Studio",
        native_options,
        Box::new(|cc| Ok(Box::new(app::MbApp::new(cc)))),
    )
}
