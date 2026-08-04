//! CAJ-format specific reading logic.
//!
//! See `docs/format-analysis.md` for the byte-level layout this module parses.

use crate::{CajDocument, CajError, CajResult, OutlineEntry};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{Read, Seek, SeekFrom};

/// Read the page count and (optionally) the table of contents for a CAJ file,
/// then patch the document's layout with the actual offsets.
pub(crate) fn read_meta(doc: &mut CajDocument) -> CajResult<()> {
    use crate::Layout;
    let mut f = std::fs::File::open(&doc.path)?;

    // Page count is at offset 0x10 (4 bytes, little-endian).
    f.seek(SeekFrom::Start(0x10))?;
    let page_count = f.read_u32::<LittleEndian>()?;
    doc.page_count = page_count;

    // The original Python code uses 0x110 as the start of the TOC block. We
    // follow the same convention.
    f.seek(SeekFrom::Start(0x110))?;
    let toc_count = f.read_u32::<LittleEndian>()?;
    doc.toc = Vec::with_capacity(toc_count as usize);
    for i in 0..toc_count {
        let offset = 0x110u64 + 4 + (i as u64) * 0x134;
        f.seek(SeekFrom::Start(offset))?;
        let mut buf = [0u8; 0x134];
        f.read_exact(&mut buf)?;
        // Layout of each entry (308 bytes):
        //   0x000..0x100  title (256 bytes GBK, NUL-terminated)
        //   0x100..0x118  unknown 24 bytes
        //   0x118..0x124  page number (12 ASCII bytes, NUL-terminated)
        //   0x124..0x130  unknown 12 bytes
        //   0x130         level (4-byte int32, little-endian)
        let ttl_end = buf[..0x100].iter().position(|&b| b == 0).unwrap_or(0x100);
        let title_raw = &buf[..ttl_end];
        let (title, _, _) = encoding_rs::GBK.decode(title_raw);
        let title = title.into_owned();

        let pg_end = buf[0x118..0x124]
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(12);
        let page: u32 = std::str::from_utf8(&buf[0x118..0x118 + pg_end])
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);
        let level = i32::from_le_bytes(buf[0x130..0x134].try_into().unwrap()) as u8;

        doc.toc.push(OutlineEntry { title, page, level });
    }

    // Patch the layout now that we know the page count.
    // In CAJ format, the embedded PDF starts at an offset pointed to by
    // (0x14) and the page-info table sits immediately after.
    f.seek(SeekFrom::Start(0x14))?;
    let pdf_start_pointer = f.read_u32::<LittleEndian>()? as u64;
    f.seek(SeekFrom::Start(pdf_start_pointer))?;
    let pdf_start = f.read_u32::<LittleEndian>()? as u64;
    let page_info_table_offset = pdf_start_pointer + 4; // immediately after the pointer
    doc.layout = Layout::Caj {
        pdf_start_offset: pdf_start,
        page_info_table_offset,
    };

    Ok(())
}
