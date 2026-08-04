//! # caj2pdf-core
//!
//! Core types, format detection, and CAJ/HN parsing for caj2pdf-rs.
//!
//! This crate is the single source of truth for the data model shared by every
//! other crate in the workspace. It is intentionally dependency-light: only
//! `byteorder` and `encoding_rs` are required at runtime.
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

    #[error("malformed {format:?} file: {message}")]
    Malformed { format: FileFormat, message: String },

    #[error("page index {index} out of range (page count is {count})")]
    PageOutOfRange { index: usize, count: usize },

    #[error("text decoding error: {0}")]
    Text(String),

    #[error("zlib decompression failed: {0}")]
    Zlib(String),
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
    /// HN8 / HN format (GBK-encoded ASCII magic at offset 0).
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
    pub kind: ImageKind,
    pub data: Vec<u8>,
    /// Native pixel width, in 1/300 inch coordinate units. The original
    /// documents use a fixed 300 DPI, so this is the same as the rendered size
    /// in points.
    pub width_px: u32,
    /// Native pixel height.
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
    /// format-specific and interpreted by `crate::pages`.
    pub(crate) layout: Layout,
}

/// Format-specific page-data layout.
#[derive(Debug, Clone)]
pub(crate) enum Layout {
    Caj {
        pdf_start_offset: u64,
        page_info_table_offset: u64,
    },
    Hn {
        page_info_table_offset: u64,
        toc_end_offset: u64,
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
    pub fn open<P: AsRef<Path>>(path: P) -> CajResult<Self> {
        let path = path.as_ref().to_path_buf();
        let mut f = std::fs::File::open(&path)?;
        let mut header = [0u8; 4];
        use std::io::Read;
        f.read_exact(&mut header)?;

        let format = detect_format(&header, &mut f)?;
        drop(f);

        let mut doc = Self {
            path,
            format,
            page_count: 0,
            toc: Vec::new(),
            layout: detect_layout(format)?,
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

    pub fn path(&self) -> &Path {
        &self.path
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

mod caj;
mod hn;
mod hn_page;

pub(crate) fn detect_format<R: std::io::Read + std::io::Seek>(
    header: &[u8; 4],
    f: &mut R,
) -> CajResult<FileFormat> {
    use std::io::SeekFrom;

    // C8: first byte 0xC8, with a tiny header before page data.
    if header[0] == 0xC8 {
        return Ok(FileFormat::C8);
    }

    // HN8: starts with ASCII "HN" then binary bytes.
    if &header[0..2] == b"HN" {
        // The first 4 bytes are "HN" + 2 bytes. C8 (HN with binary) has
        // 0xC8 0x00 in those two bytes.
        if &header[2..4] == b"\xc8\x00" {
            return Ok(FileFormat::Hn);
        }
        // Otherwise fall through to GBK magic check below.
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

    match trimmed.as_str() {
        "CAJ" => Ok(FileFormat::Caj),
        "HN" => Ok(FileFormat::Hn),
        "%PDF" => Ok(FileFormat::Pdf),
        "KDH " => Ok(FileFormat::Kdh),
        "TEB" => Ok(FileFormat::Teb),
        _ => {
            // Some HN files look like "H" + binary. Re-check by sampling
            // further into the header.
            let _ = f.seek(SeekFrom::Start(0));
            Err(CajError::UnknownFormat(*header))
        }
    }
}

pub(crate) fn detect_layout(format: FileFormat) -> CajResult<Layout> {
    // For Caj/Hn/C8, the page-data offset depends on the page count, which is
    // only known after reading the page-count field. We store a placeholder
    // here and update it inside `read_meta` once the page count is known.
    Ok(match format {
        FileFormat::Caj => Layout::Caj {
            pdf_start_offset: 0,
            page_info_table_offset: 0,
        },
        FileFormat::Hn => Layout::Hn {
            page_info_table_offset: 0,
            toc_end_offset: 0,
        },
        FileFormat::C8 => Layout::C8 {
            page_info_table_offset: 0,
        },
        FileFormat::Pdf => Layout::Pdf,
        FileFormat::Kdh => Layout::Kdh,
        FileFormat::Teb => Layout::Teb,
    })
}
