use crate::layout::{calculate_cut_marks, calculate_grid, distribute_cards};
use crate::types::{Card, LayoutParams, PageOrientation, ThumbnailState};
use eframe::egui;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Default)]
pub struct PreviewState {
    current_page: usize,
    texture_cache: HashMap<PathBuf, egui::TextureHandle>,
}

impl PreviewState {
    pub fn reset_to_first_page(&mut self) {
        self.current_page = 0;
    }

    pub fn clear_texture_cache(&mut self) {
        self.texture_cache.clear();
    }

    fn get_or_create_texture(
        &mut self,
        ctx: &egui::Context,
        card: &Card,
    ) -> Option<&egui::TextureHandle> {
        if !self.texture_cache.contains_key(&card.path) {
            if let Some(thumbnail) = card.get_thumbnail() {
                let (width, height) = thumbnail.dimensions();
                let pixels: Vec<egui::Color32> = thumbnail
                    .pixels()
                    .map(|pixel| {
                        egui::Color32::from_rgba_unmultiplied(
                            pixel[0], pixel[1], pixel[2], pixel[3],
                        )
                    })
                    .collect();

                let color_image = egui::ColorImage {
                    size: [width as usize, height as usize],
                    pixels,
                };

                let texture = ctx.load_texture(
                    format!("thumbnail_{}", card.path.display()),
                    color_image,
                    egui::TextureOptions::LINEAR,
                );

                self.texture_cache.insert(card.path.clone(), texture);
            }
        }

        self.texture_cache.get(&card.path)
    }
}

