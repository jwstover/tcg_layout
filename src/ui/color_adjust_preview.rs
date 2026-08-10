use eframe::egui;
use image::{ImageBuffer, Rgba};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use tcg_layout::color_adjust::{self, AdjustmentScope, HslAdjustment};

/// Maximum dimension for the preview image. Full-resolution scans can exceed
/// GPU texture limits, so we cap the longest edge while staying far above
/// thumbnail fidelity.
const MAX_PREVIEW_DIMENSION: u32 = 2048;

/// One image the editor can page through: a card front or a back image
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewEntry {
    pub path: PathBuf,
    pub is_back: bool,
}

#[derive(Debug)]
enum ColorAdjustPreviewMessage {
    ImageLoaded {
        generation: u64,
        original: Arc<ImageBuffer<Rgba<u8>, Vec<u8>>>,
    },
    ImageLoadFailed {
        generation: u64,
        error: String,
    },
    Processed {
        generation: u64,
        adjustments: Vec<HslAdjustment>,
        image: ImageBuffer<Rgba<u8>, Vec<u8>>,
    },
}

/// Result of showing the color adjust preview window for one frame
pub enum ColorAdjustPreviewAction {
    None,
    /// User clicked "Apply to all cards" with this adjustment list
    Apply(Vec<HslAdjustment>),
}

/// State for the full-resolution color adjustment preview window.
///
/// Pages through a list of entries (card fronts, then backs), loading the
/// current image (downscaled to MAX_PREVIEW_DIMENSION if needed) and
/// re-processing it on a background task whenever the adjustment list
/// changes. Only adjustments whose scope covers the current entry's side are
/// applied, so backs-only adjustments preview correctly. At most one
/// processing task runs at a time; if the controls move while one is in
/// flight, the latest adjustments are processed when the task completes.
/// `generation` increments on every navigation so results from a previous
/// image arriving late are discarded. The dropper samples colors from the
/// original buffer to seed an adjustment's target hue.
#[derive(Default)]
pub struct ColorAdjustPreviewState {
    open: bool,
    entries: Vec<PreviewEntry>,
    current_index: usize,
    generation: u64,
    pub pending_adjustments: Vec<HslAdjustment>,
    original: Option<Arc<ImageBuffer<Rgba<u8>, Vec<u8>>>>,
    original_texture: Option<egui::TextureHandle>,
    processed_texture: Option<egui::TextureHandle>,
    /// Adjustments of the currently displayed processed texture
    displayed_adjustments: Option<Vec<HslAdjustment>>,
    process_in_flight: bool,
    is_loading: bool,
    error: Option<String>,
    zoom: f32,
    /// Index of the adjustment row waiting for a dropper sample
    dropper_row: Option<usize>,
    sender: Option<mpsc::Sender<ColorAdjustPreviewMessage>>,
    receiver: Option<mpsc::Receiver<ColorAdjustPreviewMessage>>,
}

impl ColorAdjustPreviewState {
    pub fn is_open(&self) -> bool {
        self.open
    }

    fn current_entry(&self) -> Option<&PreviewEntry> {
        self.entries.get(self.current_index)
    }

    /// Open the preview window for a set of images and start loading the first
    pub fn open(&mut self, entries: Vec<PreviewEntry>, initial_adjustments: Vec<HslAdjustment>) {
        if entries.is_empty() {
            return;
        }

        *self = Self {
            open: true,
            entries,
            pending_adjustments: initial_adjustments,
            zoom: 1.0,
            ..Self::default()
        };

        let (sender, receiver) = mpsc::channel();
        self.sender = Some(sender);
        self.receiver = Some(receiver);

        self.start_load();
    }

    /// Move to another entry, wrapping around at either end
    fn navigate(&mut self, delta: isize) {
        let len = self.entries.len() as isize;
        if len <= 1 {
            return;
        }
        self.current_index = (self.current_index as isize + delta).rem_euclid(len) as usize;
        self.start_load();
    }

