#![warn(clippy::all, rust_2018_idioms)]

pub mod decklist;
pub mod image_processing;
pub mod layout;
pub mod pdf_export;
pub mod settings;
pub mod style;
pub mod svg_export;
pub mod thumbnail_manager;
pub mod types;
pub mod ui;

use crate::decklist::{DecklistEntry, DecklistManager, MatchedCard};
use crate::pdf_export::export_pages_to_pdf_with_progress;
use crate::settings::{AppSettings, SettingsManager};
use crate::svg_export::export_pages_to_single_svg_with_progress;
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc;
use thumbnail_manager::{ThumbnailManager, ThumbnailMessage};
use tokio::task::JoinHandle;
use types::{Card, LayoutParams};
use ui::decklist_panel::DecklistState;
use ui::preview_panel::PreviewState;
use ui::{card_list_panel, decklist_panel, parameters_panel, preview_panel};
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

#[derive(Debug)]
pub enum AIMatchingMessage {
    Started,
    Completed { matches: Vec<MatchedCard> },
    Failed { error: String },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ExportFormat {
    Svg,
    Pdf,
}

impl ExportFormat {
    fn label(self) -> &'static str {
        match self {
            ExportFormat::Svg => "SVG",
            ExportFormat::Pdf => "PDF",
        }
    }
}

#[derive(Debug)]
enum ExportMessage {
    Started {
        format: ExportFormat,
        total_pages: usize,
    },
    PageCompleted {
        page_number: usize,
    },
    Completed {
        format: ExportFormat,
        output_path: PathBuf,
        total_pages: usize,
    },
    Failed {
        format: ExportFormat,
        error: String,
    },
}

#[derive(Default)]
struct ExportState {
    is_exporting: bool,
    format: Option<ExportFormat>,
    total_pages: usize,
    pages_completed: usize,
    error_message: Option<String>,
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
    decklist_state: DecklistState,
    decklist_manager: DecklistManager,
    show_decklist_tab: bool,
    ai_matching_receiver: Option<mpsc::Receiver<AIMatchingMessage>>,
    ai_matching_task: Option<JoinHandle<()>>,
    export_state: ExportState,
    export_receiver: Option<mpsc::Receiver<ExportMessage>>,
    export_task: Option<JoinHandle<()>>,
    settings_manager: SettingsManager,
    previous_bleed_enabled: bool,
    previous_bleed_mm: f32,
}

impl Default for TcgLayoutApp {
    fn default() -> Self {
        // Create settings manager
        let settings_manager = SettingsManager::new().unwrap_or_else(|e| {
            log::error!("Failed to create settings manager: {e}");
            panic!("Cannot create settings manager: {e}");
        });

        // Load saved settings
        let saved_settings = settings_manager.load_settings();

        // Load API key from keyring and initialize decklist state
        let api_key = settings_manager.load_openai_api_key().unwrap_or_default();
        let decklist_state = DecklistState {
            api_key,
            ..Default::default()
        };

        Self {
            layout_params: saved_settings.layout_params.clone(),
            validation_errors: Vec::new(),
            show_success_message: false,
            success_message_timer: 0.0,
            page_size_option: saved_settings.page_size_option,
            card_size_option: saved_settings.card_size_option,
            selected_cards: Vec::new(),
            preview_state: PreviewState::default(),
            thumbnail_manager: ThumbnailManager::with_capacity(200), // Larger cache for better performance
            decklist_state,
            decklist_manager: DecklistManager::new(),
            show_decklist_tab: false,
            ai_matching_receiver: None,
            ai_matching_task: None,
            export_state: ExportState::default(),
            export_receiver: None,
            export_task: None,
            settings_manager,
            previous_bleed_enabled: saved_settings.layout_params.enable_bleed,
            previous_bleed_mm: saved_settings.layout_params.bleed_mm,
        }
    }
}

impl TcgLayoutApp {
    fn save_settings(&self) {
        let settings = AppSettings {
            layout_params: self.layout_params.clone(),
            page_size_option: self.page_size_option,
            card_size_option: self.card_size_option,
        };

        if let Err(e) = self.settings_manager.save_settings(&settings) {
            log::error!("Failed to save settings: {e}");
        }
    }

    fn save_api_key(&self) {
        if let Err(e) = self
            .settings_manager
            .save_openai_api_key(&self.decklist_state.api_key)
        {
            log::error!("Failed to save API key: {e}");
        }
    }

