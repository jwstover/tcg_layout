#![warn(clippy::all, rust_2018_idioms)]

pub mod image_processing;
pub mod layout;
pub mod svg_export;
pub mod thumbnail_manager;
pub mod types;
pub mod ui;

use crate::svg_export::export_pages_to_single_svg;
use eframe::egui;
use thumbnail_manager::{ThumbnailManager, ThumbnailMessage};
use types::{Card, LayoutParams};
use ui::preview_panel::PreviewState;
use ui::{card_list_panel, parameters_panel, preview_panel};
use ui::{CardSizeOption, PageSizeOption};

#[tokio::main]
async fn main() -> Result<(), eframe::Error> {
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
    selected_cards: Vec<Card>,
    preview_state: PreviewState,
    thumbnail_manager: ThumbnailManager,
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
            selected_cards: Vec::new(),
            preview_state: PreviewState::default(),
            thumbnail_manager: ThumbnailManager::with_capacity(200), // Larger cache for better performance
        }
    }
}

impl TcgLayoutApp {
    fn import_images(&mut self) {
        let files = rfd::FileDialog::new()
            .add_filter("Images", &["jpg", "jpeg", "png", "tiff", "tif"])
            .set_title("Select Card Images")
            .pick_files();

        if let Some(paths) = files {
            for path in paths {
                let mut card = Card::new(path.clone());

                // Check if thumbnail is already cached
                if let Some(thumbnail) = self.thumbnail_manager.request_thumbnail(path.clone()) {
                    // Cache hit - set thumbnail immediately
                    card.set_thumbnail_loaded(thumbnail);
                } else {
                    // Cache miss - set loading state and request async loading
                    card.set_thumbnail_loading();
                }

                self.selected_cards.push(card);
            }
            // Reset to first page when new cards are added
            self.preview_state.reset_to_first_page();
        }
    }

    fn process_thumbnail_messages(&mut self) {
        while let Some(message) = self.thumbnail_manager.try_recv_message() {
            match message {
                ThumbnailMessage::ThumbnailLoaded { path, result } => {
                    // Find the card with this path and update its thumbnail
                    if let Some(card) = self.selected_cards.iter_mut().find(|c| c.path == path) {
                        match result {
                            thumbnail_manager::ThumbnailResult::Success(thumbnail) => {
                                card.set_thumbnail_loaded(thumbnail);
                            }
                            thumbnail_manager::ThumbnailResult::Error(error) => {
                                card.set_thumbnail_failed(error);
                            }
                        }
                    }
                }
            }
        }
    }

    fn remove_card(&mut self, index: usize) {
        if index < self.selected_cards.len() {
            self.selected_cards.remove(index);
            // Reset to first page when cards are removed
            self.preview_state.reset_to_first_page();
        }
    }

    fn update_card_copy_count(&mut self, index: usize, new_count: u32) {
        if index < self.selected_cards.len() {
            self.selected_cards[index].set_copy_count(new_count);
            // Reset to first page when copy counts change to refresh layout
            self.preview_state.reset_to_first_page();
        }
    }

    fn export_to_svg(&mut self) {
        if self.selected_cards.is_empty() {
            return;
        }

        // Show file save dialog
        let output_file = rfd::FileDialog::new()
            .set_title("Save SVG Layout")
            .set_file_name("card_layout.svg")
            .add_filter("SVG files", &["svg"])
            .set_directory(".")
            .save_file();

        if let Some(output_path) = output_file {
            // Calculate layout and distribute cards across pages
            let grid = layout::calculate_grid(&self.layout_params);
            let pages = layout::distribute_cards(&self.selected_cards, &grid, &self.layout_params);

            // Export all pages to single SVG file
            match export_pages_to_single_svg(&pages, &self.layout_params, &output_path) {
                Ok(()) => {
                    println!(
                        "Successfully exported {} pages to SVG: {:?}",
                        pages.len(),
                        output_path
                    );
                    // Show success message
                    self.show_success_message = true;
                    self.success_message_timer = 3.0;
                }
                Err(e) => {
                    eprintln!("Failed to export SVG file: {}", e);
                    // Could show error dialog here
                }
            }
        }
    }
}

impl eframe::App for TcgLayoutApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process thumbnail messages from background loader
        self.process_thumbnail_messages();

        // Request repaint if we have pending thumbnails to keep UI updating
        if self.thumbnail_manager.has_pending_requests() {
            ctx.request_repaint();
        }

        // Menu bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Import Images...").clicked() {
                        self.import_images();
                        ui.close_menu();
                    }
                    if ui.button("Clear All").clicked() {
                        self.selected_cards.clear();
                        self.preview_state.reset_to_first_page();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Export to SVG...").clicked() {
                        self.export_to_svg();
                        ui.close_menu();
                    }
                });
            });
        });

        // Handle success message timer
        if self.show_success_message {
            self.success_message_timer -= ctx.input(|i| i.unstable_dt);
            if self.success_message_timer <= 0.0 {
                self.show_success_message = false;
            }
        }

        // Left pane - Card list
        let mut cards_to_remove = None;
        let mut should_import = false;
        let mut copy_count_changes = None;
        egui::SidePanel::left("card_list_panel")
            .resizable(true)
            .default_width(200.0)
            .width_range(150.0..=400.0)
            .show(ctx, |ui| {
                card_list_panel::show_card_list_panel(
                    ui,
                    &self.selected_cards,
                    |index| cards_to_remove = Some(index),
                    || should_import = true,
                    |index, new_count| copy_count_changes = Some((index, new_count)),
                );
            });

        // Handle card removal
        if let Some(index) = cards_to_remove {
            self.remove_card(index);
        }

        // Handle copy count changes
        if let Some((index, new_count)) = copy_count_changes {
            self.update_card_copy_count(index, new_count);
        }

        // Handle import request
        if should_import {
            self.import_images();
        }

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
            preview_panel::show_preview_panel(
                ui,
                &self.layout_params,
                &self.selected_cards,
                &mut self.preview_state,
            );
        });

        // Show success snackbar
        if self.show_success_message {
            egui::Window::new("validation_success")
                .title_bar(false)
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_TOP, [0.0, 50.0])
                .show(ctx, |ui| {
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(220, 255, 220))
                        .rounding(egui::Rounding::same(8.0))
                        .inner_margin(egui::Margin::same(12.0))
                        .shadow(egui::Shadow::default())
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.colored_label(egui::Color32::from_rgb(0, 150, 0), "✓");
                                ui.add_space(8.0);
                                ui.colored_label(
                                    egui::Color32::from_rgb(0, 120, 0),
                                    "Parameters are valid!",
                                );
                            });
                        });
                });
        }

        // Keep the UI updating for the timer
        if self.show_success_message {
            ctx.request_repaint();
        }
    }
}
