//! # caj2pdf-jbig1
//!
//! Pure-Rust port of the **custom** JBIG1 decoder found in CAJViewer's
//! `libreaderex_x64.so`.
//!
//! The reference implementation is `JBigDecode.cc` in the original
//! `caj2pdf` repository; this module mirrors its behavior bit-for-bit so
//! that JBIG1 images extracted from a CAJ/HN file decode identically.
//!
//! ## Why a custom port?
//!
//! The standard `jbig-kit` library assumes a 14-context adaptive template.
//! CNKI's JBIG1 encoder uses only 5 contexts (MPS/ST tables of length 0x20
//! instead of 0x4000), uses 3-bit shifts in `GetBit`, and slightly
//! diverges from the standard in the SLNTP/LNTP predictor. Substituting
//! the standard codec silently produces wrong pixels for real-world files.
//!
//! See `docs/jbig1-reverse-notes.md` for the line-by-line analysis.

#![deny(missing_debug_implementations)]
#![warn(unreachable_pub)]

use thiserror::Error;

/// Errors produced by the JBIG1 decoder.
#[derive(Debug, Error)]
pub enum JbigError {
    #[error("input buffer is too short: need {need} bytes, got {got}")]
    ShortInput { need: usize, got: usize },
    #[error("invalid image header")]
    InvalidHeader,
    #[error("decoder arithmetic error: {0}")]
    Arithmetic(String),
}

pub type JbigResult<T> = std::result::Result<T, JbigError>;

/// The decoded bitmap.
#[derive(Debug, Clone)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    /// One byte per row, MSB-first, padded to the next 8-pixel boundary.
    pub bits: Vec<u8>,
}

impl Bitmap {
    /// Padded width in bytes (8 pixels per byte).
    pub fn row_bytes(&self) -> usize {
        ((self.width as usize) + 7) / 8
    }
}

/// Decode a JBIG1 image. The `width` and `height` must be supplied by the
/// caller because they are stored in a CNKI-private header that this crate
/// does not parse.
pub fn decode(input: &[u8], width: u32, height: u32) -> JbigResult<Bitmap> {
    // Implementation lives in `codec.rs`. This stub is replaced by the
    // JBIG1 implementation agent.
    unimplemented!("JBIG1 decoder stub — see codec.rs (filled in by implementation agent)")
}
