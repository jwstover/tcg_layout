use eframe::egui;
use crate::types::Card;

pub fn show_card_list_panel<F, I, C>(ui: &mut egui::Ui, cards: &[Card], mut remove_callback: F, mut import_callback: I, mut copy_count_callback: C) 
where
    F: FnMut(usize),
    I: FnMut(),
    C: FnMut(usize, u32),
{
    ui.heading("Card List");
    ui.separator();
    
    // Show card count and total copies
    ui.horizontal(|ui| {
        let total_copies: u32 = cards.iter().map(|card| card.copy_count).sum();
        ui.label(format!("Cards: {} ({} copies)", cards.len(), total_copies));
        if ui.small_button("Import").clicked() {
            import_callback();
        }
    });
    
    ui.separator();
    
    if cards.is_empty() {
        ui.vertical_centered_justified(|ui| {
            ui.add_space(20.0);
            ui.label("No cards selected");
            ui.add_space(10.0);
            ui.label("Use File → Import Images... to add cards");
        });
    } else {
        // Scrollable list of cards
        egui::ScrollArea::vertical()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let mut to_remove = None;
                let mut copy_count_changes = Vec::new();
                
                for (index, card) in cards.iter().enumerate() {
                    ui.horizontal(|ui| {
                        // Card info and copy count controls
                        ui.vertical(|ui| {
                            // Card filename
                            if let Some(filename) = card.path.file_name() {
                                ui.label(filename.to_string_lossy().to_string());
                            } else {
                                ui.label("Unknown file");
                            }
                            
                            // Show file path in smaller text
                            ui.small(card.path.to_string_lossy().to_string());
                            
                            // Copy count controls below the card info
                            ui.horizontal(|ui| {
                                ui.small("Copies:");
                                if ui.small_button("−").clicked() {
                                    let new_count = card.copy_count.saturating_sub(1).max(1);
                                    copy_count_changes.push((index, new_count));
                                }
                                ui.small(format!("{}", card.copy_count));
                                if ui.small_button("+").clicked() {
                                    let new_count = card.copy_count + 1;
                                    copy_count_changes.push((index, new_count));
                                }
                            });
                        });
                        
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("❌").clicked() {
                                to_remove = Some(index);
                            }
                        });
                    });
                    
                    ui.separator();
                }
                
                // Handle copy count changes after iteration
                for (index, new_count) in copy_count_changes {
                    copy_count_callback(index, new_count);
                }
                
                // Handle removal after iteration
                if let Some(index) = to_remove {
                    remove_callback(index);
                }
            });
    }
}