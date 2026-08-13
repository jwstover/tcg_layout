#![warn(clippy::all, rust_2018_idioms)]

pub mod decklist;
pub mod google_drive;
pub mod image_processing;
pub mod layout;
pub mod marvelcdb;
pub mod pdf_export;
pub mod project;
pub mod settings;
pub mod style;
pub mod svg_export;
pub mod thumbnail_manager;
pub mod types;
pub mod ui;

use crate::decklist::{DecklistEntry, DecklistManager, MatchedCard};
use crate::google_drive::GoogleDriveMessage;
use crate::image_processing::ThumbnailParams;
use crate::marvelcdb::MarvelCdbMessage;
use crate::pdf_export::export_pages_to_pdf_with_progress;
use crate::project::{Project, ProjectCard, PROJECT_FILE_EXTENSION};
use crate::settings::{AppSettings, SettingsManager};
use crate::svg_export::export_pages_to_single_svg_with_progress;
use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc;
use thumbnail_manager::{ThumbnailManager, ThumbnailMessage};
use tokio::task::JoinHandle;
use types::{Card, LayoutParams};
use ui::color_adjust_preview::{ColorAdjustPreviewAction, ColorAdjustPreviewState};
use ui::decklist_panel::DecklistState;
use ui::preview_panel::PreviewState;
use ui::sharpen_preview::{SharpenPreviewAction, SharpenPreviewState};
use ui::{
    card_list_panel, color_adjust_preview, decklist_panel, parameters_panel, preview_panel,
    sharpen_preview,
};
use ui::{CardSizeOption, PageSizeOption};

#[tokio::main]
async fn main() -> Result<(), eframe::Error> {
    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
        // When launched as a bare binary (e.g. `cargo run`) rather than a `.app`
        // bundle, macOS defaults to an accessory activation policy: the app is
        // hidden from CMD+Tab and can't reliably become the frontmost window,
        // which drops keyboard focus. Force the Regular policy so it behaves
        // like a normal foreground app.
        event_loop_builder: Some(Box::new(|_builder| {
            #[cfg(target_os = "macos")]
            {
                use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
                _builder.with_activation_policy(ActivationPolicy::Regular);
            }
        })),
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

/// Results of native file/folder dialogs, run off the UI thread (see
/// `spawn_file_dialog`). Nothing is sent for a cancelled dialog.
enum DialogMessage {
    Images(Vec<PathBuf>),
    CardBack { index: usize, path: PathBuf },
    DefaultBack(PathBuf),
    ExportPath { format: ExportFormat, path: PathBuf },
    MarvelDir(PathBuf),
    SaveProjectPath(PathBuf),
    OpenProjectPath(PathBuf),
}

struct TcgLayoutApp {
    layout_params: LayoutParams,
    validation_errors: Vec<String>,
    show_success_message: bool,
    success_message_timer: f32,
    page_size_option: PageSizeOption,
    card_size_option: CardSizeOption,
    selected_cards: Vec<Card>,
    back_cards: HashMap<PathBuf, Card>,
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
    marvelcdb_receiver: Option<mpsc::Receiver<MarvelCdbMessage>>,
    marvelcdb_task: Option<JoinHandle<()>>,
    drive_index_receiver: Option<mpsc::Receiver<GoogleDriveMessage>>,
    drive_index_task: Option<JoinHandle<()>>,
    settings_manager: SettingsManager,
    current_project_path: Option<PathBuf>,
    recent_projects: Vec<PathBuf>,
    previous_bleed_enabled: bool,
    previous_bleed_mm: f32,
    previous_sharpen_enabled: bool,
    previous_sharpen: tcg_layout::sharpen::SharpenParams,
    sharpen_preview_state: SharpenPreviewState,
    color_adjust_preview_state: ColorAdjustPreviewState,
    dialog_sender: mpsc::Sender<DialogMessage>,
    dialog_receiver: mpsc::Receiver<DialogMessage>,
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

        // Load API keys from keyring and initialize decklist state
        let api_key = settings_manager.load_openai_api_key().unwrap_or_default();
        let google_drive_api_key = settings_manager
            .load_google_drive_api_key()
            .unwrap_or_default();
        let marvel_dir = saved_settings
            .marvel_champions_dir
            .clone()
            .unwrap_or_else(settings::default_marvel_champions_dir);
        let google_drive_folder_url = saved_settings
            .google_drive_folder_id
            .clone()
            .unwrap_or_default();
        let drive_index_updated_at = google_drive::load_drive_index_timestamp();
        let decklist_state = DecklistState {
            api_key,
            marvel_champions_dir: marvel_dir,
            google_drive_api_key,
            google_drive_folder_url,
            drive_index_updated_at,
            ..Default::default()
        };

        let (dialog_sender, dialog_receiver) = mpsc::channel();

        Self {
            layout_params: saved_settings.layout_params.clone(),
            validation_errors: Vec::new(),
            show_success_message: false,
            success_message_timer: 0.0,
            page_size_option: saved_settings.page_size_option,
            card_size_option: saved_settings.card_size_option,
            selected_cards: Vec::new(),
            back_cards: HashMap::new(),
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
            marvelcdb_receiver: None,
            marvelcdb_task: None,
            drive_index_receiver: None,
            drive_index_task: None,
            settings_manager,
            current_project_path: None,
            recent_projects: saved_settings.recent_projects.clone(),
            previous_bleed_enabled: saved_settings.layout_params.enable_bleed,
            previous_bleed_mm: saved_settings.layout_params.bleed_mm,
            previous_sharpen_enabled: saved_settings.layout_params.enable_sharpen,
            previous_sharpen: saved_settings.layout_params.sharpen_params(),
            sharpen_preview_state: SharpenPreviewState::default(),
            color_adjust_preview_state: ColorAdjustPreviewState::default(),
            dialog_sender,
            dialog_receiver,
        }
    }
}

