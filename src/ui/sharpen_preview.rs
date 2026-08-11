use eframe::egui;
use image::{ImageBuffer, Rgba};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use tcg_layout::sharpen;

/// Maximum dimension for the preview image. Full-resolution scans can exceed
/// GPU texture limits, so we cap the longest edge while staying far above
/// thumbnail fidelity.
const MAX_PREVIEW_DIMENSION: u32 = 2048;

#[derive(Debug)]
enum SharpenPreviewMessage {
    ImageLoaded {
        original: Arc<ImageBuffer<Rgba<u8>, Vec<u8>>>,
    },
    ImageLoadFailed {
        error: String,
    },
    Sharpened {
        params: sharpen::SharpenParams,
        image: ImageBuffer<Rgba<u8>, Vec<u8>>,
    },
}

/// Result of showing the sharpen preview window for one frame
pub enum SharpenPreviewAction {
    None,
    /// User clicked "Apply to all cards" with these sharpen settings
    Apply(sharpen::SharpenParams),
}

/// State for the full-resolution sharpen preview window.
///
/// Loads the card image once (downscaled to MAX_PREVIEW_DIMENSION if needed),
/// then re-sharpens it on a background task whenever the amount slider moves.
/// At most one sharpen task runs at a time; if the slider moves while one is
/// in flight, the latest amount is processed when the task completes.
#[derive(Default)]
pub struct SharpenPreviewState {
    open: bool,
    card_path: Option<PathBuf>,
    pub pending: sharpen::SharpenParams,
    original: Option<Arc<ImageBuffer<Rgba<u8>, Vec<u8>>>>,
    original_texture: Option<egui::TextureHandle>,
    sharpened_texture: Option<egui::TextureHandle>,
    /// Settings the currently displayed sharpened texture was built with
    displayed: Option<sharpen::SharpenParams>,
    sharpen_in_flight: bool,
    is_loading: bool,
    error: Option<String>,
    zoom: f32,
    sender: Option<mpsc::Sender<SharpenPreviewMessage>>,
    receiver: Option<mpsc::Receiver<SharpenPreviewMessage>>,
}

impl SharpenPreviewState {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Open the preview window for a card and start loading its image
    pub fn open(&mut self, card_path: PathBuf, initial: sharpen::SharpenParams) {
        *self = Self {
            open: true,
            card_path: Some(card_path.clone()),
            pending: initial,
            zoom: 1.0,
            is_loading: true,
            ..Self::default()
        };

        let (sender, receiver) = mpsc::channel();
        self.sender = Some(sender.clone());
        self.receiver = Some(receiver);

        tokio::task::spawn_blocking(move || {
            let message = match image::open(&card_path) {
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
                    SharpenPreviewMessage::ImageLoaded {
                        original: Arc::new(img.to_rgba8()),
                    }
                }
                Err(e) => SharpenPreviewMessage::ImageLoadFailed {
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
        self.is_loading || self.sharpen_in_flight || self.displayed != Some(self.pending)
    }

    /// Kick off a background sharpen pass for the current pending amount,
    /// unless one is already running or the result is already displayed
    fn request_sharpen_if_needed(&mut self) {
        if self.sharpen_in_flight || self.displayed == Some(self.pending) {
            return;
        }

        let (Some(original), Some(sender)) = (self.original.clone(), self.sender.clone()) else {
            return;
        };

        let params = self.pending;
        self.sharpen_in_flight = true;

        tokio::task::spawn_blocking(move || {
            let image = sharpen::apply_sharpen_to_buffer(&original, &params);
            let _ = sender.send(SharpenPreviewMessage::Sharpened { params, image });
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
                SharpenPreviewMessage::ImageLoaded { original } => {
                    self.is_loading = false;
                    self.original_texture =
                        Some(load_texture(ctx, "sharpen_preview_original", &original));
                    self.original = Some(original);
                    self.request_sharpen_if_needed();
                }
                SharpenPreviewMessage::ImageLoadFailed { error } => {
                    self.is_loading = false;
                    self.error = Some(error);
                }
                SharpenPreviewMessage::Sharpened { params, image } => {
                    self.sharpen_in_flight = false;
                    self.sharpened_texture =
                        Some(load_texture(ctx, "sharpen_preview_sharpened", &image));
                    self.displayed = Some(params);
                    // Slider may have moved while this pass was running
                    self.request_sharpen_if_needed();
                }
            }
        }
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

/// Show the sharpen preview window. Returns the action the user took.
pub fn show_sharpen_preview_window(
    ctx: &egui::Context,
    state: &mut SharpenPreviewState,
) -> SharpenPreviewAction {
    if !state.open {
        return SharpenPreviewAction::None;
    }

    state.process_messages(ctx);

    let mut action = SharpenPreviewAction::None;
    let mut window_open = true;
    let mut close_requested = false;

    egui::Window::new("Sharpen Preview")
        .open(&mut window_open)
        .resizable(true)
        .default_size([700.0, 800.0])
        .show(ctx, |ui| {
            if let Some(path) = &state.card_path {
                let filename = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "?".to_string());
                ui.label(format!("Card: {filename}"));
            }

            ui.add_space(4.0);

            // Controls row
            let mut compare_held = false;
            ui.horizontal(|ui| {
                ui.label("Amount:");
                ui.add(
                    egui::Slider::new(&mut state.pending.amount, 0.0..=sharpen::MAX_SHARPEN_AMOUNT)
                        .fixed_decimals(2),
                );

                ui.label("Radius:");
                ui.add(
                    egui::Slider::new(
                        &mut state.pending.radius,
                        sharpen::MIN_SHARPEN_RADIUS..=sharpen::MAX_SHARPEN_RADIUS,
                    )
                    .fixed_decimals(2)
                    .suffix(" px"),
                );

                ui.label("Threshold:");
                ui.add(
                    egui::Slider::new(
                        &mut state.pending.threshold,
                        0.0..=sharpen::MAX_SHARPEN_THRESHOLD,
                    )
                    .fixed_decimals(3),
                );

                ui.separator();

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

            state.request_sharpen_if_needed();

            // Status row
            ui.horizontal(|ui| {
                if state.is_loading {
                    ui.spinner();
                    ui.label("Loading full-resolution image...");
                } else if state.sharpen_in_flight || state.displayed != Some(state.pending) {
                    ui.spinner();
                    ui.label("Sharpening...");
                } else if compare_held {
                    ui.label("Showing original");
                } else if let Some(shown) = state.displayed {
                    ui.label(format!(
                        "Showing sharpened (amount {:.2}, radius {:.2} px, threshold {:.3})",
                        shown.amount, shown.radius, shown.threshold
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
            // latest sharpened result (falling back to the original until
            // the first sharpen pass completes)
            let texture = if compare_held {
                state.original_texture.as_ref()
            } else {
                state
                    .sharpened_texture
                    .as_ref()
                    .or(state.original_texture.as_ref())
            };

            if let Some(texture) = texture {
                let size = texture.size_vec2() * state.zoom;
                egui::ScrollArea::both()
                    .max_height(ui.available_height() - 40.0)
                    .show(ui, |ui| {
                        ui.add(egui::Image::new((texture.id(), size)));
                    });
            }

            ui.add_space(4.0);
            ui.separator();

            ui.horizontal(|ui| {
                let apply_enabled = !state.is_loading && state.error.is_none();
                if ui
                    .add_enabled(apply_enabled, egui::Button::new("Apply to all cards"))
                    .clicked()
                {
                    action = SharpenPreviewAction::Apply(state.pending);
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
