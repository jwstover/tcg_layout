use crate::decklist::{DecklistEntry, DecklistManager, MatchedCard};
use crate::marvelcdb::UnmatchedCard;
use eframe::egui;
use std::collections::HashSet;
use std::path::PathBuf;

pub struct DecklistState {
    pub decklist_text: String,
    pub api_key: String,
    pub show_api_key: bool,
    /// True once the API key field has been edited since the last save. The
    /// keyring write only fires on focus-lost (not per keystroke), so this
    /// tracks whether there's actually a pending change to flush.
    pub api_key_dirty: bool,
    pub parsed_entries: Vec<DecklistEntry>,
    pub matched_cards: Vec<MatchedCard>,
    pub is_processing: bool,
    pub error_message: Option<String>,
    pub success_message: Option<String>,
    pub show_results: bool,
    // MarvelCDB state
    pub marvelcdb_url: String,
    pub marvelcdb_deck_name: Option<String>,
    pub marvelcdb_hero_name: Option<String>,
    pub marvelcdb_unmatched: Vec<UnmatchedCard>,
    pub marvelcdb_matched: Vec<MatchedCard>,
    pub is_fetching_marvelcdb: bool,
    pub marvelcdb_error: Option<String>,
    pub marvelcdb_success: Option<String>,
    pub marvelcdb_progress: Option<String>,
    pub marvel_champions_dir: PathBuf,
    // Google Drive state
    pub google_drive_api_key: String,
    pub show_google_drive_api_key: bool,
    /// Same debouncing purpose as `api_key_dirty`, for the Drive API key field.
    pub google_drive_api_key_dirty: bool,
    pub google_drive_folder_url: String,
    pub is_building_drive_index: bool,
    pub drive_index_updated_at: Option<u64>,
    pub drive_index_progress: Option<(usize, usize)>,
    pub drive_index_error: Option<String>,
    pub drive_index_success: Option<String>,
}

impl Default for DecklistState {
    fn default() -> Self {
        Self {
            decklist_text: String::new(),
            api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            show_api_key: false,
            api_key_dirty: false,
            parsed_entries: Vec::new(),
            matched_cards: Vec::new(),
            is_processing: false,
            error_message: None,
            success_message: None,
            show_results: false,
            marvelcdb_url: String::new(),
            marvelcdb_deck_name: None,
            marvelcdb_hero_name: None,
            marvelcdb_unmatched: Vec::new(),
            marvelcdb_matched: Vec::new(),
            is_fetching_marvelcdb: false,
            marvelcdb_error: None,
            marvelcdb_success: None,
            marvelcdb_progress: None,
            marvel_champions_dir: crate::settings::default_marvel_champions_dir(),
            google_drive_api_key: String::new(),
            show_google_drive_api_key: false,
            google_drive_api_key_dirty: false,
            google_drive_folder_url: String::new(),
            is_building_drive_index: false,
            drive_index_updated_at: None,
            drive_index_progress: None,
            drive_index_error: None,
            drive_index_success: None,
        }
    }
}

pub struct DecklistPanelActions {
    pub browse_marvel_dir_clicked: bool,
    pub api_key_changed: bool,
    pub marvel_dir_changed: bool,
    pub google_drive_api_key_changed: bool,
    pub google_drive_folder_url_changed: bool,
    pub start_build_drive_index: bool,
}