impl TcgLayoutApp {
    fn save_settings(&self) {
        let google_drive_folder_id = if self
            .decklist_state
            .google_drive_folder_url
            .trim()
            .is_empty()
        {
            None
        } else {
            Some(self.decklist_state.google_drive_folder_url.clone())
        };

        let settings = AppSettings {
            layout_params: self.layout_params.clone(),
            page_size_option: self.page_size_option,
            card_size_option: self.card_size_option,
            marvel_champions_dir: Some(self.decklist_state.marvel_champions_dir.clone()),
            google_drive_folder_id,
            recent_projects: self.recent_projects.clone(),
        };

        if let Err(e) = self.settings_manager.save_settings(&settings) {
            log::error!("Failed to save settings: {e}");
        }
    }

    fn save_api_key(&self) {
        self.settings_manager
            .save_openai_api_key(&self.decklist_state.api_key);
    }

    fn save_google_drive_api_key(&self) {
        self.settings_manager
            .save_google_drive_api_key(&self.decklist_state.google_drive_api_key);
    }

    fn import_images(&mut self) {
        let sender = self.dialog_sender.clone();
        tokio::spawn(async move {
            let files = rfd::AsyncFileDialog::new()
                .add_filter("Images", &["jpg", "jpeg", "png", "tiff", "tif"])
                .set_title("Select Card Images")
                .pick_files()
                .await;
            if let Some(files) = files {
                let paths = files.into_iter().map(PathBuf::from).collect();
                let _ = sender.send(DialogMessage::Images(paths));
            }
        });
    }

    fn pick_marvel_dir(&self) {
        let sender = self.dialog_sender.clone();
        let starting_dir = self.decklist_state.marvel_champions_dir.clone();
        tokio::spawn(async move {
            let dir = rfd::AsyncFileDialog::new()
                .set_title("Select Marvel Champions Images Directory")
                .set_directory(&starting_dir)
                .pick_folder()
                .await;
            if let Some(dir) = dir {
                let _ = sender.send(DialogMessage::MarvelDir(PathBuf::from(dir)));
            }
        });
    }

    fn spawn_open_project_dialog(&self) {
        let sender = self.dialog_sender.clone();
        tokio::spawn(async move {
            let file = rfd::AsyncFileDialog::new()
                .add_filter("TCG Layout Project", &[PROJECT_FILE_EXTENSION])
                .set_title("Open Project")
                .pick_file()
                .await;
            if let Some(file) = file {
                let _ = sender.send(DialogMessage::OpenProjectPath(PathBuf::from(file)));
            }
        });
    }

    fn spawn_save_project_dialog(&self) {
        let sender = self.dialog_sender.clone();
        tokio::spawn(async move {
            let file = rfd::AsyncFileDialog::new()
                .add_filter("TCG Layout Project", &[PROJECT_FILE_EXTENSION])
                .set_file_name(format!("project.{PROJECT_FILE_EXTENSION}"))
                .set_title("Save Project As")
                .save_file()
                .await;
            if let Some(file) = file {
                let _ = sender.send(DialogMessage::SaveProjectPath(PathBuf::from(file)));
            }
        });
    }

    /// Saves to the current project path, or prompts for one if this project
    /// hasn't been saved before.
    fn save_project(&mut self) {
        match self.current_project_path.clone() {
            Some(path) => self.write_project_to_path(path),
            None => self.spawn_save_project_dialog(),
        }
    }

