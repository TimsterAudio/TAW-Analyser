//! Timster's Audio Works Analyser (SP-001): a spectrum analyser on its own.
//! It only listens, to what an output plays or to an input, and never plays
//! anything back.

// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod capture;
mod icon;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TAW Analyser")
            .with_app_id("taw-analyser")
            .with_inner_size(app::DEFAULT_SIZE)
            .with_min_inner_size(egui::Vec2::new(320.0, 200.0))
            .with_icon(icon::icon()),
        ..Default::default()
    };
    eframe::run_native(
        "TAW Analyser",
        options,
        Box::new(|cc| Ok(Box::new(app::AnalyserApp::new(cc)))),
    )
}
