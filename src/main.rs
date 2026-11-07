#![cfg_attr(windows, windows_subsystem = "windows")] // no console window on Windows

mod app;
mod classchange;
mod dft;
mod gamedata;
mod paths;
mod save;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Underkeeper")
            .with_app_id("underkeeper")
            .with_inner_size([1180.0, 780.0])
            .with_min_inner_size([900.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Underkeeper",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
