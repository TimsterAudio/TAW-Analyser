//! Timster's Audio Works Analyser (SP-001): a spectrum analyser on its own.
//! It only listens, to what an output plays or to an input, and never plays
//! anything back.

// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod capture;
mod family;
mod icon;
mod instance;

fn main() -> eframe::Result {
    // One Analyser at a time: if one is open, bring it forward instead.
    let dir = instance::data_dir();
    if dir.as_deref().is_some_and(instance::hand_over) {
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TAW Analyser")
            .with_app_id("taw-analyser")
            .with_inner_size(app::DEFAULT_SIZE)
            .with_min_inner_size(egui::Vec2::new(480.0, 300.0))
            // The Analyser draws its own title strip and window buttons;
            // Settings can bring back the system's.
            .with_decorations(false)
            .with_icon(icon::icon()),
        ..Default::default()
    };
    eframe::run_native(
        "TAW Analyser",
        options,
        Box::new(move |cc| {
            if let Some(dir) = dir {
                instance::watch(dir, cc.egui_ctx.clone());
            }
            Ok(Box::new(app::AnalyserApp::new(cc)))
        }),
    )
}
