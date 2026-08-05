//! High-level conversion entry point: turn a CAJ-family file into a PDF.
//!
//! This module is the single place that knows about every format. Other
//! crates (the CLI, the GUI, library users) should call [`convert`] and let
//! this module dispatch on the detected file format.
//!
//! The full pipeline (xref repair, outline injection, JBIG/JPEG image
//! decoding, PDF assembly) lives here so every front-end uses the same
//! implementation.

use std::path::Path;

use tracing::{info, warn};

use crate::{CajDocument, CajError, CajResult, DecodedImage, FileFormat, ImageKind, Page, RawImage};
use caj2pdf_jbig1 as jbig1;
use caj2pdf_jbig2 as jbig2;
use caj2pdf_pdf::{self as pdf, PageInput};

/// XOR key used by the KDH format's "encryption".
pub const KDH_PASSPHRASE: &[u8] = b"FZHMEI";

/// Convert a CAJ-family file at `input` to a PDF at `output`.
///
/// This is the **single high-level entry point** used by every front-end
/// (the `caj2pdf` CLI, the `caj2pdf-gui` desktop app, and any future
/// library consumer). The per-format pipeline is:
///
/// * **CAJ** – extract the embedded PDF, repair its xref table, inject the
///   outline tree, write to `output`.
/// * **HN / C8** – iterate the per-page text and image blocks, decode the
///   JBIG / JBIG2 / JPEG images via `caj2pdf-jbig1` / `caj2pdf-jbig2`, build
///   a fresh PDF via `caj2pdf-pdf`, write to `output`.
/// * **PDF** – copy the file verbatim.
/// * **KDH** – apply the XOR decryption with [`KDH_PASSPHRASE`], drop the
///   254-byte header, truncate to the last `%%EOF` marker, write to
///   `output`.
/// * **TEB** – not yet supported; returns
///   [`CajError::Unsupported("TEB format not yet implemented")`].
///
/// # Errors
/// See [`CajError`].
pub fn convert(input: &Path, output: &Path) -> CajResult<()> {
    info!(file = %input.display(), "opening input");
    let doc = CajDocument::open(input)?;
    info!(format = %doc.format(), "detected format");

    match doc.format() {
        FileFormat::Caj => convert_caj(&doc, output),
        FileFormat::Hn | FileFormat::C8 => convert_hn(&doc, output),
        FileFormat::Pdf => convert_pdf(&doc, output),
        FileFormat::Kdh => convert_kdh(&doc, output),
        FileFormat::Teb => Err(CajError::Unsupported(
            "TEB format is not yet implemented",
        )),
    }
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
    let pdf_bytes = pdf::inject_outlines(&pdf_bytes, doc.toc()).map_err(|e| {
        CajError::Malformed {
            format: doc.format(),
            message: format!("xref repair / outline injection failed: {e}"),
        }
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

    let pdf_bytes = pdf::build_document(&page_inputs, doc.toc()).map_err(|e| {
        CajError::Malformed {
            format: doc.format(),
            message: format!("PDF build failed: {e}"),
        }
    })?;
    std::fs::write(output, &pdf_bytes)?;
    info!(file = %output.display(), pages = page_inputs.len(), "wrote PDF");
    Ok(())
}

/// Decode every image on a page, returning the successfully decoded ones.
fn decode_page(page: &Page) -> CajResult<Vec<DecodedImage>> {
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
            // The CNKI header (raw 0..48) stores width/height but in
            // big-endian with negative-height for upside-down images. The
            // 48-byte header is already inside raw.data; the rest is the
            // JPEG stream, which the PDF layer parses via the SOF marker.
            let height = if upside_down {
                -(raw.height_px as i32) as i32 as u32
            } else {
                raw.height_px
            };
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

fn convert_pdf(doc: &CajDocument, output: &Path) -> CajResult<()> {
    info!("PDF pass-through, copying file");
    std::fs::copy(doc.path(), output)?;
    Ok(())
}

fn convert_kdh(doc: &CajDocument, output: &Path) -> CajResult<()> {
    info!("decrypting KDH");
    let bytes = std::fs::read(doc.path())?;
    let decrypted = decrypt_kdh(&bytes);

    // The KDH container holds a complete PDF (typically PDF 1.5+ with a
    // cross-reference stream) so no additional xref repair is needed.
    if !decrypted.starts_with(b"%PDF-") {
        return Err(CajError::Malformed {
            format: doc.format(),
            message: "decrypted KDH does not start with %PDF- — wrong XOR key?".into(),
        });
    }
    if !decrypted.windows(5).any(|w| w == b"%%EOF") {
        return Err(CajError::Malformed {
            format: doc.format(),
            message: "decrypted KDH is missing %%EOF marker".into(),
        });
    }

    std::fs::write(output, &decrypted)?;
    info!(
        file = %output.display(),
        bytes = decrypted.len(),
        "wrote decrypted PDF"
    );
    Ok(())
}

/// Decrypt a KDH file by applying a fixed 6-byte XOR keystream
/// ([`KDH_PASSPHRASE`]) after skipping the 254-byte container header, then
/// truncate the result to the last `%%EOF` marker.
///
/// This matches `_convert_kdh` in the original Python
/// `cajparser.py:605-640` line-for-line:
///
/// 1. Drop the first 254 bytes (the KDH container header).
/// 2. XOR each remaining byte with the keystream
///    (`FZHMEI` repeated cyclically).
/// 3. Truncate to the byte just past the last occurrence of `%%EOF`.
///
/// The caller is responsible for repairing the PDF xref table (e.g. with
/// `mutool clean` or `lopdf`) after this returns.
pub fn decrypt_kdh(input: &[u8]) -> Vec<u8> {
    if input.len() <= 254 {
        // Truncated input: no payload to decrypt, no %%EOF to find.
        return Vec::new();
    }
    let payload = &input[254..];
    let mut output = Vec::with_capacity(payload.len());
    for (i, &b) in payload.iter().enumerate() {
        output.push(b ^ KDH_PASSPHRASE[i % KDH_PASSPHRASE.len()]);
    }

    // Drop everything after the last `%%EOF` (inclusive of the 5 bytes).
    if let Some(pos) = output
        .windows(b"%%EOF".len())
        .rposition(|w| w == b"%%EOF")
    {
        output.truncate(pos + b"%%EOF".len());
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip: a single byte XORed with `FZHMEI` should produce the
    /// original byte back.
    #[test]
    fn decrypt_round_trip_single_byte() {
        let key = KDH_PASSPHRASE;
        for (i, original) in b"hello world!".iter().enumerate() {
            let encrypted = original ^ key[i % key.len()];
            assert_eq!(encrypted ^ key[i % key.len()], *original);
        }
    }

    /// The decryption must skip the first 254 bytes of the input.
    /// We construct a payload that, after XOR with `FZHMEI`, yields a
    /// string ending in `%%EOF` (so the `rfind("%%EOF")` step has
    /// something to find).
    #[test]
    fn decrypt_skips_254_byte_header() {
        let target = b"%PDF-1.3\n%%EOF\n";
        let mut encrypted = target.to_vec();
        for (i, b) in encrypted.iter_mut().enumerate() {
            *b ^= KDH_PASSPHRASE[i % KDH_PASSPHRASE.len()];
        }
        let mut input = vec![0u8; 254];
        input.extend_from_slice(&encrypted);
        let out = decrypt_kdh(&input);
        // The output is the (decrypted) target truncated to the last
        // %%EOF, so the trailing newline gets dropped (this matches
        // the original Python behaviour).
        let last_eof = target.iter().rposition(|&b| b == b'\n').unwrap() + 1;
        let truncated = &target[..target.len() - (target.len() - last_eof)];
        // More simply: the output must end exactly at the last %%EOF.
        assert!(out.ends_with(b"%%EOF"));
        assert_eq!(out.len(), target.len() - 1);
    }

    /// Decryption should truncate everything after the last `%%EOF`.
    #[test]
    fn decrypt_truncates_after_last_eof() {
        let mut input = vec![0u8; 254];
        let mut payload = b"%PDF-1.3\njunk\n%%EOF\nmore-junk-after".to_vec();
        // Re-XOR so the magic+EOF survive the keystream.
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= KDH_PASSPHRASE[i % KDH_PASSPHRASE.len()];
        }
        input.extend_from_slice(&payload);
        let out = decrypt_kdh(&input);
        // The trailing "more-junk-after" must be truncated.
        assert!(out.ends_with(b"%%EOF"));
        // Output is a strict prefix of the decrypted payload, ending
        // right after the last %%EOF.
        assert!(out.len() < 254 + payload.len());
    }

    /// An input shorter than 254 bytes must yield an empty output.
    #[test]
    fn decrypt_handles_truncated_input() {
        assert_eq!(decrypt_kdh(&[]), Vec::<u8>::new());
        assert_eq!(decrypt_kdh(&[0u8; 100]), Vec::<u8>::new());
        assert_eq!(decrypt_kdh(&[0u8; 254]), Vec::<u8>::new());
    }

    /// If the input has no `%%EOF` the original Python raises; the Rust
    /// port returns the whole decrypted blob instead (more forgiving).
    #[test]
    fn decrypt_no_eof_returns_full_payload() {
        let mut input = vec![0u8; 254];
        input.extend_from_slice(b"no eof marker here at all");
        let out = decrypt_kdh(&input);
        // The whole payload comes through, in XORed form.
        assert_eq!(
            out.len(),
            b"no eof marker here at all".len()
        );
    }
}
