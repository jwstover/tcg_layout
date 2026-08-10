#![warn(clippy::all, rust_2018_idioms)]

extern crate self as tcg_layout;

pub mod bleed;
pub mod color_adjust;
pub mod image;
pub mod image_processing;
pub mod layout;
pub mod pdf_export;
pub mod sharpen;
pub mod svg_export;
pub mod thumbnail_manager;
pub mod types;
