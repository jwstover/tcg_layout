use crate::decklist::{DecklistEntry, DecklistManager, MatchedCard};
use eframe::egui;

pub struct DecklistState {
    pub decklist_text: String,
    pub api_key: String,
    pub show_api_key: bool,
    pub parsed_entries: Vec<DecklistEntry>,
    pub matched_cards: Vec<MatchedCard>,
    pub is_processing: bool,
    pub error_message: Option<String>,
    pub success_message: Option<String>,
    pub show_results: bool,
}

impl Default for DecklistState {
    fn default() -> Self {
        Self {
            decklist_text: String::new(),
            api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            show_api_key: false,
            parsed_entries: Vec::new(),
            matched_cards: Vec::new(),
            is_processing: false,
            error_message: None,
            success_message: None,
            show_results: false,
        }
    }
}

pub fn show_decklist_panel<F, M>(
    ui: &mut egui::Ui,
    decklist_state: &mut DecklistState,
    mut apply_decklist_callback: F,
    mut match_cards_callback: M,
) where
    F: FnMut(&[MatchedCard]),
    M: FnMut(&str, &[DecklistEntry]),
{
    ui.heading("Decklist Import");
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(8.0);

    // API Key section
    ui.collapsing("API Configuration", |ui| {
        ui.horizontal(|ui| {
            ui.label("OpenAI API Key:");
            if decklist_state.show_api_key {
                ui.text_edit_singleline(&mut decklist_state.api_key).on_hover_text("Your OpenAI API key for card name matching");
            } else {
                let mut masked_key = "*".repeat(decklist_state.api_key.len().min(20));
                if ui.text_edit_singleline(&mut masked_key).changed() {
                    // If user tries to edit the masked field, show the real field
                    decklist_state.show_api_key = true;
                }
            }
        });
        
        ui.horizontal(|ui| {
            if ui.checkbox(&mut decklist_state.show_api_key, "Show API key").changed() && !decklist_state.show_api_key {
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
                        decklist_state.success_message = Some(format!("Parsed {} cards from decklist. Matching with AI...", entries.len()));
                        decklist_state.show_results = true;
                    }
                    Err(e) => {
                        decklist_state.error_message = Some(format!("Failed to parse decklist: {}", e));
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
        ui.colored_label(ui.visuals().error_fg_color, format!("❌ {}", error));
        ui.add_space(4.0);
    }

    // Success message
    if let Some(success) = &decklist_state.success_message {
        ui.colored_label(egui::Color32::from_rgb(0, 150, 0), format!("✓ {}", success));
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
                .id_source("decklist_parsed_cards_scroll")
                .max_height(150.0)
                .show(ui, |ui| {
                    for entry in &decklist_state.parsed_entries {
                        ui.horizontal(|ui| {
                            ui.colored_label(ui.visuals().strong_text_color(), format!("{}x", entry.count));
                            ui.label(&entry.card_name);
                        });
                    }
                });
            
            ui.add_space(8.0);
        }

        if !decklist_state.matched_cards.is_empty() {
            ui.heading("Matched Cards");
            ui.add_space(4.0);
            
            egui::ScrollArea::vertical()
                .id_source("decklist_matched_cards_scroll")
                .max_height(200.0)
                .show(ui, |ui| {
                    for matched in &decklist_state.matched_cards {
                        ui.horizontal(|ui| {
                            ui.colored_label(ui.visuals().strong_text_color(), format!("{}x", matched.count));
                            ui.label(&matched.card_name);
                            ui.label("→");
                            ui.colored_label(ui.visuals().weak_text_color(), 
                                matched.matched_path.file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string()
                            );
                            ui.colored_label(
                                if matched.confidence > 0.8 {
                                    egui::Color32::from_rgb(0, 150, 0)
                                } else if matched.confidence > 0.6 {
                                    egui::Color32::from_rgb(200, 150, 0)
                                } else {
                                    ui.visuals().error_fg_color
                                },
                                format!("{:.0}%", matched.confidence * 100.0)
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
}