    /// Start loading the current entry's image, discarding any state and
    /// in-flight results from the previous entry
    fn start_load(&mut self) {
        let (Some(entry), Some(sender)) = (self.current_entry(), self.sender.clone()) else {
            return;
        };
        let path = entry.path.clone();

        self.generation += 1;
        let generation = self.generation;
        self.is_loading = true;
        self.error = None;
        self.original = None;
        self.original_texture = None;
        self.processed_texture = None;
        self.displayed_adjustments = None;
        self.process_in_flight = false;

        tokio::task::spawn_blocking(move || {
            let message = match image::open(&path) {
                Ok(img) => {
                    // Cap the preview size to stay within GPU texture limits
                    let img = if img.width().max(img.height()) > MAX_PREVIEW_DIMENSION {
                        img.resize(
                            MAX_PREVIEW_DIMENSION,
                            MAX_PREVIEW_DIMENSION,
                            image::imageops::FilterType::Lanczos3,
                        )
                    } else {
                        img
                    };
                    ColorAdjustPreviewMessage::ImageLoaded {
                        generation,
                        original: Arc::new(img.to_rgba8()),
                    }
                }
                Err(e) => ColorAdjustPreviewMessage::ImageLoadFailed {
                    generation,
                    error: e.to_string(),
                },
            };
            let _ = sender.send(message);
        });
    }

    fn close(&mut self) {
        *self = Self::default();
    }

    fn has_pending_work(&self) -> bool {
        self.is_loading
            || self.process_in_flight
            || self.displayed_adjustments.as_ref() != Some(&self.pending_adjustments)
    }

    /// Kick off a background processing pass for the current adjustments,
    /// unless one is already running or the result is already displayed
    fn request_process_if_needed(&mut self) {
        if self.process_in_flight
            || self.displayed_adjustments.as_ref() == Some(&self.pending_adjustments)
        {
            return;
        }

        let (Some(original), Some(sender)) = (self.original.clone(), self.sender.clone()) else {
            return;
        };

        let adjustments = self.pending_adjustments.clone();
        let is_back = self.current_entry().is_some_and(|e| e.is_back);
        let generation = self.generation;
        self.process_in_flight = true;

        tokio::task::spawn_blocking(move || {
            // Only adjustments scoped to this entry's side take effect; the
            // full list is echoed back for the displayed-state comparison
            let side_adjustments = color_adjust::adjustments_for_side(&adjustments, is_back);
            let image = color_adjust::apply_hsl_adjustments(&original, &side_adjustments);
            let _ = sender.send(ColorAdjustPreviewMessage::Processed {
                generation,
                adjustments,
                image,
            });
        });
    }

    fn process_messages(&mut self, ctx: &egui::Context) {
        let Some(receiver) = &self.receiver else {
            return;
        };

        let mut messages = Vec::new();
        while let Ok(message) = receiver.try_recv() {
            messages.push(message);
        }

        for message in messages {
            match message {
                ColorAdjustPreviewMessage::ImageLoaded {
                    generation,
                    original,
                } => {
                    // Results for a previously displayed entry arrive late
                    // after navigation; drop them
                    if generation != self.generation {
                        continue;
                    }
                    self.is_loading = false;
                    self.original_texture = Some(load_texture(
                        ctx,
                        "color_adjust_preview_original",
                        &original,
                    ));
                    self.original = Some(original);
                    self.request_process_if_needed();
                }
                ColorAdjustPreviewMessage::ImageLoadFailed { generation, error } => {
                    if generation != self.generation {
                        continue;
                    }
                    self.is_loading = false;
                    self.error = Some(error);
                }
                ColorAdjustPreviewMessage::Processed {
                    generation,
                    adjustments,
                    image,
                } => {
                    if generation != self.generation {
                        continue;
                    }
                    self.process_in_flight = false;
                    self.processed_texture =
                        Some(load_texture(ctx, "color_adjust_preview_processed", &image));
                    self.displayed_adjustments = Some(adjustments);
                    // Controls may have moved while this pass was running
                    self.request_process_if_needed();
                }
            }
        }
    }

