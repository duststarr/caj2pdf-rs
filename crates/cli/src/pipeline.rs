//! End-to-end conversion pipeline.
//!
//! This is the only place in the workspace that knows about every crate at
//! once. It dispatches on the detected file format and uses the appropriate
//! decoder(s) to produce a final PDF.
//!
//! The architecture avoids a circular dependency: `caj2pdf-pdf` depends on
//! `caj2pdf-core` (for the shared data types), so `caj2pdf-core` cannot
//! also depend on `caj2pdf-pdf`. Putting the full pipeline in the CLI crate
//! breaks the cycle: the CLI depends on all five crates, but no crate
//! depends on the CLI.

use std::path::Path;

use anyhow::{Context, Result};
use tracing::{info, warn};

use caj2pdf_core::{
    convert, CajDocument, CajError, CajResult, DecodedImage, FileFormat, ImageKind,
    Page as CorePage, RawImage,
};
use caj2pdf_jbig1 as jbig1;
use caj2pdf_jbig2 as jbig2;
use caj2pdf_pdf::{self as pdf, PageInput};

/// Convert an opened CAJ-family file to a PDF at `output`.
pub fn run(input: &Path, output: &Path) -> Result<()> {
    info!(file = %input.display(), "opening input");
    let doc = CajDocument::open(input).context("opening input file")?;
    info!(format = %doc.format(), "detected format");

    let result: CajResult<()> = match doc.format() {
        FileFormat::Caj => convert_caj(&doc, output),
        FileFormat::Hn | FileFormat::C8 => convert_hn(&doc, output),
        FileFormat::Pdf => copy_pdf(&doc, output),
        FileFormat::Kdh => convert_kdh(&doc, output),
        FileFormat::Teb => Err(CajError::Unsupported("TEB format is not yet implemented")),
    };
    result.map_err(anyhow::Error::new)
}

// ---------------------------------------------------------------------------
// CAJ: extract embedded PDF, repair xref, inject outlines
// ---------------------------------------------------------------------------

fn convert_caj(doc: &CajDocument, output: &Path) -> CajResult<()> {
    info!("extracting embedded PDF from CAJ container");
    let pdf_bytes = doc.extract_pdf()?;
    info!(bytes = pdf_bytes.len(), "embedded PDF extracted");

    // Load the broken PDF, re-save it (lopdf rebuilds the xref), and add the
    // outline tree. caj2pdf-pdf::inject_outlines does the save+inject in
    // one step.
    let pdf_bytes = pdf::inject_outlines(&pdf_bytes, doc.toc())
        .map_err(|e| CajError::Malformed {
            format: doc.format(),
            message: format!("xref repair / outline injection failed: {e}"),
        })?;
    std::fs::write(output, &pdf_bytes)?;
    info!(file = %output.display(), "wrote repaired PDF with outlines");
    Ok(())
}

// ---------------------------------------------------------------------------
// HN / C8: iterate pages, decode images, build PDF
// ---------------------------------------------------------------------------

fn convert_hn(doc: &CajDocument, output: &Path) -> CajResult<()> {
    info!("iterating pages");
    let pages = doc.pages()?;
    info!(count = pages.len(), "decoded page list");

    let mut page_inputs = Vec::with_capacity(pages.len());
    for (idx, page) in pages.iter().enumerate() {
        info!(page = idx + 1, images = page.images.len(), "decoding page");
        let decoded = decode_page(page)?;
        if decoded.is_empty() {
            warn!(page = idx + 1, "no decodable images on page, skipping");
            continue;
        }
        // Use the first image as the page's primary content; multi-image
        // pages are rare and the original Python project also collapses them
        // to the first image when the layout is ambiguous.
        let image = decoded.into_iter().next().expect("non-empty by check above");
        page_inputs.push(PageInput {
            image,
            text_overlay: if page.text.is_empty() {
                None
            } else {
                Some(page.text.clone())
            },
        });
    }

    if page_inputs.is_empty() {
        return Err(CajError::Malformed {
            format: doc.format(),
            message: "file is pure-text; no images to embed".into(),
        });
    }

    let pdf_bytes = pdf::build_document(&page_inputs, doc.toc())
        .map_err(|e| CajError::Malformed {
            format: doc.format(),
            message: format!("PDF build failed: {e}"),
        })?;
    std::fs::write(output, &pdf_bytes)?;
    info!(file = %output.display(), pages = page_inputs.len(), "wrote PDF");
    Ok(())
}