pub fn show_preview_panel(
    ui: &mut egui::Ui,
    layout_params: &LayoutParams,
    cards: &[Card],
    preview_state: &mut PreviewState,
) {
    ui.heading("Preview");
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(8.0);

    if cards.is_empty() {
        ui.colored_label(
            ui.visuals().weak_text_color(),
            "No cards selected. Import images to see preview.",
        );
        return;
    }

    let grid = calculate_grid(layout_params);
    let pages = distribute_cards(cards, &grid, layout_params);

    if pages.is_empty() {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "Cannot fit any cards on page with current parameters.",
        );
        return;
    }

    // Ensure current page is valid
    if preview_state.current_page >= pages.len() {
        preview_state.current_page = 0;
    }

    // Page navigation
    ui.horizontal(|ui| {
        if ui.button("◀ Previous").clicked() && preview_state.current_page > 0 {
            preview_state.current_page -= 1;
        }

        ui.colored_label(
            ui.visuals().text_color(),
            format!("Page {} of {}", preview_state.current_page + 1, pages.len()),
        );

        if ui.button("Next ▶").clicked() && preview_state.current_page < pages.len() - 1 {
            preview_state.current_page += 1;
        }
    });

    // Calculate scale to fit preview in available space
    let available_rect = ui.available_rect_before_wrap();
    let available_size = available_rect.size();

    // Get effective page dimensions based on orientation
    let (page_width, page_height) = match layout_params.page_orientation {
        PageOrientation::Portrait => layout_params.page_size,
        PageOrientation::Landscape => (layout_params.page_size.1, layout_params.page_size.0), // Swap width and height
    };

    // Convert page size from mm to pixels for display
    let page_width_px = page_width * 3.0; // Rough conversion for preview
    let page_height_px = page_height * 3.0;

    let scale_x = available_size.x / page_width_px;
    let scale_y = available_size.y / page_height_px;
    let scale = scale_x.min(scale_y).min(1.0); // Don't scale up beyond actual size

    let preview_width = page_width_px * scale;
    let preview_height = page_height_px * scale;

    // Center the preview
    let center_x = available_rect.left() + (available_size.x - preview_width) / 2.0;
    let center_y = available_rect.top() + (available_size.y - preview_height) / 2.0;

    let preview_rect = egui::Rect::from_min_size(
        egui::pos2(center_x, center_y),
        egui::vec2(preview_width, preview_height),
    );

    // Draw page background
    ui.painter().rect_filled(
        preview_rect,
        egui::Rounding::same(4.0),
        egui::Color32::from_rgb(255, 255, 255),
    );

    ui.painter().rect_stroke(
        preview_rect,
        egui::Rounding::same(4.0),
        egui::Stroke::new(2.0, ui.visuals().text_color()),
    );

    // Draw cut marks
    let cut_marks = calculate_cut_marks(layout_params, &grid);
    for cut_mark in &cut_marks {
        let x1 = center_x + (cut_mark.x1 * 3.0 * scale);
        let y1 = center_y + (cut_mark.y1 * 3.0 * scale);
        let x2 = center_x + (cut_mark.x2 * 3.0 * scale);
        let y2 = center_y + (cut_mark.y2 * 3.0 * scale);

        ui.painter().line_segment(
            [egui::pos2(x1, y1), egui::pos2(x2, y2)],
            egui::Stroke::new(1.0, egui::Color32::from_rgb(128, 128, 128)), // Gray cut marks
        );
    }

    // Draw cards for current page
    let current_page = &pages[preview_state.current_page];

    for (card, position) in &current_page.cards {
        // Calculate card dimensions and position based on bleed setting
        let (card_x, card_y, card_width, card_height) =
            if layout_params.enable_bleed && layout_params.bleed_mm > 0.0 {
                // With bleed: draw larger image offset by -bleed_mm to match SVG export
                let bleed_width =
                    (layout_params.card_size.0 + 2.0 * layout_params.bleed_mm) * 3.0 * scale;
                let bleed_height =
                    (layout_params.card_size.1 + 2.0 * layout_params.bleed_mm) * 3.0 * scale;
                let offset_x = center_x + ((position.x - layout_params.bleed_mm) * 3.0 * scale);
                let offset_y = center_y + ((position.y - layout_params.bleed_mm) * 3.0 * scale);
                (offset_x, offset_y, bleed_width, bleed_height)
            } else {
                // Without bleed: draw at card_size
                let x = center_x + (position.x * 3.0 * scale);
                let y = center_y + (position.y * 3.0 * scale);
                let w = layout_params.card_size.0 * 3.0 * scale;
                let h = layout_params.card_size.1 * 3.0 * scale;
                (x, y, w, h)
            };

        let card_rect = egui::Rect::from_min_size(
            egui::pos2(card_x, card_y),
            egui::vec2(card_width, card_height),
        );

        // Draw card based on thumbnail state
        match &card.thumbnail_state {
            ThumbnailState::Loaded(_) => {
                // Try to draw thumbnail
                if let Some(texture) = preview_state.get_or_create_texture(ui.ctx(), card) {
                    // Draw thumbnail
                    ui.painter().image(
                        texture.id(),
                        card_rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), // UV coordinates
                        egui::Color32::WHITE, // Tint
                    );

                    // Draw border around thumbnail
                    ui.painter().rect_stroke(
                        card_rect,
                        egui::Rounding::same(2.0),
                        egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
                    );
                } else {
                    // Fallback if texture creation failed
                    draw_placeholder_card(ui, card_rect, card, ui.visuals().faint_bg_color);
                }
            }
            ThumbnailState::Loading => {
                // Draw loading placeholder with spinner
                draw_loading_card(ui, card_rect, card);
            }
            ThumbnailState::Failed(error) => {
                // Draw error placeholder
                draw_error_card(ui, card_rect, card, error);
            }
            ThumbnailState::NotLoaded => {
                // Draw default placeholder
                draw_placeholder_card(ui, card_rect, card, ui.visuals().faint_bg_color);
            }
        }
    }

    // Show grid info
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(8.0);

    egui::Frame::none()
        .fill(ui.visuals().faint_bg_color)
        .rounding(egui::Rounding::same(4.0))
        .inner_margin(egui::Margin::same(8.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(format!("Grid: {}×{}", grid.cols, grid.rows));
                ui.separator();
                ui.colored_label(
                    ui.visuals().text_color(),
                    format!("Cards per page: {}", grid.cards_per_page),
                );
                ui.separator();
                ui.colored_label(
                    ui.visuals().text_color(),
                    format!(
                        "Total cards: {}",
                        cards.iter().map(|card| card.copy_count).sum::<u32>()
                    ),
                );
            });
        });
}

