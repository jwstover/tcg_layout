#![warn(clippy::all, rust_2018_idioms)]

pub mod types;
pub mod layout;
pub mod ui;

use eframe::egui;
use types::LayoutParams;
use ui::{PageSizeOption, CardSizeOption};
use ui::{parameters_panel, card_list_panel, preview_panel};

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
    layout_params: LayoutParams,
    validation_errors: Vec<String>,
    show_success_message: bool,
    success_message_timer: f32,
    page_size_option: PageSizeOption,
    card_size_option: CardSizeOption,
}

impl Default for TcgLayoutApp {
    fn default() -> Self {
        Self {
            layout_params: LayoutParams::default(),
            validation_errors: Vec::new(),
            show_success_message: false,
            success_message_timer: 0.0,
            page_size_option: PageSizeOption::A4,
            card_size_option: CardSizeOption::Poker,
        }
    }
}


impl eframe::App for TcgLayoutApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle success message timer
        if self.show_success_message {
            self.success_message_timer -= ctx.input(|i| i.unstable_dt);
            if self.success_message_timer <= 0.0 {
                self.show_success_message = false;
            }
        }

        // Left pane - Card list (future)
        egui::SidePanel::left("card_list_panel")
            .resizable(true)
            .default_width(200.0)
            .width_range(150.0..=400.0)
            .show(ctx, |ui| {
                card_list_panel::show_card_list_panel(ui);
            });

        // Right pane - Parameters form
        egui::SidePanel::right("parameters_panel")
            .resizable(true)
            .default_width(300.0)
            .width_range(250.0..=500.0)
            .show(ctx, |ui| {
                parameters_panel::show_parameters_panel(
                    ui,
                    &mut self.layout_params,
                    &mut self.validation_errors,
                    &mut self.show_success_message,
                    &mut self.success_message_timer,
                    &mut self.page_size_option,
                    &mut self.card_size_option,
                );
            });

        // Center pane - Preview
        egui::CentralPanel::default().show(ctx, |ui| {
            preview_panel::show_preview_panel(ui);
        });

        // Show success snackbar
        if self.show_success_message {
            egui::Window::new("validation_success")
                .title_bar(false)
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_TOP, [0.0, 50.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.colored_label(egui::Color32::from_rgb(0, 150, 0), "✓");
                        ui.colored_label(egui::Color32::from_rgb(0, 120, 0), "Parameters are valid!");
                    });
                });
        }

        // Keep the UI updating for the timer
        if self.show_success_message {
            ctx.request_repaint();
        }
    }
}