    fn import_images(&mut self) {
        let files = rfd::FileDialog::new()
            .add_filter("Images", &["jpg", "jpeg", "png", "tiff", "tif"])
            .set_title("Select Card Images")
            .pick_files();

        if let Some(paths) = files {
            for path in paths {
                let mut card = Card::new(path.clone());

                // Check if thumbnail is already cached
                if let Some(thumbnail) = self.thumbnail_manager.request_thumbnail(
                    path.clone(),
                    self.layout_params.bleed_mm,
                    self.layout_params.enable_bleed,
                    self.layout_params.card_size,
                ) {
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
                ThumbnailMessage::ThumbnailLoaded { path, result, .. } => {
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

    fn swap_cards(&mut self, index1: usize, index2: usize) {
        if index1 < self.selected_cards.len()
            && index2 < self.selected_cards.len()
            && index1 != index2
        {
            self.selected_cards.swap(index1, index2);
        }
    }

    fn move_card_up(&mut self, index: usize) {
        if index > 0 && index < self.selected_cards.len() {
            self.swap_cards(index, index - 1);
        }
    }

    fn move_card_down(&mut self, index: usize) {
        if index < self.selected_cards.len().saturating_sub(1) {
            self.swap_cards(index, index + 1);
        }
    }

    fn export_to_svg(&mut self) {
        if self.selected_cards.is_empty() || self.export_state.is_exporting {
            return;
        }

        let output_file = rfd::FileDialog::new()
            .set_title("Save SVG Layout")
            .set_file_name("card_layout.svg")
            .add_filter("SVG files", &["svg"])
            .set_directory(".")
            .save_file();

        if let Some(output_path) = output_file {
            self.start_export(ExportFormat::Svg, output_path);
        }
    }

    fn export_to_pdf(&mut self) {
        if self.selected_cards.is_empty() || self.export_state.is_exporting {
            return;
        }

        let output_file = rfd::FileDialog::new()
            .set_title("Save PDF Layout")
            .set_file_name("card_layout.pdf")
            .add_filter("PDF files", &["pdf"])
            .set_directory(".")
            .save_file();

        if let Some(output_path) = output_file {
            self.start_export(ExportFormat::Pdf, output_path);
        }
    }

    fn start_export(&mut self, format: ExportFormat, output_path: PathBuf) {
        let grid = layout::calculate_grid(&self.layout_params);
        let pages = layout::distribute_cards(&self.selected_cards, &grid, &self.layout_params);
        let total_pages = pages.len();
        let params = self.layout_params.clone();

        let (sender, receiver) = mpsc::channel();
        let progress_sender = sender.clone();

        let task = tokio::task::spawn_blocking(move || {
            let _ = sender.send(ExportMessage::Started {
                format,
                total_pages,
            });

            let result = match format {
                ExportFormat::Svg => export_pages_to_single_svg_with_progress(
                    &pages,
                    &params,
                    &output_path,
                    |completed, _total| {
                        let _ = progress_sender.send(ExportMessage::PageCompleted {
                            page_number: completed,
                        });
                    },
                ),
                ExportFormat::Pdf => export_pages_to_pdf_with_progress(
                    &pages,
                    &params,
                    &output_path,
                    |completed, _total| {
                        let _ = progress_sender.send(ExportMessage::PageCompleted {
                            page_number: completed,
                        });
                    },
                ),
            };

            match result {
                Ok(()) => {
                    let _ = sender.send(ExportMessage::Completed {
                        format,
                        output_path,
                        total_pages,
                    });
                }
                Err(e) => {
                    let _ = sender.send(ExportMessage::Failed {
                        format,
                        error: e.to_string(),
                    });
                }
            }
        });

        self.export_state = ExportState {
            is_exporting: true,
            format: Some(format),
            total_pages,
            pages_completed: 0,
            error_message: None,
        };
        self.export_receiver = Some(receiver);
        self.export_task = Some(task);
    }

    fn process_export_messages(&mut self) {
        let mut messages = Vec::new();

        if let Some(receiver) = &self.export_receiver {
            while let Ok(message) = receiver.try_recv() {
                messages.push(message);
            }
        }

        for message in messages {
            match message {
                ExportMessage::Started {
                    format,
                    total_pages,
                } => {
                    self.export_state.is_exporting = true;
                    self.export_state.format = Some(format);
                    self.export_state.total_pages = total_pages;
                    self.export_state.pages_completed = 0;
                }
                ExportMessage::PageCompleted { page_number } => {
                    self.export_state.pages_completed = page_number;
                }
                ExportMessage::Completed {
                    format,
                    output_path,
                    total_pages,
                } => {
                    println!(
                        "Successfully exported {total_pages} pages to {}: {output_path:?}",
                        format.label(),
                    );
                    self.export_state = ExportState::default();
                    self.export_receiver = None;
                    self.export_task = None;
                    self.show_success_message = true;
                    self.success_message_timer = 3.0;
                }
                ExportMessage::Failed { format, error } => {
                    eprintln!("Failed to export {} file: {error}", format.label());
                    self.export_state.is_exporting = false;
                    self.export_state.error_message = Some(error);
                    self.export_receiver = None;
                    self.export_task = None;
                }
            }
        }
    }

    fn apply_decklist(&mut self, matched_cards: &[MatchedCard]) {
        match self
            .decklist_manager
            .apply_decklist_to_cards(matched_cards, &mut self.selected_cards)
        {
            Ok(()) => {
                // Reset to first page when cards are reordered
                self.preview_state.reset_to_first_page();
                // Update decklist state
                self.decklist_state.success_message =
                    Some("Decklist applied successfully!".to_string());
                self.decklist_state.error_message = None;
            }
            Err(e) => {
                self.decklist_state.error_message = Some(format!("Failed to apply decklist: {e}"));
                self.decklist_state.success_message = None;
            }
        }
    }

    fn start_ai_matching(&mut self, api_key: String, entries: Vec<DecklistEntry>) {
        // Cancel any existing task
        if let Some(task) = self.ai_matching_task.take() {
            task.abort();
        }

        let (sender, receiver) = mpsc::channel();
        self.ai_matching_receiver = Some(receiver);

        let mut manager = DecklistManager::new();
        manager.set_api_key(api_key);
        let cards = self.selected_cards.clone();

        // Spawn async task for AI matching
        let task = tokio::spawn(async move {
            let _ = sender.send(AIMatchingMessage::Started);

            match manager.match_cards_to_files(&entries, &cards).await {
                Ok(matches) => {
                    let _ = sender.send(AIMatchingMessage::Completed { matches });
                }
                Err(e) => {
                    let _ = sender.send(AIMatchingMessage::Failed {
                        error: format!("AI matching failed: {e}"),
                    });
                }
            }
        });

        self.ai_matching_task = Some(task);
        self.decklist_state.is_processing = true;
    }

    fn process_ai_matching_messages(&mut self) {
        let mut messages_to_process = Vec::new();

        // Collect messages without holding a borrow on the receiver
        if let Some(receiver) = &self.ai_matching_receiver {
            while let Ok(message) = receiver.try_recv() {
                messages_to_process.push(message);
            }
        }

        // Process messages
        for message in messages_to_process {
            match message {
                AIMatchingMessage::Started => {
                    self.decklist_state.is_processing = true;
                    self.decklist_state.success_message =
                        Some("Starting AI matching...".to_string());
                    self.decklist_state.error_message = None;
                }
                AIMatchingMessage::Completed { matches } => {
                    self.decklist_state.is_processing = false;
                    self.decklist_state.matched_cards = matches;
                    self.decklist_state.success_message = Some(format!(
                        "AI matching completed! Found {} matches.",
                        self.decklist_state.matched_cards.len()
                    ));
                    self.decklist_state.show_results = true;
                    self.ai_matching_receiver = None;
                    self.ai_matching_task = None;
                }
                AIMatchingMessage::Failed { error } => {
                    self.decklist_state.is_processing = false;
                    self.decklist_state.error_message = Some(error);
                    self.decklist_state.success_message = None;
                    self.ai_matching_receiver = None;
                    self.ai_matching_task = None;
                }
            }
        }
    }
}

impl eframe::App for TcgLayoutApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process thumbnail messages from background loader
        self.process_thumbnail_messages();

        // Process AI matching messages
        self.process_ai_matching_messages();

        // Process export messages
        self.process_export_messages();

        // Request repaint if we have pending thumbnails or AI matching to keep UI updating
        if self.thumbnail_manager.has_pending_requests()
            || self.decklist_state.is_processing
            || self.export_state.is_exporting
        {
            ctx.request_repaint();
        }

        ctx.set_style(style::style());

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
                    let export_enabled = !self.export_state.is_exporting;
                    if ui
                        .add_enabled(export_enabled, egui::Button::new("Export to SVG..."))
                        .clicked()
                    {
                        self.export_to_svg();
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(export_enabled, egui::Button::new("Export to PDF..."))
                        .clicked()
                    {
                        self.export_to_pdf();
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

        // Left pane - Card list and Decklist (tabbed)
        let mut cards_to_remove = None;
        let mut should_import = false;
        let mut copy_count_changes = None;
        let mut reorder_action = None;
        let mut decklist_matches_to_apply = None;
        let mut start_ai_matching = None;
        let mut api_key_changed = false;

        egui::SidePanel::left("left_panel")
            .resizable(true)
            .default_width(300.0)
            .width_range(250.0..=800.0)
            .show(ctx, |ui| {
                // Tabs for Card List and Decklist
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.show_decklist_tab, false, "Card List");
                    ui.selectable_value(&mut self.show_decklist_tab, true, "Decklist");
                });

                ui.separator();
                ui.add_space(8.0);

                if !self.show_decklist_tab {
                    // Card List panel
                    card_list_panel::show_card_list_panel(
                        ui,
                        &self.selected_cards,
                        |index| cards_to_remove = Some(index),
                        || should_import = true,
                        |index, new_count| copy_count_changes = Some((index, new_count)),
                        |index, is_move_up| reorder_action = Some((index, is_move_up)),
                    );
                } else {
                    // Decklist panel
                    api_key_changed = decklist_panel::show_decklist_panel(
                        ui,
                        &mut self.decklist_state,
                        |matched_cards| decklist_matches_to_apply = Some(matched_cards.to_vec()),
                        |api_key, entries| {
                            // Set up AI matching request
                            start_ai_matching = Some((api_key.to_string(), entries.to_vec()));
                        },
                    );
                }
            });

        // Handle card removal
        if let Some(index) = cards_to_remove {
            self.remove_card(index);
        }

        // Handle copy count changes
        if let Some((index, new_count)) = copy_count_changes {
            self.update_card_copy_count(index, new_count);
        }

        // Handle reorder actions
        if let Some((index, is_move_up)) = reorder_action {
            if is_move_up {
                self.move_card_up(index);
            } else {
                self.move_card_down(index);
            }
        }

        // Handle import request
        if should_import {
            self.import_images();
        }

        // Handle decklist application
        if let Some(matched_cards) = decklist_matches_to_apply {
            self.apply_decklist(&matched_cards);
        }

        // Handle AI matching request
        if let Some((api_key, entries)) = start_ai_matching {
            self.start_ai_matching(api_key, entries);
        }

        // Save API key if it changed
        if api_key_changed {
            self.save_api_key();
        }

        // Right pane - Parameters form
        let settings_changed = egui::SidePanel::right("parameters_panel")
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
                )
            })
            .inner;