pub fn show_decklist_panel<F, M, MF, MI>(
    ui: &mut egui::Ui,
    decklist_state: &mut DecklistState,
    mut apply_decklist_callback: F,
    mut match_cards_callback: M,
    mut marvelcdb_fetch_callback: MF,
    mut marvelcdb_import_callback: MI,
) -> DecklistPanelActions
where
    F: FnMut(&[MatchedCard]),
    M: FnMut(&str, &[DecklistEntry]),
    MF: FnMut(&str, &PathBuf),
    MI: FnMut(&[MatchedCard]),
{
    // Store original values to detect changes
    let original_marvel_dir = decklist_state.marvel_champions_dir.clone();
    let original_google_drive_folder_url = decklist_state.google_drive_folder_url.clone();
    let mut start_build_index = false;
    let mut browse_marvel_dir_clicked = false;
    // API keys save to the OS keyring, which can be slow (or block on an
    // unfocused system prompt) — only flag a save on focus-lost, not on
    // every keystroke.
    let mut api_key_changed = false;
    let mut google_drive_api_key_changed = false;

    ui.heading("Decklist Import");
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(8.0);

    // --- MarvelCDB Import Section ---
    ui.collapsing("MarvelCDB Import", |ui| {
        ui.add_space(4.0);

        // URL input
        ui.label("MarvelCDB Deck URL or ID:");
        ui.text_edit_singleline(&mut decklist_state.marvelcdb_url);
        ui.add_space(4.0);

        // Images directory
        ui.horizontal(|ui| {
            ui.label("Images folder:");
            let dir_display = decklist_state.marvel_champions_dir.display().to_string();
            ui.add(
                egui::Label::new(
                    egui::RichText::new(if dir_display.len() > 40 {
                        format!("...{}", &dir_display[dir_display.len() - 37..])
                    } else {
                        dir_display
                    })
                    .weak(),
                )
                .truncate(),
            );
            if ui.button("Browse").clicked() {
                browse_marvel_dir_clicked = true;
            }
        });
        ui.add_space(4.0);

        // Fetch button
        ui.horizontal(|ui| {
            let fetch_enabled = !decklist_state.marvelcdb_url.trim().is_empty()
                && !decklist_state.is_fetching_marvelcdb;

            ui.add_enabled_ui(fetch_enabled, |ui| {
                if ui.button("Fetch Deck").clicked() {
                    decklist_state.marvelcdb_error = None;
                    decklist_state.marvelcdb_success = None;
                    decklist_state.marvelcdb_deck_name = None;
                    decklist_state.marvelcdb_hero_name = None;
                    decklist_state.marvelcdb_matched.clear();
                    decklist_state.marvelcdb_unmatched.clear();
                    decklist_state.marvelcdb_progress = None;
                    marvelcdb_fetch_callback(
                        &decklist_state.marvelcdb_url,
                        &decklist_state.marvel_champions_dir,
                    );
                }
            });

            if decklist_state.is_fetching_marvelcdb {
                ui.spinner();
                if let Some(progress) = &decklist_state.marvelcdb_progress {
                    ui.label(progress.as_str());
                } else {
                    ui.label("Fetching...");
                }
            }
        });

        // Error message
        if let Some(error) = &decklist_state.marvelcdb_error {
            ui.add_space(4.0);
            ui.colored_label(ui.visuals().error_fg_color, format!("Error: {error}"));
        }

        // Success / results
        if let Some(success) = &decklist_state.marvelcdb_success {
            ui.add_space(4.0);
            ui.colored_label(egui::Color32::from_rgb(0, 150, 0), success.as_str());
        }

        if let Some(deck_name) = &decklist_state.marvelcdb_deck_name {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Deck:");
                ui.strong(deck_name);
            });
        }

        if let Some(hero_name) = &decklist_state.marvelcdb_hero_name {
            ui.horizontal(|ui| {
                ui.label("Hero:");
                ui.strong(hero_name);
            });
        }

        // Unmatched cards warning
        if !decklist_state.marvelcdb_unmatched.is_empty() {
            ui.add_space(4.0);
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!(
                    "{} cards could not be matched:",
                    decklist_state.marvelcdb_unmatched.len()
                ),
            );

            egui::ScrollArea::vertical()
                .id_salt("marvelcdb_unmatched_scroll")
                .max_height(100.0)
                .show(ui, |ui| {
                    for card in &decklist_state.marvelcdb_unmatched {
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                ui.visuals().error_fg_color,
                                format!("{}x {} ({})", card.count, card.name, card.faction),
                            );
                        });
                    }
                });
        }

        // Matched cards
        if !decklist_state.marvelcdb_matched.is_empty() {
            ui.add_space(4.0);
            ui.label(format!(
                "{} cards matched:",
                decklist_state.marvelcdb_matched.len()
            ));

            egui::ScrollArea::vertical()
                .id_salt("marvelcdb_matched_scroll")
                .max_height(150.0)
                .show(ui, |ui| {
                    for matched in &decklist_state.marvelcdb_matched {
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                ui.visuals().strong_text_color(),
                                format!("{}x", matched.count),
                            );
                            ui.label(&matched.card_name);
                            ui.label("->");
                            ui.colored_label(
                                ui.visuals().weak_text_color(),
                                matched
                                    .matched_path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string(),
                            );
                            ui.colored_label(
                                if matched.confidence > 0.95 {
                                    egui::Color32::from_rgb(0, 150, 0)
                                } else if matched.confidence > 0.85 {
                                    egui::Color32::from_rgb(200, 150, 0)
                                } else {
                                    ui.visuals().error_fg_color
                                },
                                format!("{:.0}%", matched.confidence * 100.0),
                            );
                        });
                    }
                });

            ui.add_space(4.0);
            if ui.button("Apply to Card List").clicked() {
                marvelcdb_import_callback(&decklist_state.marvelcdb_matched);
                decklist_state.marvelcdb_success =
                    Some("MarvelCDB decklist applied to card list!".to_string());
            }
        }
    });

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(8.0);

    // API Key section
    ui.collapsing("API Configuration", |ui| {
        ui.horizontal(|ui| {
            ui.label("OpenAI API Key:");
            if decklist_state.show_api_key {
                let response = ui
                    .text_edit_singleline(&mut decklist_state.api_key)
                    .on_hover_text("Your OpenAI API key for card name matching");
                if response.changed() {
                    decklist_state.api_key_dirty = true;
                }
                if response.lost_focus() && decklist_state.api_key_dirty {
                    api_key_changed = true;
                    decklist_state.api_key_dirty = false;
                }
            } else {
                let mut masked_key = "*".repeat(decklist_state.api_key.len().min(20));
                if ui.text_edit_singleline(&mut masked_key).changed() {
                    // If user tries to edit the masked field, show the real field
                    decklist_state.show_api_key = true;
                }
            }
        });

        ui.horizontal(|ui| {
            if ui
                .checkbox(&mut decklist_state.show_api_key, "Show API key")
                .changed()
                && !decklist_state.show_api_key
            {
                // Hide the key again
            }
            ui.colored_label(
                ui.visuals().weak_text_color(),
                "Set OPENAI_API_KEY environment variable or enter here",
            );
        });

        ui.add_space(4.0);
        ui.colored_label(
            ui.visuals().weak_text_color(),
            "API key is used to match card names to image filenames using AI",
        );
    });

    ui.add_space(8.0);

    // Google Drive Configuration section
    ui.collapsing("Google Drive", |ui| {
        ui.add_space(4.0);
        ui.colored_label(
            ui.visuals().weak_text_color(),
            "Configure Google Drive for automatic card image downloading",
        );
        ui.add_space(4.0);

        // API Key
        ui.horizontal(|ui| {
            ui.label("Drive API Key:");
            if decklist_state.show_google_drive_api_key {
                let response = ui
                    .text_edit_singleline(&mut decklist_state.google_drive_api_key)
                    .on_hover_text("Google Cloud API key with Drive API enabled");
                if response.changed() {
                    decklist_state.google_drive_api_key_dirty = true;
                }
                if response.lost_focus() && decklist_state.google_drive_api_key_dirty {
                    google_drive_api_key_changed = true;
                    decklist_state.google_drive_api_key_dirty = false;
                }
            } else {
                let mut masked = "*".repeat(decklist_state.google_drive_api_key.len().min(20));
                if ui.text_edit_singleline(&mut masked).changed() {
                    decklist_state.show_google_drive_api_key = true;
                }
            }
        });
        ui.checkbox(
            &mut decklist_state.show_google_drive_api_key,
            "Show API key",
        );

        ui.add_space(4.0);

        // Folder URL
        ui.label("Drive Folder URL or ID:");
        ui.text_edit_singleline(&mut decklist_state.google_drive_folder_url)
            .on_hover_text("Paste the Google Drive folder link or folder ID");

        ui.add_space(4.0);

        // Drive Index
        ui.horizontal(|ui| {
            let index_enabled = !decklist_state.google_drive_api_key.trim().is_empty()
                && !decklist_state.google_drive_folder_url.trim().is_empty()
                && !decklist_state.is_building_drive_index;

            ui.add_enabled_ui(index_enabled, |ui| {
                let label = if decklist_state.drive_index_updated_at.is_some() {
                    "Refresh Index"
                } else {
                    "Build Index"
                };
                if ui.button(label).clicked() {
                    decklist_state.drive_index_error = None;
                    decklist_state.drive_index_success = None;
                    start_build_index = true;
                }
            });

            if decklist_state.is_building_drive_index {
                ui.spinner();
                if let Some((scanned, total)) = &decklist_state.drive_index_progress {
                    ui.label(format!("Scanning folders... {scanned}/{total}"));
                } else {
                    ui.label("Building index...");
                }
            }
        });

        if let Some(updated_at) = decklist_state.drive_index_updated_at {
            ui.colored_label(
                ui.visuals().weak_text_color(),
                format!("Index last updated: {}", format_index_age(updated_at)),
            );
        }

        if let Some(error) = &decklist_state.drive_index_error {
            ui.colored_label(ui.visuals().error_fg_color, format!("Drive error: {error}"));
        }

        if let Some(success) = &decklist_state.drive_index_success {
            ui.colored_label(egui::Color32::from_rgb(0, 150, 0), success.as_str());
        }
    });

    ui.add_space(8.0);

    // Decklist input section
    ui.label("Decklist:");
    ui.colored_label(
        ui.visuals().weak_text_color(),
        "Paste your decklist here. Supports formats like '4 Lightning Bolt', '2x Shock', 'Fireball x1', 'Counterspell (3)'",
    );

    let text_area = egui::TextEdit::multiline(&mut decklist_state.decklist_text)
        .desired_width(f32::INFINITY)
        .desired_rows(8);
    ui.add(text_area);

    ui.add_space(8.0);

    // Action buttons
    ui.horizontal(|ui| {
        let process_enabled = !decklist_state.decklist_text.trim().is_empty()
            && !decklist_state.api_key.trim().is_empty()
            && !decklist_state.is_processing;

        ui.add_enabled_ui(process_enabled, |ui| {
            if ui.button("Parse & Match Cards").clicked() {
                decklist_state.error_message = None;
                decklist_state.success_message = None;
                decklist_state.show_results = false;

                let manager = DecklistManager::new();

                // Parse the decklist
                match manager.parse_decklist(&decklist_state.decklist_text) {
                    Ok(entries) => {
                        decklist_state.parsed_entries = entries.clone();
                        decklist_state.is_processing = true;
                        // Trigger async AI matching
                        match_cards_callback(&decklist_state.api_key, &entries);
                        decklist_state.success_message = Some(format!(
                            "Parsed {} cards from decklist. Matching with AI...",
                            entries.len()
                        ));
                        decklist_state.show_results = true;
                    }
                    Err(e) => {
                        decklist_state.error_message =
                            Some(format!("Failed to parse decklist: {e}"));
                    }
                }
            }
        });

        if decklist_state.is_processing {
            ui.spinner();
            ui.label("Processing...");
        }
    });

    // Clear button
    ui.horizontal(|ui| {
        if ui.button("Clear").clicked() {
            decklist_state.decklist_text.clear();
            decklist_state.parsed_entries.clear();
            decklist_state.matched_cards.clear();
            decklist_state.error_message = None;
            decklist_state.success_message = None;
            decklist_state.show_results = false;
        }
    });

    ui.add_space(8.0);

    // Error message
    if let Some(error) = &decklist_state.error_message {
        ui.colored_label(ui.visuals().error_fg_color, format!("❌ {error}"));
        ui.add_space(4.0);
    }

    // Success message
    if let Some(success) = &decklist_state.success_message {
        ui.colored_label(egui::Color32::from_rgb(0, 150, 0), format!("✓ {success}"));
        ui.add_space(4.0);
    }

    // Results section
    if decklist_state.show_results {
        ui.separator();
        ui.add_space(8.0);

        if !decklist_state.parsed_entries.is_empty() {
            ui.heading("Parsed Cards");
            ui.add_space(4.0);

            egui::ScrollArea::vertical()
                .id_salt("decklist_parsed_cards_scroll")
                .max_height(150.0)
                .show(ui, |ui| {
                    for entry in &decklist_state.parsed_entries {
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                ui.visuals().strong_text_color(),
                                format!("{}x", entry.count),
                            );
                            ui.label(&entry.card_name);
                        });
                    }
                });

            ui.add_space(8.0);
        }

        // Show unmatched cards first
        if !decklist_state.parsed_entries.is_empty() && !decklist_state.matched_cards.is_empty() {
            // Find entries that weren't matched
            let matched_names: HashSet<String> = decklist_state
                .matched_cards
                .iter()
                .map(|m| m.card_name.clone())
                .collect();

            let unmatched_entries: Vec<&DecklistEntry> = decklist_state
                .parsed_entries
                .iter()
                .filter(|entry| !matched_names.contains(&entry.card_name))
                .collect();

            if !unmatched_entries.is_empty() {
                ui.heading("Unmatched Cards");
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!(
                        "⚠️ {} cards from your decklist could not be matched to image files:",
                        unmatched_entries.len()
                    ),
                );
                ui.add_space(4.0);

                egui::ScrollArea::vertical()
                    .id_salt("decklist_unmatched_cards_scroll")
                    .max_height(120.0)
                    .show(ui, |ui| {
                        for entry in &unmatched_entries {
                            ui.horizontal(|ui| {
                                ui.colored_label(ui.visuals().error_fg_color, "❌");
                                ui.colored_label(
                                    ui.visuals().strong_text_color(),
                                    format!("{}x", entry.count),
                                );
                                ui.colored_label(ui.visuals().error_fg_color, &entry.card_name);
                            });
                        }
                    });

                ui.add_space(8.0);
            }
        }

        if !decklist_state.matched_cards.is_empty() {
            ui.heading("Matched Cards");
            ui.add_space(4.0);

            egui::ScrollArea::vertical()
                .id_salt("decklist_matched_cards_scroll")
                .max_height(200.0)
                .show(ui, |ui| {
                    for matched in &decklist_state.matched_cards {
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                ui.visuals().strong_text_color(),
                                format!("{}x", matched.count),
                            );
                            ui.label(&matched.card_name);
                            ui.label("→");
                            ui.colored_label(
                                ui.visuals().weak_text_color(),
                                matched
                                    .matched_path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string(),
                            );
                            ui.colored_label(
                                if matched.confidence > 0.8 {
                                    egui::Color32::from_rgb(0, 150, 0)
                                } else if matched.confidence > 0.6 {
                                    egui::Color32::from_rgb(200, 150, 0)
                                } else {
                                    ui.visuals().error_fg_color
                                },
                                format!("{:.0}%", matched.confidence * 100.0),
                            );
                        });
                    }
                });

            ui.add_space(8.0);

            // Apply button
            if ui.button("Apply to Card List").clicked() {
                apply_decklist_callback(&decklist_state.matched_cards);
                decklist_state.success_message = Some("Decklist applied to card list!".to_string());
            }
        }
    }

    // Instructions
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(8.0);

    ui.collapsing("Instructions", |ui| {
        ui.label("How to use:");
        ui.label("1. Set your OpenAI API key above or via OPENAI_API_KEY environment variable");
        ui.label("2. Import card images first using File → Import Images");
        ui.label("3. Paste your decklist in the text area above");
        ui.label("4. Click 'Parse & Match Cards' to process");
        ui.label("5. Review the matches and click 'Apply to Card List'");
        ui.add_space(4.0);

        ui.label("Supported decklist formats:");
        ui.label("• 4 Lightning Bolt");
        ui.label("• 2x Shock");
        ui.label("• Fireball x1");
        ui.label("• Counterspell (3)");
        ui.label("• Plain card names (defaults to 1 copy)");
    });

    DecklistPanelActions {
        browse_marvel_dir_clicked,
        api_key_changed,
        marvel_dir_changed: decklist_state.marvel_champions_dir != original_marvel_dir,
        google_drive_api_key_changed,
        google_drive_folder_url_changed: decklist_state.google_drive_folder_url
            != original_google_drive_folder_url,
        start_build_drive_index: start_build_index,
    }
}

fn format_index_age(updated_at: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let age_secs = now.saturating_sub(updated_at);

    if age_secs < 60 {
        "just now".to_string()
    } else if age_secs < 3600 {
        format!("{} min ago", age_secs / 60)
    } else if age_secs < 86400 {
        format!("{} hours ago", age_secs / 3600)
    } else {
        format!("{} days ago", age_secs / 86400)
    }
}
