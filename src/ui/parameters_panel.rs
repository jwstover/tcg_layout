use eframe::egui;
use crate::types::{LayoutParams, FillOrder, PageOrientation};
use super::{PageSizeOption, CardSizeOption};

pub fn show_parameters_panel(
    ui: &mut egui::Ui,
    layout_params: &mut LayoutParams,
    validation_errors: &mut Vec<String>,
    show_success_message: &mut bool,
    success_message_timer: &mut f32,
    page_size_option: &mut PageSizeOption,
    card_size_option: &mut CardSizeOption,
) {
    ui.heading("Layout Parameters");

    ui.separator();

    egui::Grid::new("params_grid")
        .num_columns(2)
        .spacing([40.0, 4.0])
        .show(ui, |ui| {
            ui.label("Page Size:");
            ui.vertical(|ui| {
                egui::ComboBox::from_id_source("page_size_combo")
                    .selected_text(page_size_option.display_name())
                    .show_ui(ui, |ui| {
                        if ui.selectable_value(page_size_option, PageSizeOption::A4, PageSizeOption::A4.display_name()).clicked() {
                            if let Some(size) = PageSizeOption::A4.get_size() {
                                layout_params.page_size = size;
                            }
                        }
                        if ui.selectable_value(page_size_option, PageSizeOption::USLetter, PageSizeOption::USLetter.display_name()).clicked() {
                            if let Some(size) = PageSizeOption::USLetter.get_size() {
                                layout_params.page_size = size;
                            }
                        }
                        if ui.selectable_value(page_size_option, PageSizeOption::A3, PageSizeOption::A3.display_name()).clicked() {
                            if let Some(size) = PageSizeOption::A3.get_size() {
                                layout_params.page_size = size;
                            }
                        }
                        ui.selectable_value(page_size_option, PageSizeOption::Custom, PageSizeOption::Custom.display_name());
                    });
                
                if *page_size_option == PageSizeOption::Custom {
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut layout_params.page_size.0)
                            .speed(1.0)
                            .range(10.0..=1000.0)
                            .suffix(" mm"));
                        ui.label("×");
                        ui.add(egui::DragValue::new(&mut layout_params.page_size.1)
                            .speed(1.0)
                            .range(10.0..=1000.0)
                            .suffix(" mm"));
                    });
                }
            });
            ui.end_row();

            ui.label("Page Orientation:");
            egui::ComboBox::from_id_source("page_orientation_combo")
                .selected_text(match layout_params.page_orientation {
                    PageOrientation::Portrait => "Portrait",
                    PageOrientation::Landscape => "Landscape",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut layout_params.page_orientation, PageOrientation::Portrait, "Portrait");
                    ui.selectable_value(&mut layout_params.page_orientation, PageOrientation::Landscape, "Landscape");
                });
            ui.end_row();

            ui.label("Card Size:");
            ui.vertical(|ui| {
                egui::ComboBox::from_id_source("card_size_combo")
                    .selected_text(card_size_option.display_name())
                    .show_ui(ui, |ui| {
                        if ui.selectable_value(card_size_option, CardSizeOption::Poker, CardSizeOption::Poker.display_name()).clicked() {
                            if let Some(size) = CardSizeOption::Poker.get_size() {
                                layout_params.card_size = size;
                            }
                        }
                        if ui.selectable_value(card_size_option, CardSizeOption::Bridge, CardSizeOption::Bridge.display_name()).clicked() {
                            if let Some(size) = CardSizeOption::Bridge.get_size() {
                                layout_params.card_size = size;
                            }
                        }
                        if ui.selectable_value(card_size_option, CardSizeOption::Tarot, CardSizeOption::Tarot.display_name()).clicked() {
                            if let Some(size) = CardSizeOption::Tarot.get_size() {
                                layout_params.card_size = size;
                            }
                        }
                        ui.selectable_value(card_size_option, CardSizeOption::Custom, CardSizeOption::Custom.display_name());
                    });
                
                if *card_size_option == CardSizeOption::Custom {
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut layout_params.card_size.0)
                            .speed(0.1)
                            .range(1.0..=500.0)
                            .suffix(" mm"));
                        ui.label("×");
                        ui.add(egui::DragValue::new(&mut layout_params.card_size.1)
                            .speed(0.1)
                            .range(1.0..=500.0)
                            .suffix(" mm"));
                    });
                }
            });
            ui.end_row();

            ui.label("Margins (mm):");
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label("Top:");
                    ui.add(egui::DragValue::new(&mut layout_params.margins.top)
                        .speed(0.1)
                        .range(0.0..=100.0)
                        .suffix(" mm"));
                    ui.label("Right:");
                    ui.add(egui::DragValue::new(&mut layout_params.margins.right)
                        .speed(0.1)
                        .range(0.0..=100.0)
                        .suffix(" mm"));
                });
                ui.horizontal(|ui| {
                    ui.label("Bottom:");
                    ui.add(egui::DragValue::new(&mut layout_params.margins.bottom)
                        .speed(0.1)
                        .range(0.0..=100.0)
                        .suffix(" mm"));
                    ui.label("Left:");
                    ui.add(egui::DragValue::new(&mut layout_params.margins.left)
                        .speed(0.1)
                        .range(0.0..=100.0)
                        .suffix(" mm"));
                });
            });
            ui.end_row();

            ui.label("Spacing (mm):");
            ui.horizontal(|ui| {
                ui.label("Horizontal:");
                ui.add(egui::DragValue::new(&mut layout_params.spacing.0)
                    .speed(0.1)
                    .range(0.0..=50.0)
                    .suffix(" mm"));
                ui.label("Vertical:");
                ui.add(egui::DragValue::new(&mut layout_params.spacing.1)
                    .speed(0.1)
                    .range(0.0..=50.0)
                    .suffix(" mm"));
            });
            ui.end_row();

            ui.label("Fill Order:");
            egui::ComboBox::from_id_source("fill_order_combo")
                .selected_text(match layout_params.orientation {
                    FillOrder::RowMajor => "Row Major (left to right, then down)",
                    FillOrder::ColumnMajor => "Column Major (top to bottom, then right)",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut layout_params.orientation, FillOrder::RowMajor, "Row Major (left to right, then down)");
                    ui.selectable_value(&mut layout_params.orientation, FillOrder::ColumnMajor, "Column Major (top to bottom, then right)");
                });
            ui.end_row();

            ui.label("Target DPI:");
            ui.add(egui::DragValue::new(&mut layout_params.target_dpi)
                .speed(10)
                .range(72..=600));
            ui.end_row();
        });

    ui.separator();

    if ui.button("Validate Parameters").clicked() {
        validate_params(layout_params, validation_errors, show_success_message, success_message_timer);
    }

    if !validation_errors.is_empty() {
        ui.separator();
        ui.colored_label(egui::Color32::RED, "Validation Errors:");
        for error in validation_errors.iter() {
            ui.colored_label(egui::Color32::RED, format!("• {}", error));
        }
    }

    if ui.button("Reset to Defaults").clicked() {
        *layout_params = LayoutParams::default();
        *page_size_option = PageSizeOption::A4;
        *card_size_option = CardSizeOption::Poker;
        validation_errors.clear();
    }
}