    fn build_project(&self) -> Project {
        Project {
            layout_params: self.layout_params.clone(),
            page_size_option: self.page_size_option,
            card_size_option: self.card_size_option,
            cards: self.selected_cards.iter().map(ProjectCard::from).collect(),
        }
    }

    fn write_project_to_path(&mut self, path: PathBuf) {
        let project = self.build_project();
        match project.save_to_file(&path) {
            Ok(()) => {
                self.current_project_path = Some(path.clone());
                settings::record_recent_project(&mut self.recent_projects, path);
                self.save_settings();
                self.show_success_message = true;
                self.success_message_timer = 3.0;
            }
            Err(e) => {
                log::error!("Failed to save project: {e}");
            }
        }
    }

    /// Clears the working state and starts a fresh, unsaved project.
    fn new_project(&mut self) {
        self.selected_cards.clear();
        self.back_cards.clear();
        self.preview_state.reset_to_first_page();
        self.preview_state.clear_texture_cache();
        self.layout_params = LayoutParams::default();
        self.page_size_option = PageSizeOption::A4;
        self.card_size_option = CardSizeOption::Poker;
        self.current_project_path = None;
    }

    /// Replaces the working state with a loaded project. Card thumbnails are
    /// requested the same way freshly-imported images are; a card whose file
    /// has moved or been deleted still loads (surfacing as a failed
    /// thumbnail rather than a load error), so the rest of the project isn't
    /// lost over one missing image.
    fn load_project_from_path(&mut self, path: PathBuf) {
        let project = match Project::load_from_file(&path) {
            Ok(project) => project,
            Err(e) => {
                log::error!("Failed to load project from {path:?}: {e}");
                return;
            }
        };

        self.back_cards.clear();
        self.preview_state.reset_to_first_page();
        self.preview_state.clear_texture_cache();

        self.layout_params = project.layout_params;
        self.page_size_option = project.page_size_option;
        self.card_size_option = project.card_size_option;

        let thumbnail_params = ThumbnailParams::from_layout(&self.layout_params);
        self.selected_cards = project
            .cards
            .iter()
            .map(|project_card| {
                let mut card = project_card.to_card();
                if let Some(thumbnail) = self
                    .thumbnail_manager
                    .request_thumbnail(card.path.clone(), &thumbnail_params)
                {
                    card.set_thumbnail_loaded(thumbnail);
                } else {
                    card.set_thumbnail_loading();
                }
                card
            })
            .collect();

        self.current_project_path = Some(path.clone());
        settings::record_recent_project(&mut self.recent_projects, path);
        self.save_settings();
    }

