//! # caj2pdf-core
//!
//! Core types, format detection, and CAJ/HN parsing for caj2pdf-rs.
//!
//! This crate is the single source of truth for the data model shared by every
//! other crate in the workspace. It is intentionally dependency-light: only
//! `byteorder`, `encoding_rs`, `flate2`, `thiserror`, and `tracing` are required
//! at runtime.
//!
//! ## High-level data flow
//!
//! ```text
//!   .caj/.hn/.c8 file
//!         │
//!         ▼
//!   CajDocument::open      ──► format detection, page count, TOC
//!         │
//!         ▼
//!   for each page:
//!     text section     ──► optional zlib-decompressed dispatch records
//!     per-image block  ──► JBIG1 / JBIG2 / JPEG bytes
//!         │
//!         ▼
//!   DecodedImage        ──► 1bpp bitmap or RGB JPEG
//!         │
//!         ▼
//!   caj2pdf-pdf         ──► final PDF with /Outlines
//! ```
//!
//! See `docs/format-analysis.md` for the on-disk layout of each format.

#![deny(missing_debug_implementations)]
#![warn(unreachable_pub)]

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

// ---------------------------------------------------------------------------
// Public error type
// ---------------------------------------------------------------------------

/// Errors that can occur while opening, parsing, or extracting pages from a
/// CAJ-family file.
#[derive(Debug, Error)]
pub enum CajError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("unsupported or unknown file type (magic bytes: {0:?})")]
    UnknownFormat([u8; 4]),

    #[error("unsupported feature: {0}")]
    Unsupported(&'static str),

    #[error("malformed {format:?} file: {message}")]
    Malformed { format: FileFormat, message: String },

    #[error("page index {index} out of range (page count is {count})")]
    PageOutOfRange { index: usize, count: usize },

    #[error("text decoding error: {0}")]
    Text(String),

    #[error("zlib decompression failed: {0}")]
    Zlib(String),
}

impl CajError {
    /// Convenience constructor for the [`CajError::Malformed`] variant.
    pub fn malformed(format: FileFormat, message: impl Into<String>) -> Self {
        CajError::Malformed {
            format,
            message: message.into(),
        }
    }
}

pub type CajResult<T> = std::result::Result<T, CajError>;

// ---------------------------------------------------------------------------
// Format identification
// ---------------------------------------------------------------------------

/// The on-disk format of an academic-journal file.
///
/// See `docs/format-analysis.md` for the byte-level identification rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileFormat {
    /// Older CAJ format (GBK-encoded ASCII magic at offset 0).
    Caj,
    /// HN format (GBK-encoded ASCII magic or `HN\xc8\x00` magic at offset 0).
    Hn,
    /// "C8" magic byte followed by 18 bytes of header before page data.
    C8,
    /// PDF wrapped in a CAJ container.
    Pdf,
    /// KDH encrypted format. The decryption uses a fixed XOR key.
    Kdh,
    /// Teb/Apabi format (best-effort identification only).
    Teb,
}

impl fmt::Display for FileFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            FileFormat::Caj => "CAJ",
            FileFormat::Hn => "HN",
            FileFormat::C8 => "C8",
            FileFormat::Pdf => "PDF",
            FileFormat::Kdh => "KDH",
            FileFormat::Teb => "TEB",
        };
        f.write_str(s)
    }
}

// ---------------------------------------------------------------------------
// Table of contents / outlines
// ---------------------------------------------------------------------------

/// One entry in the document's outline / table of contents.
///
/// `page` is 1-based, matching what the original Python project exposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineEntry {
    /// Title bytes, in UTF-8 (already transcoded from GBK by the parser).
    pub title: String,
    /// 1-based page number this entry points to.
    pub page: u32,
    /// Nesting level (1 = top-level).
    pub level: u8,
}

// ---------------------------------------------------------------------------
// Page data model
// ---------------------------------------------------------------------------

/// Codec used to encode a single page image inside the container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageKind {
    /// Custom JBIG1 variant (port of `JBigDecode.cc`).
    Jbig1,
    /// JBIG2.
    Jbig2,
    /// JPEG (possibly upside-down depending on the variant byte).
    Jpeg { upside_down: bool },
}

/// A still-undecoded image block: the raw bytes from the container, with the
/// metadata needed to decode it.
#[derive(Debug, Clone)]
pub struct RawImage {
    /// The on-disk codec.
    pub kind: ImageKind,
    /// Raw image bytes (including the 48-byte CNKI header for JBIG / JBIG2).
    pub data: Vec<u8>,
    /// Native pixel width, in 1/300 inch coordinate units. The original
    /// documents use a fixed 300 DPI, so this is the same as the rendered size
    /// in points.
    pub width_px: u32,
    /// Native pixel height. May be reported as negative by the original
    /// Python parser for upside-down JPEGs; the Rust parser folds that into
    /// [`ImageKind::Jpeg::upside_down`].
    pub height_px: u32,
}