    /// Apply a dropper sample to the armed adjustment row
    fn apply_sample(&mut self, pixel_x: u32, pixel_y: u32) {
        let (Some(row), Some(original)) = (self.dropper_row, self.original.as_ref()) else {
            return;
        };

        if let Some(sample) =
            color_adjust::sample_region(original, pixel_x, pixel_y, color_adjust::SAMPLE_RADIUS)
        {
            if let Some(adj) = self.pending_adjustments.get_mut(row) {
                adj.target_hue = sample.hue;
                adj.hue_range = sample.suggested_range;
                adj.feather = sample.suggested_range * 0.5;
            }
        }

        self.dropper_row = None;
    }
}

fn load_texture(
    ctx: &egui::Context,
    name: &str,
    image: &ImageBuffer<Rgba<u8>, Vec<u8>>,
) -> egui::TextureHandle {
    let (width, height) = image.dimensions();
    let color_image =
        egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], image.as_raw());
    ctx.load_texture(name, color_image, egui::TextureOptions::LINEAR)
}

fn hue_swatch_color(hue: f32) -> egui::Color32 {
    // Fixed saturation/lightness so the swatch reads as "which hue"
    let (r, g, b) = color_adjust::hsl_to_rgb(hue, 0.75, 0.5);
    egui::Color32::from_rgb(r, g, b)
}

fn draw_swatch(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::Rounding::same(3.0), color);
    ui.painter().rect_stroke(
        rect,
        egui::Rounding::same(3.0),
        ui.visuals().widgets.noninteractive.bg_stroke,
    );
}

/// Show one adjustment's controls. Returns true if the row should be removed.
fn show_adjustment_row(
    ui: &mut egui::Ui,
    index: usize,
    adj: &mut HslAdjustment,
    dropper_row: &mut Option<usize>,
) -> bool {
    let mut remove = false;

    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.checkbox(&mut adj.enabled, "")
                .on_hover_text("Temporarily disable this adjustment without deleting it");
            if !adj.enabled && *dropper_row == Some(index) {
                // Disarm the dropper if its row was just disabled
                *dropper_row = None;
            }

            ui.add_enabled_ui(adj.enabled, |ui| {
                // Target hue swatch, then the shifted result
                draw_swatch(ui, hue_swatch_color(adj.target_hue));
                ui.label("→");
                draw_swatch(ui, hue_swatch_color(adj.target_hue + adj.hue_shift));

                ui.add_space(4.0);
                let armed = *dropper_row == Some(index);
                if ui
                    .selectable_label(armed, "🎨 Pick")
                    .on_hover_text("Click the image below to sample the target color")
                    .clicked()
                {
                    *dropper_row = if armed { None } else { Some(index) };
                }

                ui.add_space(4.0);
                ui.label("Hue:");
                ui.add(
                    egui::DragValue::new(&mut adj.target_hue)
                        .speed(1.0)
                        .range(0.0..=360.0)
                        .suffix("°"),
                );
                ui.label("Range:");
                ui.add(
                    egui::DragValue::new(&mut adj.hue_range)
                        .speed(0.5)
                        .range(1.0..=color_adjust::MAX_HUE_RANGE)
                        .suffix("°"),
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("✕").on_hover_text("Remove adjustment").clicked() {
                    remove = true;
                }
            });
        });

        ui.add_enabled_ui(adj.enabled, |ui| {
            ui.horizontal(|ui| {
                ui.label("Shift:");
                ui.add(
                    egui::Slider::new(
                        &mut adj.hue_shift,
                        -color_adjust::MAX_HUE_SHIFT..=color_adjust::MAX_HUE_SHIFT,
                    )
                    .fixed_decimals(0)
                    .suffix("°"),
                );
                ui.label("Sat:");
                ui.add(egui::Slider::new(&mut adj.saturation_shift, -1.0..=1.0).fixed_decimals(2));
                ui.label("Light:");
                ui.add(egui::Slider::new(&mut adj.lightness_shift, -1.0..=1.0).fixed_decimals(2));
            });

            ui.horizontal(|ui| {
                ui.label("Apply to:");
                egui::ComboBox::from_id_salt(("color_adjust_scope", index))
                    .selected_text(adj.scope.display_name())
                    .show_ui(ui, |ui| {
                        for scope in AdjustmentScope::ALL {
                            ui.selectable_value(&mut adj.scope, scope, scope.display_name());
                        }
                    })
                    .response
                    .on_hover_text(
                        "Limit this adjustment to card fronts or card backs\n\
                         (backs are the images on duplex back pages)",
                    );
            });
        });
    });

    remove
}