fn validate_params(
    layout_params: &LayoutParams,
    validation_errors: &mut Vec<String>,
    show_success_message: &mut bool,
    success_message_timer: &mut f32,
) {
    validation_errors.clear();

    if layout_params.page_size.0 <= 0.0 || layout_params.page_size.1 <= 0.0 {
        validation_errors.push("Page size must be positive".to_string());
    }

    if layout_params.card_size.0 <= 0.0 || layout_params.card_size.1 <= 0.0 {
        validation_errors.push("Card size must be positive".to_string());
    }

    if layout_params.spacing.0 < 0.0 || layout_params.spacing.1 < 0.0 {
        validation_errors.push("Spacing cannot be negative".to_string());
    }

    if layout_params.target_dpi == 0 {
        validation_errors.push("Target DPI must be greater than 0".to_string());
    }

    let total_margin_width = layout_params.margins.left + layout_params.margins.right;
    let total_margin_height = layout_params.margins.top + layout_params.margins.bottom;

    if layout_params.card_size.0 + total_margin_width >= layout_params.page_size.0 {
        validation_errors.push("Card width plus margins exceeds page width".to_string());
    }

    if layout_params.card_size.1 + total_margin_height >= layout_params.page_size.1 {
        validation_errors.push("Card height plus margins exceeds page height".to_string());
    }

    if validation_errors.is_empty() {
        *show_success_message = true;
        *success_message_timer = 3.0; // Show for 3 seconds
    }
}