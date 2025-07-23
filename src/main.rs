#![warn(clippy::all, rust_2018_idioms)]

pub mod types;
pub mod layout;

use eframe::egui;

fn main() -> Result<(), eframe::Error> {
    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "TCG Layout App",
        options,
        Box::new(|_cc| Ok(Box::<TcgLayoutApp>::default())),
    )
}

struct TcgLayoutApp {
    name: String,
}

impl Default for TcgLayoutApp {
    fn default() -> Self {
        Self {
            name: "World".to_owned(),
        }
    }
}

impl eframe::App for TcgLayoutApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("TCG Layout App");
            ui.horizontal(|ui| {
                let name_label = ui.label("Your name: ");
                ui.text_edit_singleline(&mut self.name)
                    .labelled_by(name_label.id);
            });
            ui.add(egui::Slider::new(&mut 0.0, 0.0..=120.0).text("age"));
            if ui.button("Click each year").clicked() {
                // placeholder
            }
            ui.label(format!("Hello '{}', this will be the TCG layout app!", self.name));
        });
    }
}