        // Save settings if they changed
        if settings_changed {
            self.save_settings();
        }

        // Check if bleed settings changed and re-request thumbnails if needed
        let bleed_changed = self.layout_params.enable_bleed != self.previous_bleed_enabled
            || (self.layout_params.bleed_mm - self.previous_bleed_mm).abs() > 0.01;

        if bleed_changed {
            // Clear texture cache to force regeneration of preview textures
            self.preview_state.clear_texture_cache();

            // Re-request all thumbnails with new bleed settings
            for card in &mut self.selected_cards {
                // Request new thumbnail with updated bleed settings
                if let Some(thumbnail) = self.thumbnail_manager.request_thumbnail(
                    card.path.clone(),
                    self.layout_params.bleed_mm,
                    self.layout_params.enable_bleed,
                    self.layout_params.card_size,
                ) {
                    // Cache hit - set thumbnail immediately
                    card.set_thumbnail_loaded(thumbnail);
                } else {
                    // Cache miss - set loading state and request async loading
                    card.set_thumbnail_loading();
                }
            }

            // Update tracked state
            self.previous_bleed_enabled = self.layout_params.enable_bleed;
            self.previous_bleed_mm = self.layout_params.bleed_mm;
        }

        // Center pane - Preview
        egui::CentralPanel::default().show(ctx, |ui| {
            preview_panel::show_preview_panel(
                ui,
                &self.layout_params,
                &self.selected_cards,
                &mut self.preview_state,
            );
        });

