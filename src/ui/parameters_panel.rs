use super::{CardSizeOption, PageSizeOption};
use crate::types::{FillOrder, FlipEdge, LayoutParams, PageOrientation};
use eframe::egui;

/// Upper bound on a single printer margin. Real forced margins are a few
/// millimetres; this leaves headroom for oddities without letting a slip
/// collapse the printable area.
const MAX_PRINTER_MARGIN_MM: f32 = 30.0;

#[allow(clippy::too_many_arguments)]
pub fn show_parameters_panel(
    ui: &mut egui::Ui,
    layout_params: &mut LayoutParams,
    validation_errors: &mut Vec<String>,
    show_success_message: &mut bool,
    success_message_timer: &mut f32,
    page_size_option: &mut PageSizeOption,
    card_size_option: &mut CardSizeOption,
    has_cards: bool,
    sharpen_preview_requested: &mut bool,
    color_adjust_preview_requested: &mut bool,
    default_back_pick_requested: &mut bool,
) -> bool {
    // Store original values to detect changes
    let original_layout_params = layout_params.clone();
    let original_page_size_option = *page_size_option;
    let original_card_size_option = *card_size_option;

    ui.heading("Layout Parameters");
    ui.add_space(8.0);

    ui.separator();
    ui.add_space(8.0);

    egui::Grid::new("params_grid")
        .num_columns(2)
        .spacing([40.0, 4.0])
        .show(ui, |ui| {
            ui.label("Page Size:");
            ui.vertical(|ui| {
                egui::ComboBox::from_id_salt("page_size_combo")
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
            egui::ComboBox::from_id_salt("page_orientation_combo")
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
                egui::ComboBox::from_id_salt("card_size_combo")
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
                if layout_params.center_layout {
                    // Calculate and display centered margins (read-only)
                    let grid = crate::layout::calculate_grid(layout_params);
                    let effective_margins = layout_params.effective_margins(&grid);

                    ui.horizontal(|ui| {
                        ui.label("Top:");
                        ui.colored_label(
                            ui.visuals().weak_text_color(),
                            format!("{:.1} mm (auto)", effective_margins.top)
                        );
                        ui.label("Right:");
                        ui.colored_label(
                            ui.visuals().weak_text_color(),
                            format!("{:.1} mm (auto)", effective_margins.right)
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Bottom:");
                        ui.colored_label(
                            ui.visuals().weak_text_color(),
                            format!("{:.1} mm (auto)", effective_margins.bottom)
                        );
                        ui.label("Left:");
                        ui.colored_label(
                            ui.visuals().weak_text_color(),
                            format!("{:.1} mm (auto)", effective_margins.left)
                        );
                    });
                } else {
                    // Existing editable margin controls
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
                }
            });
            ui.end_row();

            // Printer margins section: the unprintable border the printer
            // forces, which the layout keeps clear and the export pages match.
            ui.horizontal(|ui| {
                ui.label("Printer Margins:");
                ui.colored_label(ui.visuals().weak_text_color(), "(?)").on_hover_text(
                    "The border your printer physically cannot print on.\n\
                     Enter it from the printer's specs, or measure a test print.\n\n\
                     Exported pages are then sized to the printable area instead\n\
                     of the whole sheet, so the driver no longer has to shrink or\n\
                     shift the page to fit it: cards print at exact size and stay\n\
                     aligned on the paper. Cards and cut marks are also kept out\n\
                     of the border, so no mark gets clipped."
                );
            });
            ui.checkbox(&mut layout_params.enable_printer_margins, "Printer has forced margins");
            ui.end_row();

            if layout_params.enable_printer_margins {
                ui.label("Unprintable (mm):");
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label("Top:");
                        ui.add(egui::DragValue::new(&mut layout_params.printer_margins.top)
                            .speed(0.1)
                            .range(0.0..=MAX_PRINTER_MARGIN_MM)
                            .suffix(" mm"));
                        ui.label("Right:");
                        ui.add(egui::DragValue::new(&mut layout_params.printer_margins.right)
                            .speed(0.1)
                            .range(0.0..=MAX_PRINTER_MARGIN_MM)
                            .suffix(" mm"));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Bottom:");
                        ui.add(egui::DragValue::new(&mut layout_params.printer_margins.bottom)
                            .speed(0.1)
                            .range(0.0..=MAX_PRINTER_MARGIN_MM)
                            .suffix(" mm"));
                        ui.label("Left:");
                        ui.add(egui::DragValue::new(&mut layout_params.printer_margins.left)
                            .speed(0.1)
                            .range(0.0..=MAX_PRINTER_MARGIN_MM)
                            .suffix(" mm"));
                    });
                });
                ui.end_row();

                let (sheet_w, sheet_h) = layout_params.effective_page_size();
                let (printable_w, printable_h) = layout_params.printable_size();
                ui.label("");
                ui.vertical(|ui| {
                    ui.colored_label(
                        ui.visuals().weak_text_color(),
                        format!(
                            "Export page: {printable_w:.1} × {printable_h:.1} mm \
                             (printable area of {sheet_w:.0} × {sheet_h:.0})"
                        ),
                    );
                    ui.colored_label(
                        ui.visuals().weak_text_color(),
                        "Print at 100% / \"Actual size\"; \"Fit to printable area\"\n\
                         now scales by 1.0, so either setting is correct.",
                    );
                });
                ui.end_row();
            }

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

            ui.horizontal(|ui| {
                ui.label("Fill Order:");
                ui.colored_label(ui.visuals().weak_text_color(), "(?)").on_hover_text(
                    "Determines how cards are arranged:\n• Row Major: Fill left to right, then down\n• Column Major: Fill top to bottom, then right"
                );
            });
            egui::ComboBox::from_id_salt("fill_order_combo")
                .selected_text(match layout_params.orientation {
                    FillOrder::RowMajor => "Row Major (left to right, then down)",
                    FillOrder::ColumnMajor => "Column Major (top to bottom, then right)",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut layout_params.orientation, FillOrder::RowMajor, "Row Major (left to right, then down)");
                    ui.selectable_value(&mut layout_params.orientation, FillOrder::ColumnMajor, "Column Major (top to bottom, then right)");
                });
            ui.end_row();

            ui.horizontal(|ui| {
                ui.label("Center Layout:");
                ui.colored_label(ui.visuals().weak_text_color(), "(?)").on_hover_text(
                    "Automatically center the card grid on the page.\nWhen enabled, margins are calculated to center the layout."
                );
            });
            ui.checkbox(&mut layout_params.center_layout, "Center on page");
            ui.end_row();

            ui.horizontal(|ui| {
                ui.label("Target DPI:");
                ui.colored_label(ui.visuals().weak_text_color(), "(?)").on_hover_text(
                    "Target resolution for export.\nHigher DPI = better quality but larger file size.\nCommon values: 72 (screen), 150 (draft), 300 (print)"
                );
            });
            ui.add(egui::DragValue::new(&mut layout_params.target_dpi)
                .speed(10)
                .range(72..=600));
            ui.end_row();

            // Bleed section
            ui.horizontal(|ui| {
                ui.label("Print Bleed:");
                ui.colored_label(ui.visuals().weak_text_color(), "(?)").on_hover_text(
                    "Extends card images beyond edges for professional printing.\n\
                     Recommended: 3mm for most print shops.\n\
                     Bleed is visible in preview and saved as separate files during export."
                );
            });
            ui.checkbox(&mut layout_params.enable_bleed, "Enable bleed");
            ui.end_row();

            // Only show bleed amount if enabled
            if layout_params.enable_bleed {
                ui.label("Bleed Amount:");
                ui.add(egui::DragValue::new(&mut layout_params.bleed_mm)
                    .speed(0.1)
                    .range(0.0..=10.0)
                    .suffix(" mm"));
                ui.end_row();
            }

            // Sharpening section
            ui.horizontal(|ui| {
                ui.label("Sharpening:");
                ui.colored_label(ui.visuals().weak_text_color(), "(?)").on_hover_text(
                    "Applies an unsharp mask to all card images.\n\
                     Useful for slightly soft scans or upscaled images.\n\
                     Use the full-resolution preview to judge the effect before exporting."
                );
            });
            ui.checkbox(&mut layout_params.enable_sharpen, "Enable sharpening");
            ui.end_row();

            // Only show sharpen controls if enabled
            if layout_params.enable_sharpen {
                ui.label("Sharpen Amount:");
                ui.add(egui::DragValue::new(&mut layout_params.sharpen_amount)
                    .speed(0.05)
                    .range(0.0..=tcg_layout::sharpen::MAX_SHARPEN_AMOUNT))
                    .on_hover_text(
                        "Strength of the unsharp mask.\n\
                         1.0-1.6 suits 600 DPI card scans; above ~2.0 halos\n\
                         become visible on high-contrast edges."
                    );
                ui.end_row();

                ui.label("Sharpen Radius:");
                ui.add(egui::DragValue::new(&mut layout_params.sharpen_radius)
                    .speed(0.05)
                    .range(
                        tcg_layout::sharpen::MIN_SHARPEN_RADIUS
                            ..=tcg_layout::sharpen::MAX_SHARPEN_RADIUS,
                    )
                    .suffix(" px"))
                    .on_hover_text(
                        "Gaussian radius of the unsharp mask, in pixels at full\n\
                         resolution. Match it to the softness of the source:\n\
                         ~0.7 for 600 DPI scans. Larger radii widen halos\n\
                         without recovering more detail."
                    );
                ui.end_row();

                ui.label("Sharpen Threshold:");
                ui.add(egui::DragValue::new(&mut layout_params.sharpen_threshold)
                    .speed(0.005)
                    .range(0.0..=tcg_layout::sharpen::MAX_SHARPEN_THRESHOLD))
                    .on_hover_text(
                        "Local contrast below this fraction of the tonal range is\n\
                         left alone, which keeps sharpening off flat areas and\n\
                         out of scanner noise. 0.02 suits clean scans; raise it\n\
                         if noise or paper texture gets crunchy."
                    );
                ui.end_row();

                ui.label("");
                ui.colored_label(
                    ui.visuals().weak_text_color(),
                    "Too small to see in the grid preview - use the full-resolution preview.",
                );
                ui.end_row();

                ui.label("");
                if ui
                    .add_enabled(has_cards, egui::Button::new("Preview Full Resolution..."))
                    .on_disabled_hover_text("Import cards to preview sharpening")
                    .clicked()
                {
                    *sharpen_preview_requested = true;
                }
                ui.end_row();
            }

            // Color adjustment section
            ui.horizontal(|ui| {
                ui.label("Color Adjust:");
                ui.colored_label(ui.visuals().weak_text_color(), "(?)").on_hover_text(
                    "Targeted HSL adjustments: shift the hue, saturation, or\n\
                     lightness of specific colors (e.g. certain yellows). Each\n\
                     adjustment can apply to all cards, only fronts, or only backs.\n\
                     Not shown in the page preview — use the editor window to\n\
                     judge the effect. Applied during export."
                );
            });
            ui.checkbox(&mut layout_params.enable_color_adjust, "Enable color adjustments");
            ui.end_row();

            if layout_params.enable_color_adjust {
                ui.label("Adjustments:");
                ui.horizontal(|ui| {
                    let enabled = layout_params
                        .hsl_adjustments
                        .iter()
                        .filter(|a| a.enabled)
                        .count();
                    ui.label(format!(
                        "{enabled} of {} enabled",
                        layout_params.hsl_adjustments.len()
                    ));
                    ui.colored_label(ui.visuals().weak_text_color(), "(not shown in preview)");
                });
                ui.end_row();

                ui.label("");
                if ui
                    .add_enabled(has_cards, egui::Button::new("Edit / Preview..."))
                    .on_disabled_hover_text("Import cards to edit color adjustments")
                    .clicked()
                {
                    *color_adjust_preview_requested = true;
                }
                ui.end_row();
            }

            // Double-sided printing section
            ui.horizontal(|ui| {
                ui.label("Double-sided:");
                ui.colored_label(ui.visuals().weak_text_color(), "(?)").on_hover_text(
                    "Generates a back page after every front page so consecutive\n\
                     pages print front-to-back with duplex printing. Back positions\n\
                     are mirrored for the flip edge; use the back offset to correct\n\
                     printer front/back misalignment."
                );
            });
            ui.checkbox(&mut layout_params.enable_duplex, "Enable double-sided");
            ui.end_row();

            if layout_params.enable_duplex {
                ui.horizontal(|ui| {
                    ui.label("Flip Edge:");
                    ui.colored_label(ui.visuals().weak_text_color(), "(?)").on_hover_text(
                        "Match this to the binding setting in your print dialog\n\
                         (Long-Edge or Short-Edge binding). The flip is about the\n\
                         physical sheet's edge, so the mirror direction depends on\n\
                         page orientation: on portrait pages a long-edge flip mirrors\n\
                         backs left-right; on landscape pages it mirrors them\n\
                         top-bottom (the sheet's long edge is the top/bottom of a\n\
                         landscape layout)."
                    );
                });
                egui::ComboBox::from_id_salt("flip_edge_combo")
                    .selected_text(match layout_params.flip_edge {
                        FlipEdge::LongEdge => "Long edge",
                        FlipEdge::ShortEdge => "Short edge",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut layout_params.flip_edge, FlipEdge::LongEdge, "Long edge");
                        ui.selectable_value(&mut layout_params.flip_edge, FlipEdge::ShortEdge, "Short edge");
                    });
                ui.end_row();

                ui.label("");
                ui.colored_label(
                    ui.visuals().weak_text_color(),
                    if layout_params.back_mirror_is_horizontal() {
                        "Backs mirror left-right (same row, opposite column)"
                    } else {
                        "Backs mirror top-bottom and print rotated 180°\n(cut cards stay head-to-head)"
                    },
                );
                ui.end_row();

                ui.horizontal(|ui| {
                    ui.label("Back Offset:");
                    ui.colored_label(ui.visuals().weak_text_color(), "(?)").on_hover_text(
                        "Shifts back-page content to compensate for printer duplex\n\
                         misregistration. Positive X moves backs toward the sheet's\n\
                         right edge (as printed on the back side); positive Y moves\n\
                         them down. Print a test sheet, hold it up to the light, and\n\
                         adjust in 0.1 mm steps until fronts and backs align."
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("X:");
                    ui.add(egui::DragValue::new(&mut layout_params.back_offset.0)
                        .speed(0.05)
                        .range(-10.0..=10.0)
                        .suffix(" mm"));
                    ui.label("Y:");
                    ui.add(egui::DragValue::new(&mut layout_params.back_offset.1)
                        .speed(0.05)
                        .range(-10.0..=10.0)
                        .suffix(" mm"));
                });
                ui.end_row();

                ui.label("Default Back:");
                ui.horizontal(|ui| {
                    match &layout_params.default_back_path {
                        Some(path) => {
                            let name = path
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| "?".to_string());
                            ui.colored_label(ui.visuals().text_color(), name);
                            if ui.small_button("✕").clicked() {
                                layout_params.default_back_path = None;
                            }
                        }
                        None => {
                            ui.colored_label(ui.visuals().weak_text_color(), "None");
                        }
                    }
                    if ui.small_button("Choose…").clicked() {
                        *default_back_pick_requested = true;
                    }
                });
                ui.end_row();
            }
        });

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(8.0);

    if ui.button("Validate Parameters").clicked() {
        validate_params(
            layout_params,
            validation_errors,
            show_success_message,
            success_message_timer,
        );
    }

    if !validation_errors.is_empty() {
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        ui.strong("Validation Errors:");
        ui.add_space(4.0);

        egui::Frame::none()
            .fill(ui.visuals().error_fg_color.gamma_multiply(0.1))
            .rounding(egui::Rounding::same(4.0))
            .inner_margin(egui::Margin::same(8.0))
            .show(ui, |ui| {
                for error in validation_errors.iter() {
                    ui.horizontal(|ui| {
                        ui.colored_label(ui.visuals().error_fg_color, "•");
                        ui.colored_label(ui.visuals().error_fg_color, error);
                    });
                }
            });
    }

    ui.add_space(8.0);
    let reset_clicked = ui.button("Reset to Defaults").clicked();
    if reset_clicked {
        *layout_params = LayoutParams::default();
        *page_size_option = PageSizeOption::A4;
        *card_size_option = CardSizeOption::Poker;
        validation_errors.clear();
    }

    // Return true if any settings were changed
    reset_clicked
        || *layout_params != original_layout_params
        || *page_size_option != original_page_size_option
        || *card_size_option != original_card_size_option
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

    if layout_params.enable_bleed && layout_params.bleed_mm < 0.0 {
        validation_errors.push("Bleed amount cannot be negative".to_string());
    }

    if layout_params.enable_bleed && layout_params.bleed_mm > 10.0 {
        validation_errors.push("Bleed amount exceeds maximum (10mm)".to_string());
    }

    if layout_params.enable_sharpen && layout_params.sharpen_amount < 0.0 {
        validation_errors.push("Sharpen amount cannot be negative".to_string());
    }

    if layout_params.enable_sharpen
        && layout_params.sharpen_amount > tcg_layout::sharpen::MAX_SHARPEN_AMOUNT
    {
        validation_errors.push(format!(
            "Sharpen amount exceeds maximum ({})",
            tcg_layout::sharpen::MAX_SHARPEN_AMOUNT
        ));
    }

    if layout_params.enable_sharpen
        && !(tcg_layout::sharpen::MIN_SHARPEN_RADIUS..=tcg_layout::sharpen::MAX_SHARPEN_RADIUS)
            .contains(&layout_params.sharpen_radius)
    {
        validation_errors.push(format!(
            "Sharpen radius must be {}-{} px",
            tcg_layout::sharpen::MIN_SHARPEN_RADIUS,
            tcg_layout::sharpen::MAX_SHARPEN_RADIUS
        ));
    }

    if layout_params.enable_sharpen
        && !(0.0..=tcg_layout::sharpen::MAX_SHARPEN_THRESHOLD)
            .contains(&layout_params.sharpen_threshold)
    {
        validation_errors.push(format!(
            "Sharpen threshold must be 0-{}",
            tcg_layout::sharpen::MAX_SHARPEN_THRESHOLD
        ));
    }

    if layout_params.enable_printer_margins {
        let printer = layout_params.printer_margins;
        if printer.top < 0.0 || printer.right < 0.0 || printer.bottom < 0.0 || printer.left < 0.0 {
            validation_errors.push("Printer margins cannot be negative".to_string());
        }

        let (sheet_width, sheet_height) = layout_params.effective_page_size();
        if printer.left + printer.right >= sheet_width
            || printer.top + printer.bottom >= sheet_height
        {
            validation_errors
                .push("Printer margins leave no printable area on the page".to_string());
        }

        let (printable_width, printable_height) = layout_params.printable_size();
        if layout_params.card_size.0 > printable_width
            || layout_params.card_size.1 > printable_height
        {
            validation_errors.push(
                "Card is larger than the printable area left by the printer margins".to_string(),
            );
        }
    }

    if layout_params.enable_duplex
        && (layout_params.back_offset.0.abs() > 10.0 || layout_params.back_offset.1.abs() > 10.0)
    {
        validation_errors.push("Back offset exceeds maximum (±10mm)".to_string());
    }

    if layout_params.enable_color_adjust {
        for (i, adj) in layout_params.hsl_adjustments.iter().enumerate() {
            if !(0.0..=360.0).contains(&adj.target_hue) {
                validation_errors.push(format!(
                    "Color adjustment {}: target hue must be 0-360°",
                    i + 1
                ));
            }
            if adj.hue_range <= 0.0 || adj.hue_range > tcg_layout::color_adjust::MAX_HUE_RANGE {
                validation_errors.push(format!(
                    "Color adjustment {}: hue range must be within 0-{}°",
                    i + 1,
                    tcg_layout::color_adjust::MAX_HUE_RANGE
                ));
            }
            if adj.hue_shift.abs() > tcg_layout::color_adjust::MAX_HUE_SHIFT {
                validation_errors.push(format!(
                    "Color adjustment {}: hue shift must be within ±{}°",
                    i + 1,
                    tcg_layout::color_adjust::MAX_HUE_SHIFT
                ));
            }
            if adj.saturation_shift.abs() > 1.0 || adj.lightness_shift.abs() > 1.0 {
                validation_errors.push(format!(
                    "Color adjustment {}: saturation and lightness shifts must be within ±1.0",
                    i + 1
                ));
            }
        }
    }

    // Calculate effective margins for validation
    let grid = crate::layout::calculate_grid(layout_params);
    let effective_margins = layout_params.effective_margins(&grid);

    let total_margin_width = effective_margins.left + effective_margins.right;
    let total_margin_height = effective_margins.top + effective_margins.bottom;

    // Get page dimensions considering orientation
    let (page_width, page_height) = match layout_params.page_orientation {
        PageOrientation::Portrait => layout_params.page_size,
        PageOrientation::Landscape => (layout_params.page_size.1, layout_params.page_size.0),
    };

    if layout_params.card_size.0 + total_margin_width >= page_width {
        validation_errors.push("Card width plus margins exceeds page width".to_string());
    }

    if layout_params.card_size.1 + total_margin_height >= page_height {
        validation_errors.push("Card height plus margins exceeds page height".to_string());
    }

    if validation_errors.is_empty() {
        *show_success_message = true;
        *success_message_timer = 3.0; // Show for 3 seconds
    }
}
