//! PDF document builder.
//!
//! Takes a list of [`PageInput`]s and produces a complete `Vec<u8>` PDF.
//!
//! All PDF objects are constructed with the `lopdf` crate; this module
//! is responsible only for the caj2pdf-specific glue (image embedding,
//! `/MediaBox` sizing, outline injection).

use caj2pdf_core::DecodedImage;
use lopdf::{Dictionary, Document, Object, Stream};
use tracing::warn;

use crate::{OutlineEntry, PageInput, PdfResult};

/// Default DPI used by caj2pdf for converting pixels to PDF points.
///
/// At 300 DPI, one pixel is exactly one point (1 in = 72 pt = 300 px),
/// so we can use the image's native pixel dimensions as the page's
/// `/MediaBox` directly.
const DEFAULT_DPI: f64 = 300.0;

/// Build a fresh PDF document from the given pages and outlines.
///
/// See [the crate-level docs](crate) for the overall design.
pub fn build_document(
    pages: &[PageInput],
    outlines: &[OutlineEntry],
) -> PdfResult<Vec<u8>> {
    let mut doc = Document::with_version("1.4");

    // The Info dictionary is referenced from the trailer; without it,
    // some PDF readers complain.
    let info_id = doc.add_object(Dictionary::new());
    doc.trailer.set("Info", info_id);

    // Build each page and its image, remembering each page's id so
    // we can wire up the /Pages tree at the end.
    let mut page_ids: Vec<(u32, (u32, u16))> = Vec::with_capacity(pages.len());
    for (i, page_input) in pages.iter().enumerate() {
        let page_num = (i + 1) as u32;
        let page_id = add_single_page(&mut doc, page_input)?;
        page_ids.push((page_num, page_id));
    }

    // Build the /Pages tree once we know all the page ids.  We also
    // build the page-number -> object-id map up front so the outline
    // builder can look up destinations by 1-based page number.
    let pages_id = doc.new_object_id();
    let kids: Vec<Object> = page_ids
        .iter()
        .map(|(_, page_id)| Object::Reference(*page_id))
        .collect();
    let pages_dict = Dictionary::from_iter(vec![
        ("Type", Object::Name(b"Pages".to_vec())),
        ("Kids", Object::Array(kids)),
        ("Count", Object::Integer(page_ids.len() as i64)),
    ]);
    doc.objects.insert(pages_id, Object::Dictionary(pages_dict));

    // Now patch the /Parent reference into each page (lopdf's Document
    // does not own a notion of "page tree", so we do it manually).
    for (_, page_id) in &page_ids {
        if let Ok(page) = doc.get_object_mut(*page_id).and_then(Object::as_dict_mut) {
            page.set("Parent", pages_id);
        } else {
            warn!("page object {:?} disappeared during assembly", page_id);
        }
    }

    // Catalog points to the page tree.
    let catalog_id = doc.add_object(Dictionary::from_iter(vec![
        ("Type", Object::Name(b"Catalog".to_vec())),
        ("Pages", Object::Reference(pages_id)),
    ]));
    doc.trailer.set("Root", catalog_id);

    // Outlines are injected last so that the outline item object ids
    // do not collide with the page / image ids.
    if !outlines.is_empty() {
        // Build a (1-based page number, ObjectId) map for the outline
        // builder. lopdf's get_pages returns a BTreeMap in the same
        // shape, so we mirror that.
        let page_map: std::collections::BTreeMap<u32, (u32, u16)> = page_ids
            .iter()
            .map(|(page_num, page_id)| (*page_num, *page_id))
            .collect();
        crate::outlines::build_outline_dict(&mut doc, outlines, &page_map)?;
    }

    // Save to bytes. lopdf 0.33 does not expose `save_to_bytes`; its
    // `save_to` returns `std::io::Result` (it shadows the import), so
    // we map any I/O error into our own error type.
    let mut buf: Vec<u8> = Vec::new();
    doc.save_to(&mut buf).map_err(crate::PdfError::from_io)?;
    Ok(buf)
}

/// Add a single page to the document and return its object id.
///
/// The image is embedded as either a 1-bpp DeviceGray image
/// (with optional CCITT Group 4 compression) or a DCT JPEG image,
/// depending on the variant of [`DecodedImage`].
fn add_single_page(
    doc: &mut Document,
    page_input: &PageInput,
) -> PdfResult<(u32, u16)> {
    let image_id = build_image(doc, &page_input.image)?;
    let (width_pt, height_pt) = page_size_for(&page_input.image);

    // The content stream just draws the image at full page size.
    // The `cm` matrix is followed by `Do` to reference /Im0.
    let content = build_page_content_stream(width_pt, height_pt);
    let content_id = doc.add_object(Stream::new(Dictionary::new(), content));

    // The XObject dictionary references the image.
    let xobjects_dict = Dictionary::from_iter(vec![("Im0", Object::Reference(image_id))]);
    let resources_id = doc.add_object(Dictionary::from_iter(vec![(
        "XObject",
        Object::Dictionary(xobjects_dict),
    )]));

    let page_dict = Dictionary::from_iter(vec![
        ("Type", Object::Name(b"Page".to_vec())),
        ("MediaBox", Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Real(width_pt as f32),
            Object::Real(height_pt as f32),
        ])),
        ("Resources", Object::Reference(resources_id)),
        ("Contents", Object::Reference(content_id)),
    ]);

    let page_id = doc.add_object(page_dict);
    Ok(page_id)
}