/// A decoded image ready to be embedded in a PDF.
#[derive(Debug, Clone)]
pub enum DecodedImage {
    /// 1-bit-per-pixel monochrome bitmap.
    Mono {
        width_px: u32,
        height_px: u32,
        /// Packed bits, one byte per row padded to 8 pixels.
        bits: Vec<u8>,
    },
    /// 8-bit JPEG.
    Jpeg {
        width_px: u32,
        height_px: u32,
        /// `height_px` may be negative in the original format to indicate an
        /// upside-down image; we normalize that into a separate flag here.
        jpeg_bytes: Vec<u8>,
    },
}

impl DecodedImage {
    /// Pixel width of the image.
    pub fn width_px(&self) -> u32 {
        match self {
            DecodedImage::Mono { width_px, .. } => *width_px,
            DecodedImage::Jpeg { width_px, .. } => *width_px,
        }
    }

    /// Pixel height of the image.
    pub fn height_px(&self) -> u32 {
        match self {
            DecodedImage::Mono { height_px, .. } => *height_px,
            DecodedImage::Jpeg { height_px, .. } => *height_px,
        }
    }
}

/// Raw content of a single page after the text-section dispatch records have
/// been parsed.
#[derive(Debug, Clone, Default)]
pub struct Page {
    /// Plain text extracted from the dispatch records (GBK → UTF-8).
    pub text: String,
    /// Image blocks present on this page, in the order they appear.
    pub images: Vec<RawImage>,
}

// ---------------------------------------------------------------------------
// Document handle
// ---------------------------------------------------------------------------

/// Handle to an opened CAJ-family file. Cheap to clone (just a `PathBuf` plus
/// cached offsets).
#[derive(Debug, Clone)]
pub struct CajDocument {
    pub(crate) path: PathBuf,
    pub(crate) format: FileFormat,
    pub(crate) page_count: u32,
    pub(crate) toc: Vec<OutlineEntry>,
    /// Offsets into the file that locate the page-data array. These are
    /// format-specific and interpreted by [`crate::hn::iter_pages`].
    pub(crate) layout: Layout,
}

/// Format-specific page-data layout.
///
/// For HN there are two sub-variants:
///
/// * `with_toc = true`  – the long-form HN file, with an outline at 0x158.
/// * `with_toc = false` – the short-form HN file (`HN\xc8\x00` magic), no
///   outline, page-info table at 0xD8.
#[derive(Debug, Clone)]
pub(crate) enum Layout {
    Caj {
        /// Offset of the first byte of the embedded PDF.
        pdf_start_offset: u64,
    },
    Hn {
        /// Offset of the first 20-byte PageInfo struct.
        page_info_table_offset: u64,
        /// True if the document has an outline tree at 0x158.
        with_toc: bool,
    },
    C8 {
        page_info_table_offset: u64,
    },
    Pdf,
    Kdh,
    Teb,
}

impl CajDocument {
    /// Open a file, detect its format, and read the page count and outline.
    ///
    /// # Errors
    /// * [`CajError::Io`] – the file could not be read.
    /// * [`CajError::UnknownFormat`] – the 4-byte magic is none of the six
    ///   known formats.
    pub fn open<P: AsRef<Path>>(path: P) -> CajResult<Self> {
        let path = path.as_ref().to_path_buf();
        let mut f = std::fs::File::open(&path)?;
        let mut header = [0u8; 4];
        use std::io::Read;
        f.read_exact(&mut header)?;

        // For HN we need to know which sub-variant we have *before* we can
        // pick a layout. The `with_toc` flag is derived from the magic
        // bytes: `HN\xc8\x00` ⇒ no outline, anything else ⇒ outline.
        let (format, hn_with_toc) = detect_format_inner(&header, &mut f)?;
        drop(f);

        let mut doc = Self {
            path,
            format,
            page_count: 0,
            toc: Vec::new(),
            layout: detect_layout(format, hn_with_toc)?,
        };

        doc.read_page_count_and_toc()?;
        Ok(doc)
    }

    /// The detected file format.
    pub fn format(&self) -> FileFormat {
        self.format
    }

    /// The number of pages in the document.
    pub fn page_count(&self) -> u32 {
        self.page_count
    }

    /// The number of outline entries (zero for some formats).
    pub fn toc_entry_count(&self) -> usize {
        self.toc.len()
    }

    /// The parsed outline, in the on-disk order.
    pub fn toc(&self) -> &[OutlineEntry] {
        &self.toc
    }

    /// The on-disk path the document was opened from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Iterate the pages of an HN / C8 document, returning the parsed text and
    /// raw image blocks for each.
    ///
    /// For CAJ / PDF / KDH / TEB, returns
    /// [`CajError::Unsupported`]: use [`CajDocument::extract_pdf`] for CAJ
    /// and PDF, or rely on [`crate::convert::convert`] for the high-level
    /// conversion.
    pub fn pages(&self) -> CajResult<Vec<Page>> {
        crate::hn::iter_pages(self)
    }