/// Show the color adjust preview window. Returns the action the user took.
pub fn show_color_adjust_preview_window(
    ctx: &egui::Context,
    state: &mut ColorAdjustPreviewState,
) -> ColorAdjustPreviewAction {
    if !state.open {
        return ColorAdjustPreviewAction::None;
    }

    state.process_messages(ctx);

    let mut action = ColorAdjustPreviewAction::None;
    let mut window_open = true;
    let mut close_requested = false;

    egui::Window::new("Color Adjustments")
        .open(&mut window_open)
        .resizable(true)
        .default_size([760.0, 860.0])
        .show(ctx, |ui| {
            // Navigation across all fronts and backs
            ui.horizontal(|ui| {
                let count = state.entries.len();
                if ui
                    .add_enabled(count > 1, egui::Button::new("◀"))
                    .on_hover_text("Previous image")
                    .clicked()
                {
                    state.navigate(-1);
                }
                if ui
                    .add_enabled(count > 1, egui::Button::new("▶"))
                    .on_hover_text("Next image")
                    .clicked()
                {
                    state.navigate(1);
                }
                if let Some(entry) = state.current_entry() {
                    let filename = entry
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "?".to_string());
                    let side = if entry.is_back { "Back" } else { "Front" };
                    ui.label(format!(
                        "{filename} — {side} ({} of {count})",
                        state.current_index + 1
                    ));
                }
            });

            ui.add_space(4.0);

            // Adjustment rows
            let mut row_to_remove = None;
            for index in 0..state.pending_adjustments.len() {
                // Split the borrow: the row needs &mut adjustment and &mut dropper_row
                let mut dropper_row = state.dropper_row;
                let adj = &mut state.pending_adjustments[index];
                if show_adjustment_row(ui, index, adj, &mut dropper_row) {
                    row_to_remove = Some(index);
                }
                state.dropper_row = dropper_row;
            }

            if let Some(index) = row_to_remove {
                state.pending_adjustments.remove(index);
                // Keep the dropper pointing at the same adjustment, or disarm
                // if the armed row was removed
                state.dropper_row = match state.dropper_row {
                    Some(armed) if armed == index => None,
                    Some(armed) if armed > index => Some(armed - 1),
                    other => other,
                };
            }

            if ui.button("＋ Add adjustment").clicked() {
                state.pending_adjustments.push(HslAdjustment::default());
                // Arm the dropper so the natural next step is clicking the
                // color to target
                state.dropper_row = Some(state.pending_adjustments.len() - 1);
            }

            ui.add_space(4.0);

            // Controls row
            let mut compare_held = false;
            ui.horizontal(|ui| {
                let compare_response = ui.button("Hold to compare");
                compare_held = compare_response.is_pointer_button_down_on();

                ui.separator();

                ui.label("Zoom:");
                ui.add(
                    egui::Slider::new(&mut state.zoom, 0.25..=2.0)
                        .fixed_decimals(2)
                        .logarithmic(true),
                );
            });

            state.request_process_if_needed();

            // Status row
            ui.horizontal(|ui| {
                if state.is_loading {
                    ui.spinner();
                    ui.label("Loading full-resolution image...");
                } else if state.dropper_row.is_some() {
                    ui.label("🎨 Click the image to sample the target color");
                } else if state.process_in_flight
                    || state.displayed_adjustments.as_ref() != Some(&state.pending_adjustments)
                {
                    ui.spinner();
                    ui.label("Applying adjustments...");
                } else if compare_held {
                    ui.label("Showing original");
                } else if state.displayed_adjustments.is_some() {
                    let is_back = state.current_entry().is_some_and(|e| e.is_back);
                    let applicable = state
                        .pending_adjustments
                        .iter()
                        .filter(|a| a.enabled && a.scope.applies_to(is_back))
                        .count();
                    ui.label(format!(
                        "Showing adjusted ({applicable} of {} apply to this image)",
                        state.pending_adjustments.len()
                    ));
                }
            });

            if let Some(error) = &state.error {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Failed to load image: {error}"),
                );
            }

            ui.add_space(4.0);
            ui.separator();

            // Image area: show original while comparing, otherwise the
            // latest processed result (falling back to the original until
            // the first processing pass completes)
            let texture = if compare_held {
                state.original_texture.as_ref()
            } else {
                state
                    .processed_texture
                    .as_ref()
                    .or(state.original_texture.as_ref())
            };

            if let Some(texture) = texture {
                let size = texture.size_vec2() * state.zoom;
                let texture_id = texture.id();
                egui::ScrollArea::both()
                    .max_height(ui.available_height() - 40.0)
                    .show(ui, |ui| {
                        let dropper_armed = state.dropper_row.is_some();
                        let mut response = ui
                            .add(egui::Image::new((texture_id, size)).sense(egui::Sense::click()));
                        if dropper_armed {
                            response = response.on_hover_cursor(egui::CursorIcon::Crosshair);

                            // Live swatch of the color under the cursor
                            if let (Some(pos), Some(original)) =
                                (response.hover_pos(), state.original.as_ref())
                            {
                                let rel = (pos - response.rect.min) / state.zoom;
                                if rel.x >= 0.0 && rel.y >= 0.0 {
                                    if let Some(sample) = color_adjust::sample_region(
                                        original,
                                        rel.x as u32,
                                        rel.y as u32,
                                        color_adjust::SAMPLE_RADIUS,
                                    ) {
                                        let (r, g, b) = color_adjust::hsl_to_rgb(
                                            sample.hue,
                                            sample.saturation,
                                            sample.lightness,
                                        );
                                        let swatch_rect = egui::Rect::from_min_size(
                                            pos + egui::vec2(14.0, 14.0),
                                            egui::vec2(20.0, 20.0),
                                        );
                                        let painter = ui.painter();
                                        painter.rect_filled(
                                            swatch_rect,
                                            egui::Rounding::same(3.0),
                                            egui::Color32::from_rgb(r, g, b),
                                        );
                                        painter.rect_stroke(
                                            swatch_rect,
                                            egui::Rounding::same(3.0),
                                            egui::Stroke::new(1.0, egui::Color32::WHITE),
                                        );
                                    }
                                }
                            }

                            if response.clicked() {
                                if let Some(pos) = response.interact_pointer_pos() {
                                    let rel = (pos - response.rect.min) / state.zoom;
                                    if rel.x >= 0.0 && rel.y >= 0.0 {
                                        state.apply_sample(rel.x as u32, rel.y as u32);
                                    }
                                }
                            }
                        }
                    });
            }

            ui.add_space(4.0);
            ui.separator();

            ui.horizontal(|ui| {
                let apply_enabled = !state.is_loading && state.error.is_none();
                if ui
                    .add_enabled(apply_enabled, egui::Button::new("Apply adjustments"))
                    .on_hover_text(
                        "Save these adjustments to the layout settings;\n\
                         each adjustment affects the cards its scope covers",
                    )
                    .clicked()
                {
                    action = ColorAdjustPreviewAction::Apply(state.pending_adjustments.clone());
                    close_requested = true;
                }
                if ui.button("Cancel").clicked() {
                    close_requested = true;
                }
            });
        });

    // Keep the UI updating while background work is pending
    if state.has_pending_work() {
        ctx.request_repaint();
    }

    if !window_open || close_requested {
        state.close();
    }

    action
}