        // Export progress overlay
        if self.export_state.is_exporting {
            egui::Window::new("Exporting...")
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    let format_name = self
                        .export_state
                        .format
                        .map_or("Export", ExportFormat::label);
                    ui.heading(format!("Exporting {format_name}..."));
                    ui.add_space(8.0);
                    let progress = if self.export_state.total_pages > 0 {
                        self.export_state.pages_completed as f32
                            / self.export_state.total_pages as f32
                    } else {
                        0.0
                    };
                    ui.add(egui::ProgressBar::new(progress).text(format!(
                        "Page {} of {}",
                        self.export_state.pages_completed, self.export_state.total_pages
                    )));
                });
        }

        // Export error dialog
        if self.export_state.error_message.is_some() {
            let mut dismiss = false;
            egui::Window::new("Export Failed")
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    if let Some(error) = &self.export_state.error_message {
                        ui.label(error);
                    }
                    ui.add_space(8.0);
                    if ui.button("OK").clicked() {
                        dismiss = true;
                    }
                });
            if dismiss {
                self.export_state.error_message = None;
            }
        }

        // Show success snackbar
        if self.show_success_message {
            egui::Window::new("validation_success")
                .title_bar(false)
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_TOP, [0.0, 50.0])
                .show(ctx, |ui| {
                    egui::Frame::none()
                        .fill(ctx.style().visuals.faint_bg_color)
                        .rounding(egui::Rounding::same(8.0))
                        .inner_margin(egui::Margin::same(12.0))
                        .shadow(egui::Shadow::default())
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.colored_label(ctx.style().visuals.text_color(), "✓");
                                ui.add_space(8.0);
                                ui.colored_label(
                                    ctx.style().visuals.text_color(),
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
