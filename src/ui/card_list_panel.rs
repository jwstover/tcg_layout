use crate::types::Card;
use eframe::egui;

// Helper function to get brighter text color
fn bright_text_color(ui: &egui::Ui) -> egui::Color32 {
    let base_color = ui.visuals().text_color();
    // Brighten the text by increasing the luminance
    egui::Color32::from_rgba_unmultiplied(
        (base_color.r() as f32 * 1.3).min(255.0) as u8,
        (base_color.g() as f32 * 1.3).min(255.0) as u8,
        (base_color.b() as f32 * 1.3).min(255.0) as u8,
        base_color.a(),
    )
}

// Helper function to get brighter weak text color
fn bright_weak_text_color(ui: &egui::Ui) -> egui::Color32 {
    let base_color = ui.visuals().weak_text_color();
    // Brighten weak text more significantly
    egui::Color32::from_rgba_unmultiplied(
        (base_color.r() as f32 * 1.5).min(255.0) as u8,
        (base_color.g() as f32 * 1.5).min(255.0) as u8,
        (base_color.b() as f32 * 1.5).min(255.0) as u8,
        base_color.a(),
    )
}

pub fn show_card_list_panel<F, I, C>(
    ui: &mut egui::Ui,
    cards: &[Card],
    mut remove_callback: F,
    mut import_callback: I,
    mut copy_count_callback: C,
) where
    F: FnMut(usize),
    I: FnMut(),
    C: FnMut(usize, u32),
{
    ui.heading("Card List");
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(8.0);

    // Show card count and total copies
    ui.horizontal(|ui| {
        let total_copies: u32 = cards.iter().map(|card| card.copy_count).sum();
        ui.colored_label(
            bright_text_color(ui),
            format!("Cards: {} ({} copies)", cards.len(), total_copies),
        );
        if ui.small_button("Import").clicked() {
            import_callback();
        }
    });

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(8.0);

    if cards.is_empty() {
        ui.vertical_centered_justified(|ui| {
            ui.add_space(20.0);
            ui.strong("No cards selected");
            ui.add_space(10.0);
            ui.colored_label(
                bright_weak_text_color(ui),
                "Use File → Import Images... to add cards",
            );
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
                                ui.colored_label(
                                    bright_text_color(ui),
                                    filename.to_string_lossy().to_string(),
                                );
                            } else {
                                ui.colored_label(bright_text_color(ui), "Unknown file");
                            }

                            // Show file path in smaller text
                            ui.colored_label(
                                bright_weak_text_color(ui),
                                card.path.to_string_lossy().to_string(),
                            );

                            // Copy count controls below the card info
                            ui.horizontal(|ui| {
                                ui.small("Copies:");
                                if ui.small_button("−").clicked() {
                                    let new_count = card.copy_count.saturating_sub(1).max(1);
                                    copy_count_changes.push((index, new_count));
                                }
                                ui.colored_label(
                                    bright_text_color(ui),
                                    format!("{}", card.copy_count),
                                );
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

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);
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
