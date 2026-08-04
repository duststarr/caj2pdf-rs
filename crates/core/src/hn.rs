//! HN / C8 format specific reading logic.
//!
//! See `docs/format-analysis.md` for the byte-level layout this module parses.

use crate::{CajDocument, CajError, CajResult, Layout, OutlineEntry, RawImage, Page, ImageKind};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{Read, Seek, SeekFrom};

const PAGE_INFO_ENTRY_SIZE: usize = 20;
const TOC_ENTRY_SIZE: usize = 0x134;

/// Read the page count and (optionally) the table of contents for an HN or C8
/// file, then patch the document's layout with the actual offsets.
pub(crate) fn read_meta(doc: &mut CajDocument) -> CajResult<()> {
    let mut f = std::fs::File::open(&doc.path)?;

    // Page-count offset depends on the format.
    let (page_count_offset, toc_count_offset, toc_entry_offset) = match doc.format {
        crate::FileFormat::Hn => (0x90usize, 0x158usize, 0x158 + 4),
        crate::FileFormat::C8 => (0x08usize, 0usize, 0usize), // C8 has no TOC
        _ => unreachable!("read_meta called for non-HN/C8"),
    };

    f.seek(SeekFrom::Start(page_count_offset as u64))?;
    let page_count = f.read_u32::<LittleEndian>()?;
    doc.page_count = page_count;

    if toc_count_offset == 0 {
        doc.layout = Layout::C8 {
            page_info_table_offset: 0x50,
        };
        return Ok(());
    }

    f.seek(SeekFrom::Start(toc_count_offset as u64))?;
    let toc_count = f.read_u32::<LittleEndian>()?;
    doc.toc = Vec::with_capacity(toc_count as usize);
    for i in 0..toc_count {
        let offset = toc_entry_offset as u64 + (i as u64) * TOC_ENTRY_SIZE as u64;
        f.seek(SeekFrom::Start(offset))?;
        let mut buf = [0u8; TOC_ENTRY_SIZE];
        f.read_exact(&mut buf)?;
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

    let toc_end_offset = toc_entry_offset as u64 + (toc_count as u64) * TOC_ENTRY_SIZE as u64;
    let page_info_table_offset = toc_end_offset;
    doc.layout = Layout::Hn {
        page_info_table_offset,
        toc_end_offset,
    };

    Ok(())
}

/// Iterate the per-page info table of an HN file, yielding a [`Page`] for
/// each entry. Image blocks are kept as [`RawImage`] ready for decoding by
/// the JBIG/JPEG decoders in `caj2pdf-jbig1` / `caj2pdf-jbig2`.
pub fn iter_pages(doc: &CajDocument) -> CajResult<Vec<Page>> {
    match doc.layout {
        Layout::Hn {
            page_info_table_offset,
            ..
        } => iter_pages_inner(doc, page_info_table_offset, true),
        Layout::C8 {
            page_info_table_offset,
        } => iter_pages_inner(doc, page_info_table_offset, false),
        _ => Err(CajError::Malformed {
            format: doc.format,
            message: "iter_pages called on a non-HN/C8 document".into(),
        }),
    }
}

fn iter_pages_inner(
    doc: &CajDocument,
    page_info_table_offset: u64,
    parse_toc: bool,
) -> CajResult<Vec<Page>> {
    use flate2::read::ZlibDecoder;
    let mut f = std::fs::File::open(&doc.path)?;
    let mut pages = Vec::with_capacity(doc.page_count as usize);

    for i in 0..doc.page_count {
        let info_offset = page_info_table_offset + (i as u64) * PAGE_INFO_ENTRY_SIZE as u64;
        f.seek(SeekFrom::Start(info_offset))?;
        // struct PageInfo { page_data_offset: i32, size_of_text_section: i32,
        //                    images_per_page:    i16, page_no:           i16,
        //                    unk2:               i16, _pad:              i16,
        //                    next_page_data_offset: i32 }
        let page_data_offset = f.read_i32::<LittleEndian>()? as u64;
        let size_of_text_section = f.read_i32::<LittleEndian>()? as usize;
        let images_per_page = f.read_i16::<LittleEndian>()? as i32;
        let _page_no = f.read_i16::<LittleEndian>()?;
        let _unk2 = f.read_i16::<LittleEndian>()?;
        let _pad = f.read_i16::<LittleEndian>()?;
        let next_page_data_offset = f.read_i32::<LittleEndian>()? as u64;

        // ---- text section ----
        f.seek(SeekFrom::Start(page_data_offset))?;
        let mut header = [0u8; 32];
        f.read_exact(&mut header)?;

        let decompressed: Vec<u8> = if header[8..20] == *b"COMPRESSTEXT" || header[0..12] == *b"COMPRESSTEXT" {
            let coff = if header[0..12] == *b"COMPRESSTEXT" { 0 } else { 8 };
            let expanded_text_size = i32::from_le_bytes(
                header[12 + coff..16 + coff].try_into().unwrap(),
            ) as usize;
            f.seek(SeekFrom::Start(page_data_offset + 16 + coff as u64))?;
            let mut compressed = vec![0u8; size_of_text_section - 16 - coff];
            f.read_exact(&mut compressed)?;
            let mut dec = ZlibDecoder::new(&compressed[..]);
            let mut out = Vec::with_capacity(expanded_text_size);
            std::io::copy(&mut dec, &mut out).map_err(|e| CajError::Zlib(e.to_string()))?;
            if out.len() != expanded_text_size {
                return Err(CajError::Malformed {
                    format: doc.format,
                    message: format!(
                        "page {} text size mismatch: got {} expected {}",
                        i + 1,
                        out.len(),
                        expanded_text_size
                    ),
                });
            }
            out
        } else {
            f.seek(SeekFrom::Start(page_data_offset))?;
            let mut out = vec![0u8; size_of_text_section];
            f.read_exact(&mut out)?;
            out
        };

        let old_style = next_page_data_offset > page_data_offset;
        let text = if parse_toc {
            crate::hn_page::parse_page_text(&decompressed, old_style)
        } else {
            String::new()
        };

        // ---- image blocks ----
        let mut images = Vec::with_capacity(images_per_page.max(0) as usize);
        let mut current_offset = page_data_offset + size_of_text_section as u64;
        for _ in 0..images_per_page.max(0) {
            f.seek(SeekFrom::Start(current_offset))?;
            let mut head = [0u8; 12];
            f.read_exact(&mut head)?;
            let image_type_enum = i32::from_le_bytes(head[0..4].try_into().unwrap());
            let offset_to_image_data = i32::from_le_bytes(head[4..8].try_into().unwrap()) as u64;
            let size_of_image_data = i32::from_le_bytes(head[8..12].try_into().unwrap()) as usize;
            if offset_to_image_data != current_offset + 12 {
                return Err(CajError::Malformed {
                    format: doc.format,
                    message: format!(
                        "page {} unusual image offset: header says 0x{:x}, expected 0x{:x}",
                        i + 1,
                        offset_to_image_data,
                        current_offset + 12
                    ),
                });
            }
            f.seek(SeekFrom::Start(offset_to_image_data))?;
            let mut data = vec![0u8; size_of_image_data];
            f.read_exact(&mut data)?;
            current_offset = offset_to_image_data + size_of_image_data as u64;

            // The image header (first 48 bytes) holds the dimensions:
            //   u32 width, u32 height, u16 planes, u16 bits_per_pixel
            let (width, height) = if data.len() >= 16 {
                (
                    u32::from_le_bytes(data[4..8].try_into().unwrap()),
                    u32::from_le_bytes(data[8..12].try_into().unwrap()),
                )
            } else {
                (0, 0)
            };
            let kind = match image_type_enum {
                0 => ImageKind::Jbig1,
                1 | 2 => ImageKind::Jpeg { upside_down: image_type_enum == 2 },
                3 => ImageKind::Jbig2,
                other => {
                    return Err(CajError::Malformed {
                        format: doc.format,
                        message: format!("page {} unknown image type {}", i + 1, other),
                    });
                }
            };
            images.push(RawImage { kind, data, width_px: width, height_px: height });
        }

        pages.push(Page { text, images });
    }

    Ok(pages)
}