/// Decode every image on a page, returning the successfully decoded ones.
fn decode_page(page: &CorePage) -> CajResult<Vec<DecodedImage>> {
    let mut out = Vec::with_capacity(page.images.len());
    for raw in &page.images {
        match decode_image(raw) {
            Ok(img) => out.push(img),
            Err(e) => warn!(error = %e, "skipping undecodable image"),
        }
    }
    Ok(out)
}

fn decode_image(raw: &RawImage) -> CajResult<DecodedImage> {
    match raw.kind {
        ImageKind::Jbig1 => {
            let bmp = jbig1::decode(&raw.data, raw.width_px, raw.height_px)
                .map_err(|e| CajError::Malformed {
                    format: FileFormat::Hn,
                    message: format!("JBIG1 decode failed: {e}"),
                })?;
            Ok(DecodedImage::Mono {
                width_px: bmp.width,
                height_px: bmp.height,
                bits: bmp.bits,
            })
        }
        ImageKind::Jbig2 => {
            let bmp = jbig2::decode(&raw.data, raw.width_px, raw.height_px)
                .map_err(|e| CajError::Malformed {
                    format: FileFormat::Hn,
                    message: format!("JBIG2 decode failed: {e}"),
                })?;
            Ok(DecodedImage::Mono {
                width_px: bmp.width,
                height_px: bmp.height,
                bits: bmp.bits,
            })
        }
        ImageKind::Jpeg { upside_down } => {
            // We need the JPEG's actual width/height. The CNKI header (raw
            // 0..48) stores them at offsets 4-11 but in big-endian with
            // negative-height for upside-down images. The 48-byte header
            // is already inside raw.data; the rest of the data is the JPEG
            // stream. We just pass the whole blob through to the PDF layer,
            // which knows how to parse the JPEG SOF marker.
            let height = if upside_down {
                -(raw.height_px as i32) as i32 as u32
            } else {
                raw.height_px
            };
            // Skip the CNKI 48-byte header when handing the JPEG to the
            // PDF writer. The PDF writer parses the SOF marker itself.
            const CNKI_HDR: usize = 48;
            if raw.data.len() < CNKI_HDR {
                return Err(CajError::Malformed {
                    format: FileFormat::Hn,
                    message: format!(
                        "JPEG block is only {} bytes; need at least {}",
                        raw.data.len(),
                        CNKI_HDR
                    ),
                });
            }
            let jpeg_bytes = raw.data[CNKI_HDR..].to_vec();
            Ok(DecodedImage::Jpeg {
                width_px: raw.width_px,
                height_px: height,
                jpeg_bytes,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// PDF and KDH: pass-through and decrypt
// ---------------------------------------------------------------------------

fn copy_pdf(doc: &CajDocument, output: &Path) -> CajResult<()> {
    info!("PDF pass-through, copying file");
    std::fs::copy(doc.path(), output)?;
    Ok(())
}

fn convert_kdh(doc: &CajDocument, output: &Path) -> CajResult<()> {
    info!("decrypting KDH");
    let bytes = std::fs::read(doc.path())?;
    let decrypted = convert::decrypt_kdh(&bytes);
    std::fs::write(output, &decrypted)?;
    info!(file = %output.display(), "wrote decrypted PDF (xref repair TODO)");
    // The original Python runs `mutool clean` after this; we could call
    // lopdf to repair the xref as well, but that requires parsing the
    // decrypted blob. For now we leave that to the user.
    Ok(())
}

// silence unused imports if Page is no longer referenced
#[allow(dead_code)]
fn _force_use_page(_: &CorePage) {}
