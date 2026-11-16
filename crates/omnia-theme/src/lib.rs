//! Colour tokens for Omnia (plan section 9).
//!
//! `generated.rs` is written by `cargo xtask tokens` from the `Theme` collection of
//! `omnia.figcraft` and the overlay `canvas-tokens.toml` (plan Q2). Edit those, not it.

/// A colour with straight (not premultiplied) alpha; every channel is in `0.0..=1.0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }
}

mod generated;

pub use generated::{GRAPHITE, GRAY, Theme};
