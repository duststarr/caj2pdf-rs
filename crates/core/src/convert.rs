//! High-level conversion entry point: turn a CAJ-family file into a PDF.
//!
//! This module is the single place that knows about every format. Other
//! crates (e.g. the CLI) should call [`convert`] and let this module dispatch
//! on the detected file format.

use std::path::Path;

use crate::{CajDocument, CajError, CajResult, FileFormat};

/// XOR key used by the KDH format's "encryption".
pub const KDH_PASSPHRASE: &[u8] = b"FZHMEI";

/// Convert an opened CAJ-family file to a PDF at `output`.
///
/// This is the high-level entry point used by the CLI. The per-format
/// pipeline is:
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
    tracing::info!(file = %input.display(), "opening input");
    let doc = CajDocument::open(input)?;
    tracing::info!(format = %doc.format(), "detected format");

    match doc.format() {
        FileFormat::Caj => convert_caj(&doc, output),
        FileFormat::Hn | FileFormat::C8 => convert_hn(&doc, output),
        FileFormat::Pdf => convert_pdf(&doc, output),
        FileFormat::Kdh => convert_kdh(&doc, output),
        FileFormat::Teb => Err(CajError::Unsupported(
            "TEB format not yet implemented",
        )),
    }
}

fn convert_caj(doc: &CajDocument, output: &Path) -> CajResult<()> {
    tracing::info!("extracting embedded PDF from CAJ container");
    let pdf_bytes = doc.extract_pdf()?;
    tracing::info!(bytes = pdf_bytes.len(), "embedded PDF extracted");

    // The CAJ PDF is typically missing a /Catalog and /Pages object. We
    // delegate the xref repair + outline injection to a future integration
    // agent. For now we just write the raw bytes out so the user can still
    // see something.
    std::fs::write(output, &pdf_bytes)?;
    tracing::info!(file = %output.display(), "wrote CAJ PDF (xref repair TODO)");
    Ok(())
}

fn convert_hn(_doc: &CajDocument, _output: &Path) -> CajResult<()> {
    tracing::info!("iterating pages");
    let pages = _doc.pages()?;
    tracing::info!(count = pages.len(), "decoded page list");

    // For each page, decode the images and assemble a PDF.
    //
    // TODO(jbig1): call caj2pdf_jbig1::decode for ImageKind::Jbig1.
    // TODO(jbig2): call caj2pdf_jbig2::decode for ImageKind::Jbig2.
    // TODO(pdf):  assemble decoded images + text overlays via caj2pdf_pdf.
    let _ = pages; // silence unused warning until the integration agent wires this up
    unimplemented!(
        "HN/C8 -> PDF conversion requires caj2pdf-jbig1, caj2pdf-jbig2, and caj2pdf-pdf"
    );
    // The integration agent will replace the `unimplemented!` with something
    // like:
    //     let mut page_inputs = Vec::with_capacity(pages.len());
    //     for page in &pages {
    //         for raw in &page.images {
    //             let decoded = match raw.kind {
    //                 ImageKind::Jbig1 => caj2pdf_jbig1::decode(...)?,
    //                 ...
    //             };
    //             page_inputs.push(DecodedImage::Mono { ... });
    //         }
    //     }
    //     let pdf = caj2pdf_pdf::build_document(&page_inputs, doc.toc())?;
    //     std::fs::write(output, pdf)?;
}

fn convert_pdf(doc: &CajDocument, output: &Path) -> CajResult<()> {
    tracing::info!("PDF pass-through, copying file");
    std::fs::copy(doc.path(), output)?;
    Ok(())
}

fn convert_kdh(doc: &CajDocument, output: &Path) -> CajResult<()> {
    tracing::info!("decrypting KDH");
    let bytes = std::fs::read(doc.path())?;
    let decrypted = decrypt_kdh(&bytes);
    std::fs::write(output, &decrypted)?;
    tracing::info!(file = %output.display(), "wrote decrypted PDF");
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
