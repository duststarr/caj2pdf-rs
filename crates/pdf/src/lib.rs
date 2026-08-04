//! # caj2pdf-pdf
//!
//! PDF document assembly and outline injection for caj2pdf-rs.
//!
//! Two responsibilities:
//!
//! 1. Build a fresh PDF document from a list of decoded page images
//!    (mono bitmaps + JPEG), using the lopdf crate for low-level PDF
//!    object construction.
//! 2. Inject a `/Outlines` tree (table of contents) into an existing
//!    PDF document.

#![deny(missing_debug_implementations)]
#![warn(unreachable_pub)]

use thiserror::Error;
use caj2pdf_core::{CajError, DecodedImage, OutlineEntry};

/// Errors that may occur while assembling a PDF.
#[derive(Debug, Error)]
pub enum PdfError {
    #[error("PDF construction failed: {0}")]
    Lopdf(#[from] lopdf::Error),
    #[error("page assembly error: {0}")]
    Assembly(String),
    #[error("core error: {0}")]
    Core(#[from] CajError),
}

pub type PdfResult<T> = std::result::Result<T, PdfError>;

/// A single page ready to be assembled into a PDF, with the image to embed
/// and any extracted text.
#[derive(Debug, Clone)]
pub struct PageInput {
    pub image: DecodedImage,
    /// Optional plain text overlay (HN-format files only).
    pub text_overlay: Option<String>,
}

/// Build a fresh PDF document from a sequence of page images.
///
/// The returned `Vec<u8>` is a complete, valid PDF file (not a `lopdf::Document`),
/// ready to be written to disk.
pub fn build_document(pages: &[PageInput], outlines: &[OutlineEntry]) -> PdfResult<Vec<u8>> {
    // Implementation lives in `builder.rs`. This stub is replaced by the
    // PDF implementation agent.
    unimplemented!("PDF builder stub — see builder.rs (filled in by implementation agent)")
}

/// Inject an outline tree into an existing PDF.
pub fn inject_outlines(
    existing_pdf: &[u8],
    outlines: &[OutlineEntry],
) -> PdfResult<Vec<u8>> {
    // Implementation lives in `outlines.rs`. This stub is replaced by the
    // PDF implementation agent.
    unimplemented!("Outline injection stub — see outlines.rs (filled in by implementation agent)")
}