    /// Extract the embedded PDF blob of a CAJ document.
    ///
    /// The returned byte slice is the raw PDF as it sits inside the CAJ
    /// container, without any repair. For most workflows you want
    /// [`crate::convert::convert`] instead, which repairs the xref table and
    /// adds outlines.
    pub fn extract_pdf(&self) -> CajResult<Vec<u8>> {
        match self.format {
            FileFormat::Caj => crate::caj::extract_pdf(self),
            FileFormat::Pdf => std::fs::read(&self.path).map_err(CajError::from),
            _ => Err(CajError::Unsupported(
                "extract_pdf is only supported for CAJ and PDF documents",
            )),
        }
    }

    pub(crate) fn read_page_count_and_toc(&mut self) -> CajResult<()> {
        match self.layout {
            Layout::Caj { .. } => crate::caj::read_meta(self),
            Layout::Hn { .. } | Layout::C8 { .. } => crate::hn::read_meta(self),
            Layout::Pdf | Layout::Kdh | Layout::Teb => Ok(()),
        }
    }
}

// ---------------------------------------------------------------------------
// Format detection helper (private)
// ---------------------------------------------------------------------------

pub mod convert;

mod caj;
mod hn;
mod hn_page;

fn detect_format_inner<R: std::io::Read + std::io::Seek>(
    header: &[u8; 4],
    _f: &mut R,
) -> CajResult<(FileFormat, Option<bool>)> {
    use std::io::SeekFrom;

    // C8: first byte 0xC8, with a tiny header before page data.
    if header[0] == 0xC8 {
        return Ok((FileFormat::C8, None));
    }

    // HN with binary magic: "HN" + 0xC8 0x00 ⇒ short-form HN (no TOC).
    if &header[0..2] == b"HN" {
        if &header[2..4] == b"\xc8\x00" {
            return Ok((FileFormat::Hn, Some(false)));
        }
        // Fall through to the GBK magic check.
    }

    // Otherwise, interpret the first 4 bytes as GBK-decoded ASCII.
    let decoded = match encoding_rs::GBK.decode(&header[..]) {
        (cow, _, false) => cow.into_owned(),
        _ => {
            return Err(CajError::UnknownFormat(*header));
        }
    };

    // Trim NUL padding (e.g. "CAJ\0").
    let trimmed = decoded.trim_end_matches('\0').to_string();

    let result = match trimmed.as_str() {
        "CAJ" => Ok((FileFormat::Caj, None)),
        // The long-form HN file has a GBK-decoded "HN" magic (the two
        // bytes following "HN" are usually 0x00 0x00, or any pair that
        // decodes to empty / non-ASCII PUA characters that get trimmed).
        "HN" => Ok((FileFormat::Hn, Some(true))),
        "%PDF" => Ok((FileFormat::Pdf, None)),
        "KDH " => Ok((FileFormat::Kdh, None)),
        "TEB" => Ok((FileFormat::Teb, None)),
        _ => {
            // Some HN files have an unusual 2-byte suffix that does not
            // GBK-decode cleanly. We've already handled the
            // "HN\xc8\x00" case above; everything else is unknown.
            let _ = _f.seek(SeekFrom::Start(0));
            Err(CajError::UnknownFormat(*header))
        }
    };
    result
}

/// Public re-export of the internal detection helper, used by the integration
/// tests in `tests/format_detection.rs`.
///
/// The second return value is `Some(true)` / `Some(false)` for HN files,
/// where it indicates whether the document carries an outline tree. For
/// every other format the value is `None`.
#[doc(hidden)]
pub fn detect_format<R: std::io::Read + std::io::Seek>(
    header: &[u8; 4],
    f: &mut R,
) -> CajResult<(FileFormat, Option<bool>)> {
    detect_format_inner(header, f)
}

pub(crate) fn detect_layout(
    format: FileFormat,
    hn_with_toc: Option<bool>,
) -> CajResult<Layout> {
    // For Caj/Hn/C8, the page-data offset depends on the page count, which is
    // only known after reading the page-count field. We store a placeholder
    // here and update it inside `read_meta` once the page count is known.
    Ok(match format {
        FileFormat::Caj => Layout::Caj {
            pdf_start_offset: 0,
        },
        FileFormat::Hn => Layout::Hn {
            page_info_table_offset: 0,
            with_toc: hn_with_toc.unwrap_or(true),
        },
        FileFormat::C8 => Layout::C8 {
            page_info_table_offset: 0,
        },
        FileFormat::Pdf => Layout::Pdf,
        FileFormat::Kdh => Layout::Kdh,
        FileFormat::Teb => Layout::Teb,
    })
}