fn draw_placeholder_card(
    ui: &mut egui::Ui,
    card_rect: egui::Rect,
    card: &Card,
    bg_color: egui::Color32,
) {
    // Draw background
    ui.painter()
        .rect_filled(card_rect, egui::Rounding::same(2.0), bg_color);

    ui.painter().rect_stroke(
        card_rect,
        egui::Rounding::same(2.0),
        egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
    );

    // Draw card filename (if it fits)
    if card_rect.width() > 50.0 && card_rect.height() > 20.0 {
        let filename = card
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("?");

        ui.painter().text(
            card_rect.center(),
            egui::Align2::CENTER_CENTER,
            filename,
            egui::FontId::proportional(10.0),
            ui.visuals().text_color(),
        );
    }
}

fn draw_loading_card(ui: &mut egui::Ui, card_rect: egui::Rect, _card: &Card) {
    // Draw background with different color to indicate loading
    ui.painter().rect_filled(
        card_rect,
        egui::Rounding::same(2.0),
        ui.visuals().code_bg_color, // Loading state
    );

    ui.painter().rect_stroke(
        card_rect,
        egui::Rounding::same(2.0),
        egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
    );

    // Draw loading indicator
    if card_rect.width() > 50.0 && card_rect.height() > 30.0 {
        let center = card_rect.center();

        // Simple rotating dots as loading indicator
        let time = ui.input(|i| i.time) as f32;
        let angle = time * 2.0; // Rotate speed

        for i in 0..3 {
            let dot_angle = angle + (i as f32) * std::f32::consts::PI * 2.0 / 3.0;
            let radius = 8.0;
            let dot_pos = egui::pos2(
                center.x + dot_angle.cos() * radius,
                center.y + dot_angle.sin() * radius,
            );

            ui.painter()
                .circle_filled(dot_pos, 2.0, ui.visuals().text_color());
        }

        // Draw "Loading..." text below
        let text_pos = egui::pos2(center.x, center.y + 15.0);
        ui.painter().text(
            text_pos,
            egui::Align2::CENTER_CENTER,
            "Loading...",
            egui::FontId::proportional(9.0),
            ui.visuals().text_color(),
        );
    }
}

fn draw_error_card(ui: &mut egui::Ui, card_rect: egui::Rect, card: &Card, _error: &str) {
    // Draw background with error color
    ui.painter().rect_filled(
        card_rect,
        egui::Rounding::same(2.0),
        ui.visuals().error_fg_color.gamma_multiply(0.1), // Error state background
    );

    ui.painter().rect_stroke(
        card_rect,
        egui::Rounding::same(2.0),
        egui::Stroke::new(1.0, ui.visuals().error_fg_color),
    );

    // Draw error indicator
    if card_rect.width() > 50.0 && card_rect.height() > 30.0 {
        let center = card_rect.center();

        // Draw X symbol
        let size = 8.0;
        ui.painter().line_segment(
            [
                egui::pos2(center.x - size, center.y - size),
                egui::pos2(center.x + size, center.y + size),
            ],
            egui::Stroke::new(2.0, ui.visuals().error_fg_color),
        );
        ui.painter().line_segment(
            [
                egui::pos2(center.x + size, center.y - size),
                egui::pos2(center.x - size, center.y + size),
            ],
            egui::Stroke::new(2.0, ui.visuals().error_fg_color),
        );

        // Draw filename and error text
        let filename = card
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("?");

        ui.painter().text(
            egui::pos2(center.x, center.y + 15.0),
            egui::Align2::CENTER_CENTER,
            filename,
            egui::FontId::proportional(8.0),
            ui.visuals().text_color(),
        );

        ui.painter().text(
            egui::pos2(center.x, center.y + 25.0),
            egui::Align2::CENTER_CENTER,
            "Error",
            egui::FontId::proportional(8.0),
            ui.visuals().error_fg_color,
        );
    }
}
