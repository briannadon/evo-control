//! Placeholder egui GUI shell — populated in Step 9.

use eframe::egui;

#[derive(Default)]
pub struct App;

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("evo-control");
            ui.label("Device controls will appear here in Step 9.");
            ui.separator();
            if ui.button("Quit").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }
}
