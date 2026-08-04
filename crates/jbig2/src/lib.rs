//! # caj2pdf-jbig2
//!
//! JBIG2 stream decoder for caj2pdf-rs. Uses FFI to the system
//! `libjbig2dec` library (Debian/Ubuntu package `libjbig2dec0-dev`,
//! version 0.19+).
//!
//! At runtime, `jbig2dec.h` provides a single function:
//!
//! ```c
//! int jbig2_decode_generic(
//!     const uint8_t *data, size_t length,
//!     uint8_t *buf, uint32_t width, uint32_t height,
//!     uint32_t row_stride, uint8_t flags);
//! ```
//!
//! We bind it through a small wrapper that handles the system library
//! loading and exposes a safe Rust API.

#![deny(missing_debug_implementations)]
#![warn(unreachable_pub)]

use thiserror::Error;

/// Errors produced by the JBIG2 decoder.
#[derive(Debug, Error)]
pub enum Jbig2Error {
    #[error("libjbig2dec could not be loaded: {0}")]
    Library(String),
    #[error("decoder returned error code {0}")]
    Decode(i32),
    #[error("input buffer is too short: need {need} bytes, got {got}")]
    ShortInput { need: usize, got: usize },
}

pub type Jbig2Result<T> = std::result::Result<T, Jbig2Error>;

/// The decoded bitmap (1 bit per pixel, MSB-first within each byte).
#[derive(Debug, Clone)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub bits: Vec<u8>,
}

impl Bitmap {
    pub fn row_bytes(&self) -> usize {
        ((self.width as usize) + 7) / 8
    }
}

/// Decode a JBIG2 image.
pub fn decode(input: &[u8], width: u32, height: u32) -> Jbig2Result<Bitmap> {
    // Implementation lives in `ffi.rs`. This stub is replaced by the
    // JBIG2 implementation agent.
    unimplemented!("JBIG2 decoder stub — see ffi.rs (filled in by implementation agent)")
}
