//! Parse the dispatch records of a single HN-format page (text + figure
//! positions).
//!
//! See `docs/format-analysis.md` for the record grammar.

/// Parse the text section of an HN page.
///
/// The on-disk format is a flat stream of 2-byte dispatch codes followed by
/// record-specific payloads:
///
/// * `0x8001` / `0x8070` – a run of single GBK characters. Old-style pages
///   (`page_style == true`) use a 4-byte-per-character layout; new-style
///   pages use a 6-byte layout where the first 2 bytes of the payload are
///   the character's GBK code and the next 2 are an unknown field.
/// * `0x800A` – a figure position record (26 bytes: x, y, width, height,
///   plus 8 unknown bytes).
///
/// Any other dispatch code is treated as a 4-byte "skip" record.
pub fn parse_page_text(data: &[u8], page_style: bool) -> String {
    let mut out = String::new();
    let mut off = 0usize;
    let len = data.len();

    while off + 2 <= len {
        let code = u16::from_le_bytes([data[off], data[off + 1]]);
        off += 2;

        match (code, page_style) {
            (0x8001, false) => {
                if off + 4 > len {
                    break;
                }
                let b1 = data[off + 1];
                let b0 = data[off];
                push_gbk(&mut out, b0, b1);
                off += 4;
            }
            (0x8001 | 0x8070, true) => {
                // 0x8001 in old-style means "newline before this run".
                if code == 0x8001 {
                    out.push('\n');
                }
                off += 2; // skip 2 unknown bytes
                while off + 4 <= len {
                    if data[off + 1] == 0x80 {
                        break;
                    }
                    let b1 = data[off + 3];
                    let b0 = data[off + 2];
                    push_gbk(&mut out, b0, b1);
                    off += 4;
                }
            }
            (0x800A, _) => {
                if off + 26 > len {
                    break;
                }
                // Figure records don't contribute to the text body.
                off += 26;
            }
            _ => {
                off += 2;
            }
        }
    }
    out
}

fn push_gbk(out: &mut String, b0: u8, b1: u8) {
    // The original Python code maps a small set of "GBK extension" code points
    // that show up as OCR artifacts in HN files: 0xA38D/0xA38A → line break,
    // 0xA389 → tab, 0xA3A0 → space.
    let code = ((b1 as u16) << 8) | (b0 as u16);
    match code {
        0xA389 => out.push('\t'),
        0xA38A => out.push('\n'),
        0xA38D => out.push('\r'),
        0xA3A0 => out.push(' '),
        _ => {
            let bytes = [b0, b1];
            let (cow, _, had_repl) = encoding_rs::GBK.decode(&bytes);
            if !had_repl && !cow.is_empty() {
                out.push_str(&cow);
            } else {
                out.push_str(&format!("<0x{:04X}>\n", code));
            }
        }
    }
}