    fn process_thumbnail_messages(&mut self) {
        while let Some(message) = self.thumbnail_manager.try_recv_message() {
            match message {
                ThumbnailMessage::ThumbnailLoaded { path, result, .. } => {
                    // Find the card with this path (front or back) and update
                    // its thumbnail
                    let card = self
                        .selected_cards
                        .iter_mut()
                        .find(|c| c.path == path)
                        .or_else(|| self.back_cards.get_mut(&path));
                    if let Some(card) = card {
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

    /// Make sure every referenced back image (per-card or default) has a
    /// thumbnail-managed Card instance in `back_cards`. Covers paths restored
    /// from settings as well as freshly assigned ones.
    fn ensure_back_cards_registered(&mut self) {
        let mut missing: Vec<PathBuf> = Vec::new();

        if let Some(path) = &self.layout_params.default_back_path {
            if !self.back_cards.contains_key(path) {
                missing.push(path.clone());
            }
        }
        for card in &self.selected_cards {
            if let Some(path) = &card.back_path {
                if !self.back_cards.contains_key(path) {
                    missing.push(path.clone());
                }
            }
        }

        let thumbnail_params = ThumbnailParams::from_layout(&self.layout_params);
        for path in missing {
            let mut card = Card::new(path.clone());
            if let Some(thumbnail) = self
                .thumbnail_manager
                .request_thumbnail(path.clone(), &thumbnail_params)
            {
                card.set_thumbnail_loaded(thumbnail);
            } else {
                card.set_thumbnail_loading();
            }
            self.back_cards.insert(path, card);
        }
    }

    /// Images the color adjustment editor can page through: every unique
    /// card front, then every unique back (per-card backs and the default)
    fn color_adjust_preview_entries(&self) -> Vec<color_adjust_preview::PreviewEntry> {
        let mut entries = Vec::new();

        let mut seen_fronts = HashSet::new();
        for card in &self.selected_cards {
            if seen_fronts.insert(card.path.clone()) {
                entries.push(color_adjust_preview::PreviewEntry {
                    path: card.path.clone(),
                    is_back: false,
                });
            }
        }

        let mut seen_backs = HashSet::new();
        let back_paths = self
            .selected_cards
            .iter()
            .filter_map(|card| card.back_path.as_ref())
            .chain(self.layout_params.default_back_path.as_ref());
        for path in back_paths {
            if seen_backs.insert(path.clone()) {
                entries.push(color_adjust_preview::PreviewEntry {
                    path: path.clone(),
                    is_back: true,
                });
            }
        }

        entries
    }

    /// Spawns the native back-image picker off the UI thread and hands the
    /// chosen path to `on_picked` (run on `process_dialog_messages`) once the
    /// dialog resolves.
    fn spawn_back_image_dialog(
        &self,
        title: &'static str,
        on_picked: impl FnOnce(PathBuf) -> DialogMessage + Send + 'static,
    ) {
        let sender = self.dialog_sender.clone();
        tokio::spawn(async move {
            let file = rfd::AsyncFileDialog::new()
                .add_filter("Images", &["jpg", "jpeg", "png", "tiff", "tif"])
                .set_title(title)
                .pick_file()
                .await;
            if let Some(file) = file {
                let _ = sender.send(on_picked(PathBuf::from(file)));
            }
        });
    }

    fn set_card_back(&mut self, index: usize) {
        if index >= self.selected_cards.len() {
            return;
        }
        self.spawn_back_image_dialog("Select Card Back Image", move |path| {
            DialogMessage::CardBack { index, path }
        });
    }

    fn clear_card_back(&mut self, index: usize) {
        if let Some(card) = self.selected_cards.get_mut(index) {
            card.back_path = None;
        }
    }

    fn pick_default_back(&mut self) {
        self.spawn_back_image_dialog("Select Default Card Back Image", DialogMessage::DefaultBack);
    }

    fn remove_card(&mut self, index: usize) {
        if index < self.selected_cards.len() {
            self.selected_cards.remove(index);
            // Keep the current preview page; the preview panel clamps it if the
            // page count shrinks below the current page.
        }
    }

    fn update_card_copy_count(&mut self, index: usize, new_count: u32) {
        if index < self.selected_cards.len() {
            self.selected_cards[index].set_copy_count(new_count);
            // Keep the current preview page; the preview panel clamps it if the
            // page count shrinks below the current page.
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
        self.spawn_export_path_dialog(
            ExportFormat::Svg,
            "Save SVG Layout",
            "card_layout.svg",
            "SVG files",
            &["svg"],
        );
    }

    fn export_to_pdf(&mut self) {
        if self.selected_cards.is_empty() || self.export_state.is_exporting {
            return;
        }
        self.spawn_export_path_dialog(
            ExportFormat::Pdf,
            "Save PDF Layout",
            "card_layout.pdf",
            "PDF files",
            &["pdf"],
        );
    }

    /// Spawns the native save-file picker off the UI thread; the chosen path
    /// arrives via `DialogMessage::ExportPath` on `process_dialog_messages`.
    fn spawn_export_path_dialog(
        &self,
        format: ExportFormat,
        title: &'static str,
        default_file_name: &'static str,
        filter_name: &'static str,
        filter_extensions: &'static [&'static str],
    ) {
        let sender = self.dialog_sender.clone();
        tokio::spawn(async move {
            let output_file = rfd::AsyncFileDialog::new()
                .set_title(title)
                .set_file_name(default_file_name)
                .add_filter(filter_name, filter_extensions)
                .set_directory(".")
                .save_file()
                .await;
            if let Some(output_file) = output_file {
                let _ = sender.send(DialogMessage::ExportPath {
                    format,
                    path: PathBuf::from(output_file),
                });
            }
        });
    }

    fn start_export(&mut self, format: ExportFormat, output_path: PathBuf) {
        let grid = layout::calculate_grid(&self.layout_params);
        let pages = layout::distribute_cards_with_backs(
            &self.selected_cards,
            &grid,
            &self.layout_params,
            &self.back_cards,
        );
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

    fn process_dialog_messages(&mut self) {
        while let Ok(message) = self.dialog_receiver.try_recv() {
            match message {
                DialogMessage::Images(paths) => {
                    let thumbnail_params = ThumbnailParams::from_layout(&self.layout_params);
                    for path in paths {
                        let mut card = Card::new(path.clone());

                        // Check if thumbnail is already cached
                        if let Some(thumbnail) = self
                            .thumbnail_manager
                            .request_thumbnail(path.clone(), &thumbnail_params)
                        {
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
                DialogMessage::CardBack { index, path } => {
                    if let Some(card) = self.selected_cards.get_mut(index) {
                        card.back_path = Some(path);
                    }
                }
                DialogMessage::DefaultBack(path) => {
                    self.layout_params.default_back_path = Some(path);
                    self.save_settings();
                }
                DialogMessage::ExportPath { format, path } => {
                    self.start_export(format, path);
                }
                DialogMessage::MarvelDir(path) => {
                    self.decklist_state.marvel_champions_dir = path;
                    self.save_settings();
                }
                DialogMessage::SaveProjectPath(path) => {
                    self.write_project_to_path(path);
                }
                DialogMessage::OpenProjectPath(path) => {
                    self.load_project_from_path(path);
                }
            }
        }
    }

    fn import_matched_cards(&mut self, matched_cards: &[MatchedCard]) {
        // Clear existing cards and import fresh from matched paths
        self.selected_cards.clear();
        self.preview_state.clear_texture_cache();

        let thumbnail_params = ThumbnailParams::from_layout(&self.layout_params);
        for matched in matched_cards {
            let mut card = Card::new(matched.matched_path.clone());
            card.set_copy_count(matched.count);

            // Request thumbnail
            if let Some(thumbnail) = self
                .thumbnail_manager
                .request_thumbnail(matched.matched_path.clone(), &thumbnail_params)
            {
                card.set_thumbnail_loaded(thumbnail);
            } else {
                card.set_thumbnail_loading();
            }

            self.selected_cards.push(card);
        }

        self.preview_state.reset_to_first_page();
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

    fn start_marvelcdb_fetch(&mut self, url: String, images_dir: PathBuf) {
        // Cancel any existing task
        if let Some(task) = self.marvelcdb_task.take() {
            task.abort();
        }

        let (sender, receiver) = mpsc::channel();
        self.marvelcdb_receiver = Some(receiver);
        self.decklist_state.is_fetching_marvelcdb = true;

        let google_drive_api_key = self.decklist_state.google_drive_api_key.clone();
        let google_drive_folder_url = self.decklist_state.google_drive_folder_url.clone();

        let task = tokio::spawn(async move {
            let _ = sender.send(MarvelCdbMessage::Started);

            // Step 1: Fetch deck from MarvelCDB API
            let fetched_deck = match marvelcdb::fetch_deck(&url).await {
                Ok(deck) => deck,
                Err(e) => {
                    let _ = sender.send(MarvelCdbMessage::Failed(e.to_string()));
                    return;
                }
            };

            let _ = sender.send(MarvelCdbMessage::DeckFetched {
                deck_name: fetched_deck.deck_name.clone(),
                hero_name: fetched_deck.hero_name.clone(),
            });

            // Step 2: If Drive configured, match + download via Drive index
            let drive_configured = !google_drive_api_key.trim().is_empty()
                && !google_drive_folder_url.trim().is_empty();

            if drive_configured {
                let folder_id = match google_drive::parse_drive_folder_url(&google_drive_folder_url)
                {
                    Ok(id) => id,
                    Err(e) => {
                        log::warn!("Invalid Drive folder URL: {e}");
                        // Treat as drive not configured — all cards unmatched
                        let result = marvelcdb::MarvelCdbResult {
                            deck_name: fetched_deck.deck_name.clone(),
                            hero_name: fetched_deck.hero_name.clone(),
                            matched_cards: Vec::new(),
                            unmatched_cards: fetched_deck
                                .cards
                                .iter()
                                .map(|c| marvelcdb::UnmatchedCard {
                                    name: c.name.clone(),
                                    count: c.count,
                                    faction: c.faction_code.clone(),
                                    pack_code: c.pack_code.clone(),
                                })
                                .collect(),
                        };
                        let _ = sender.send(MarvelCdbMessage::Completed(result));
                        return;
                    }
                };

                let client = google_drive::GoogleDriveClient::new(google_drive_api_key);
                match client
                    .match_and_download_cards(&fetched_deck, &images_dir, &folder_id, &sender)
                    .await
                {
                    Ok(result) => {
                        let _ = sender.send(MarvelCdbMessage::Completed(result));
                    }
                    Err(e) => {
                        let _ = sender.send(MarvelCdbMessage::Failed(e.to_string()));
                    }
                }
            } else {
                // Drive not configured — all cards become unmatched
                let result = marvelcdb::MarvelCdbResult {
                    deck_name: fetched_deck.deck_name.clone(),
                    hero_name: fetched_deck.hero_name.clone(),
                    matched_cards: Vec::new(),
                    unmatched_cards: fetched_deck
                        .cards
                        .iter()
                        .map(|c| marvelcdb::UnmatchedCard {
                            name: c.name.clone(),
                            count: c.count,
                            faction: c.faction_code.clone(),
                            pack_code: c.pack_code.clone(),
                        })
                        .collect(),
                };
                let _ = sender.send(MarvelCdbMessage::Completed(result));
            }
        });

        self.marvelcdb_task = Some(task);
    }

    fn process_marvelcdb_messages(&mut self) {
        let mut messages = Vec::new();

        if let Some(receiver) = &self.marvelcdb_receiver {
            while let Ok(message) = receiver.try_recv() {
                messages.push(message);
            }
        }

        for message in messages {
            match message {
                MarvelCdbMessage::Started => {
                    self.decklist_state.is_fetching_marvelcdb = true;
                    self.decklist_state.marvelcdb_error = None;
                    self.decklist_state.marvelcdb_progress =
                        Some("Fetching deck from MarvelCDB...".to_string());
                }
                MarvelCdbMessage::Progress(msg) => {
                    self.decklist_state.marvelcdb_progress = Some(msg);
                }
                MarvelCdbMessage::DeckFetched {
                    deck_name,
                    hero_name,
                } => {
                    self.decklist_state.marvelcdb_deck_name = Some(deck_name);
                    self.decklist_state.marvelcdb_hero_name = Some(hero_name);
                }
                MarvelCdbMessage::Completed(result) => {
                    self.decklist_state.is_fetching_marvelcdb = false;
                    self.decklist_state.marvelcdb_progress = None;
                    self.decklist_state.marvelcdb_deck_name = Some(result.deck_name);
                    self.decklist_state.marvelcdb_hero_name = Some(result.hero_name);
                    self.decklist_state.marvelcdb_matched = result.matched_cards.clone();
                    self.decklist_state.marvelcdb_unmatched = result.unmatched_cards;
                    self.decklist_state.marvelcdb_success =
                        Some(format!("Found {} matches", result.matched_cards.len()));
                    self.marvelcdb_receiver = None;
                    self.marvelcdb_task = None;
                    // Update index timestamp since match_and_download saves the index
                    self.decklist_state.drive_index_updated_at =
                        google_drive::load_drive_index_timestamp();
                }
                MarvelCdbMessage::Failed(error) => {
                    self.decklist_state.is_fetching_marvelcdb = false;
                    self.decklist_state.marvelcdb_progress = None;
                    self.decklist_state.marvelcdb_error = Some(error);
                    self.decklist_state.marvelcdb_success = None;
                    self.marvelcdb_receiver = None;
                    self.marvelcdb_task = None;
                }
            }
        }
    }

    fn start_build_drive_index(&mut self) {
        // Cancel any existing task
        if let Some(task) = self.drive_index_task.take() {
            task.abort();
        }

        let api_key = self.decklist_state.google_drive_api_key.clone();
        let folder_url = self.decklist_state.google_drive_folder_url.clone();

        let folder_id = match google_drive::parse_drive_folder_url(&folder_url) {
            Ok(id) => id,
            Err(e) => {
                self.decklist_state.drive_index_error = Some(format!("Invalid folder URL: {e}"));
                return;
            }
        };

        let (sender, receiver) = mpsc::channel();
        self.drive_index_receiver = Some(receiver);
        self.decklist_state.is_building_drive_index = true;
        self.decklist_state.drive_index_error = None;
        self.decklist_state.drive_index_success = None;

        let task = tokio::spawn(async move {
            let client = google_drive::GoogleDriveClient::new(api_key);
            match client.build_index(&folder_id, &sender).await {
                Ok(_) => {}
                Err(e) => {
                    let _ = sender.send(GoogleDriveMessage::Failed(e.to_string()));
                }
            }
        });

        self.drive_index_task = Some(task);
    }

    fn process_google_drive_messages(&mut self) {
        let mut messages = Vec::new();

        if let Some(receiver) = &self.drive_index_receiver {
            while let Ok(message) = receiver.try_recv() {
                messages.push(message);
            }
        }

        for message in messages {
            match message {
                GoogleDriveMessage::IndexBuildProgress {
                    folders_scanned,
                    total_folders,
                } => {
                    self.decklist_state.drive_index_progress =
                        Some((folders_scanned, total_folders));
                }
                GoogleDriveMessage::IndexBuildComplete {
                    file_count,
                    updated_at,
                } => {
                    self.decklist_state.is_building_drive_index = false;
                    self.decklist_state.drive_index_progress = None;
                    self.decklist_state.drive_index_updated_at = Some(updated_at);
                    self.decklist_state.drive_index_success =
                        Some(format!("Index built: {file_count} files indexed"));
                    self.drive_index_receiver = None;
                    self.drive_index_task = None;
                }
                GoogleDriveMessage::Failed(error) => {
                    self.decklist_state.is_building_drive_index = false;
                    self.decklist_state.drive_index_progress = None;
                    self.decklist_state.drive_index_error = Some(error);
                    self.drive_index_receiver = None;
                    self.drive_index_task = None;
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

        // Process MarvelCDB messages
        self.process_marvelcdb_messages();

        // Process Google Drive messages
        self.process_google_drive_messages();

        // Process export messages
        self.process_export_messages();

        // Process results from native file/folder dialogs
        self.process_dialog_messages();

        // Request repaint if we have pending thumbnails or AI matching to keep UI updating
        if self.thumbnail_manager.has_pending_requests()
            || self.decklist_state.is_processing
            || self.decklist_state.is_fetching_marvelcdb
            || self.decklist_state.is_building_drive_index
            || self.export_state.is_exporting
        {
            ctx.request_repaint();
        }

        ctx.set_style(style::style());

        let project_label = self
            .current_project_path
            .as_ref()
            .map(|p| project::display_name(p))
            .unwrap_or_else(|| "Untitled".to_string());
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "{project_label} - TCG Layout App"
        )));

        // Menu bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New Project").clicked() {
                        self.new_project();
                        ui.close_menu();
                    }
                    if ui.button("Open Project...").clicked() {
                        self.spawn_open_project_dialog();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Save Project").clicked() {
                        self.save_project();
                        ui.close_menu();
                    }
                    if ui.button("Save Project As...").clicked() {
                        self.spawn_save_project_dialog();
                        ui.close_menu();
                    }
                    ui.menu_button("Recent Projects", |ui| {
                        if self.recent_projects.is_empty() {
                            ui.label("No Recent Projects");
                        } else {
                            for path in self.recent_projects.clone() {
                                let label = project::display_name(&path);
                                if ui
                                    .button(label)
                                    .on_hover_text(path.to_string_lossy())
                                    .clicked()
                                {
                                    self.load_project_from_path(path);
                                    ui.close_menu();
                                }
                            }
                        }
                    });
                    ui.separator();
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
        let mut back_action = None;
        let mut decklist_matches_to_apply = None;
        let mut marvelcdb_import = None;
        let mut start_ai_matching = None;
        let mut start_marvelcdb_fetch = None;
        let mut api_key_changed = false;
        let mut marvel_dir_changed = false;
        let mut google_drive_api_key_changed = false;
        let mut google_drive_folder_url_changed = false;
        let mut start_build_drive_index_action = false;
        let mut browse_marvel_dir_clicked = false;

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
                        self.layout_params.enable_duplex,
                        |index| cards_to_remove = Some(index),
                        || should_import = true,
                        |index, new_count| copy_count_changes = Some((index, new_count)),
                        |index, is_move_up| reorder_action = Some((index, is_move_up)),
                        |index, is_set| back_action = Some((index, is_set)),
                    );
                } else {
                    // Decklist panel
                    let actions = decklist_panel::show_decklist_panel(
                        ui,
                        &mut self.decklist_state,
                        |matched_cards| decklist_matches_to_apply = Some(matched_cards.to_vec()),
                        |api_key, entries| {
                            // Set up AI matching request
                            start_ai_matching = Some((api_key.to_string(), entries.to_vec()));
                        },
                        |url, images_dir| {
                            start_marvelcdb_fetch = Some((url.to_string(), images_dir.clone()));
                        },
                        |matched_cards| marvelcdb_import = Some(matched_cards.to_vec()),
                    );
                    api_key_changed = actions.api_key_changed;
                    marvel_dir_changed = actions.marvel_dir_changed;
                    google_drive_api_key_changed = actions.google_drive_api_key_changed;
                    google_drive_folder_url_changed = actions.google_drive_folder_url_changed;
                    start_build_drive_index_action = actions.start_build_drive_index;
                    browse_marvel_dir_clicked = actions.browse_marvel_dir_clicked;
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

        // Handle Marvel Champions directory browse request
        if browse_marvel_dir_clicked {
            self.pick_marvel_dir();
        }

        // Handle import request
        if should_import {
            self.import_images();
        }

        // Handle per-card back image changes
        if let Some((index, is_set)) = back_action {
            if is_set {
                self.set_card_back(index);
            } else {
                self.clear_card_back(index);
            }
        }

        // Handle decklist application (AI matching — reorders existing cards)
        if let Some(matched_cards) = decklist_matches_to_apply {
            self.apply_decklist(&matched_cards);
        }

        // Handle MarvelCDB import (imports cards from file paths)
        if let Some(matched_cards) = marvelcdb_import {
            self.import_matched_cards(&matched_cards);
        }

        // Handle AI matching request
        if let Some((api_key, entries)) = start_ai_matching {
            self.start_ai_matching(api_key, entries);
        }

        // Handle MarvelCDB fetch request
        if let Some((url, images_dir)) = start_marvelcdb_fetch {
            self.start_marvelcdb_fetch(url, images_dir);
        }

        // Save API key if it changed
        if api_key_changed {
            self.save_api_key();
        }

        // Save settings if marvel dir changed
        if marvel_dir_changed {
            self.save_settings();
        }

        // Save Google Drive API key if it changed
        if google_drive_api_key_changed {
            self.save_google_drive_api_key();
        }

        // Save settings if Google Drive folder URL changed
        if google_drive_folder_url_changed {
            self.save_settings();
        }

        // Handle Google Drive index build request
        if start_build_drive_index_action {
            self.start_build_drive_index();
        }

        // Right pane - Parameters form
        let mut sharpen_preview_requested = false;
        let mut color_adjust_preview_requested = false;
        let mut default_back_pick_requested = false;
        let has_cards = !self.selected_cards.is_empty();
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
                    has_cards,
                    &mut sharpen_preview_requested,
                    &mut color_adjust_preview_requested,
                    &mut default_back_pick_requested,
                )
            })
            .inner;

        // Save settings if they changed
        if settings_changed {
            self.save_settings();
        }

        // Open the default back image picker
        if default_back_pick_requested {
            self.pick_default_back();
        }

        // Open the full-resolution sharpen preview for the first card
        if sharpen_preview_requested {
            if let Some(card) = self.selected_cards.first() {
                self.sharpen_preview_state
                    .open(card.path.clone(), self.layout_params.sharpen_params());
            }
        }

        // Show the sharpen preview window if open
        if self.sharpen_preview_state.is_open() {
            match sharpen_preview::show_sharpen_preview_window(ctx, &mut self.sharpen_preview_state)
            {
                SharpenPreviewAction::Apply(sharpen) => {
                    self.layout_params.sharpen_amount = sharpen.amount;
                    self.layout_params.sharpen_radius = sharpen.radius;
                    self.layout_params.sharpen_threshold = sharpen.threshold;
                    self.layout_params.enable_sharpen = true;
                    self.save_settings();
                    // Thumbnail refresh is handled by the change detection below
                }
                SharpenPreviewAction::None => {}
            }
        }

        // Open the color adjustment editor over all card fronts and backs
        if color_adjust_preview_requested {
            let entries = self.color_adjust_preview_entries();
            if !entries.is_empty() {
                self.color_adjust_preview_state
                    .open(entries, self.layout_params.hsl_adjustments.clone());
            }
        }

        // Show the color adjustment editor window if open. Color adjustments
        // intentionally don't affect thumbnails, so no cache invalidation.
        if self.color_adjust_preview_state.is_open() {
            match color_adjust_preview::show_color_adjust_preview_window(
                ctx,
                &mut self.color_adjust_preview_state,
            ) {
                ColorAdjustPreviewAction::Apply(adjustments) => {
                    self.layout_params.hsl_adjustments = adjustments;
                    self.layout_params.enable_color_adjust = true;
                    self.save_settings();
                }
                ColorAdjustPreviewAction::None => {}
            }
        }

        // Check if image processing settings changed and re-request thumbnails if needed
        let thumbnails_outdated = self.layout_params.enable_bleed != self.previous_bleed_enabled
            || (self.layout_params.bleed_mm - self.previous_bleed_mm).abs() > 0.01
            || self.layout_params.enable_sharpen != self.previous_sharpen_enabled
            || self.layout_params.sharpen_params() != self.previous_sharpen;

        if thumbnails_outdated {
            // Clear texture cache to force regeneration of preview textures
            self.preview_state.clear_texture_cache();

            // Re-request all thumbnails (fronts and backs) with new processing settings
            let thumbnail_params = ThumbnailParams::from_layout(&self.layout_params);
            for card in self
                .selected_cards
                .iter_mut()
                .chain(self.back_cards.values_mut())
            {
                // Request new thumbnail with updated processing settings
                if let Some(thumbnail) = self
                    .thumbnail_manager
                    .request_thumbnail(card.path.clone(), &thumbnail_params)
                {
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
            self.previous_sharpen_enabled = self.layout_params.enable_sharpen;
            self.previous_sharpen = self.layout_params.sharpen_params();
        }

        // Register thumbnail-managed Cards for any newly referenced back images
        self.ensure_back_cards_registered();

        // Center pane - Preview
        egui::CentralPanel::default().show(ctx, |ui| {
            preview_panel::show_preview_panel(
                ui,
                &self.layout_params,
                &self.selected_cards,
                &self.back_cards,
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
