use eframe::egui;
use crate::types::Card;

pub fn show_card_list_panel<F, I>(ui: &mut egui::Ui, cards: &[Card], mut remove_callback: F, mut import_callback: I) 
where
    F: FnMut(usize),
    I: FnMut(),
{
    ui.heading("Card List");
    ui.separator();
    
    // Show card count
    ui.horizontal(|ui| {
        ui.label(format!("Cards: {}", cards.len()));
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
                
                for (index, card) in cards.iter().enumerate() {
                    ui.horizontal(|ui| {
                        // Card info
                        ui.vertical(|ui| {
                            if let Some(filename) = card.path.file_name() {
                                ui.label(filename.to_string_lossy().to_string());
                            } else {
                                ui.label("Unknown file");
                            }
                            
                            // Show file path in smaller text
                            ui.small(card.path.to_string_lossy().to_string());
                            
                            // Show DPI info if available
                            if let Some(dpi) = card.original_dpi {
                                ui.small(format!("DPI: {}", dpi));
                            }
                        });
                        
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("❌").clicked() {
                                to_remove = Some(index);
                            }
                        });
                    });
                    
                    ui.separator();
                }
                
                // Handle removal after iteration
                if let Some(index) = to_remove {
                    remove_callback(index);
                }
            });
    }
}