/// Construct the content stream for a page that paints a single image
/// covering the whole `/MediaBox`.
///
/// The `cm` matrix scales the image's 1x1 unit square to `width_pt x
/// height_pt`; the translation is zero because the image's
/// top-left in PDF coordinates already lines up with the page's
/// bottom-left.
fn build_page_content_stream(width_pt: f64, height_pt: f64) -> Vec<u8> {
    // q  <w> 0 0 <h> 0 0 cm  /Im0 Do  Q
    format!(
        "q\n{width_pt} 0 0 {height_pt} 0 0 cm\n/Im0 Do\nQ\n"
    )
    .into_bytes()
}

/// Convert a `DecodedImage` into an `ImageXObject` and return its id.
fn build_image(doc: &mut Document, image: &DecodedImage) -> PdfResult<(u32, u16)> {
    let dict = image_dict_for(image);
    let content = image_content_for(image)?;
    let id = doc.add_object(Stream::new(dict, content));
    Ok(id)
}

/// Image dictionary only — used as the `Object` stored in the document.
fn image_dict_for(image: &DecodedImage) -> Dictionary {
    match image {
        DecodedImage::Mono { width_px, height_px, .. } => Dictionary::from_iter(vec![
            ("Type", Object::Name(b"XObject".to_vec())),
            ("Subtype", Object::Name(b"Image".to_vec())),
            ("Width", Object::Integer(*width_px as i64)),
            ("Height", Object::Integer(*height_px as i64)),
            ("ColorSpace", Object::Name(b"DeviceGray".to_vec())),
            ("BitsPerComponent", Object::Integer(1)),
            // Invert 1<->0 so that bit value 1 paints as black, 0 as
            // white.  The CAJ/PDF convention is "1 = ink".
            ("Decode", Object::Array(vec![Object::Integer(1), Object::Integer(0)])),
        ]),
        DecodedImage::Jpeg { width_px, height_px, .. } => Dictionary::from_iter(vec![
            ("Type", Object::Name(b"XObject".to_vec())),
            ("Subtype", Object::Name(b"Image".to_vec())),
            ("Width", Object::Integer(*width_px as i64)),
            ("Height", Object::Integer(*height_px as i64)),
            ("ColorSpace", Object::Name(b"DeviceRGB".to_vec())),
            ("BitsPerComponent", Object::Integer(8)),
            ("Filter", Object::Name(b"DCTDecode".to_vec())),
        ]),
    }
}

/// Image content stream — the raw bytes that go into the PDF stream.
fn image_content_for(image: &DecodedImage) -> PdfResult<Vec<u8>> {
    match image {
        DecodedImage::Mono { width_px, height_px, bits } => {
            let stride = (*width_px as usize).div_ceil(8);
            let needed = stride * (*height_px as usize);
            if bits.len() < needed {
                return Err(crate::PdfError::Assembly(format!(
                    "mono bitmap is too small: got {} bytes, expected at least {}",
                    bits.len(),
                    needed
                )));
            }
            let raw = bits[..needed].to_vec();
            // Compress with zlib; the resulting `Stream` will carry the
            // `Filter /FlateDecode` entry.
            let mut s = Stream::new(Dictionary::new(), raw);
            s.compress()?;
            Ok(s.content)
        }
        DecodedImage::Jpeg { jpeg_bytes, .. } => Ok(jpeg_bytes.clone()),
    }
}

/// Compute the page size in PDF points for the given image.
///
/// At 300 DPI, 1 px = 1 pt, so the page is exactly the image's pixel
/// dimensions.
fn page_size_for(image: &DecodedImage) -> (f64, f64) {
    let w_px = image.width_px() as f64;
    let h_px = image.height_px() as f64;
    (px_to_pt(w_px, DEFAULT_DPI), px_to_pt(h_px, DEFAULT_DPI))
}

/// Convert a length in pixels to a length in PDF points (1/72 of an inch),
/// for a given DPI.
///
/// This matches the `px_to_pt` helper in the original `pdfwutils.py`.
fn px_to_pt(length_px: f64, dpi: f64) -> f64 {
    72.0 * length_px / dpi
}
