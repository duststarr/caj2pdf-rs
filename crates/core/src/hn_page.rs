//! Parse the dispatch records of a single HN-format page (text + figure
//! positions).
//!
//! See `docs/format-analysis.md` for the record grammar.

#[cfg(test)]
mod tests {
    use super::parse_page_text;

    /// A new-style 0x8001 record carries a GBK character in two bytes
    /// (low, high) followed by two unknown bytes. The GBK char `中` is
    /// `0xD6 0xD0` – we store it little-endian as `[0xD0, 0xD6]`.
    #[test]
    fn parse_new_style_single_char() {
        // 0x8001 0xD0 0xD6 0x00 0x00
        let data = [0x01, 0x80, 0xD0, 0xD6, 0x00, 0x00];
        let out = parse_page_text(&data, false);
        assert_eq!(out, "中");
    }

    /// `0xA38A` (0x8A, 0xA3 in file) is mapped to `\n` per the OCR
    /// artifact table.
    #[test]
    fn parse_new_style_ocr_linebreak() {
        // 0x8001 0x8A 0xA3 0x00 0x00
        let data = [0x01, 0x80, 0x8A, 0xA3, 0x00, 0x00];
        let out = parse_page_text(&data, false);
        assert_eq!(out, "\n");
    }

    /// `0x800A` is a 26-byte figure record; it must not contribute any
    /// text and must not panic on the trailing bytes.
    #[test]
    fn parse_skips_figure_records() {
        // 0x800A followed by 26 zero bytes
        let mut data = vec![0x0A, 0x80];
        data.extend(std::iter::repeat(0u8).take(26));
        let out = parse_page_text(&data, false);
        assert_eq!(out, "");
    }

    /// Old-style (page_style=true) records: a 0x8001 emits a newline,
    /// then 2 unknown bytes, then a run of 4-byte characters. Each
    /// 4-byte record has the GBK char at positions [2..4] (low, high)
    /// in file order; position [1] is 0x80 to signal the next dispatch
    /// code.
    #[test]
    fn parse_old_style_text_run() {
        // 0x8001 0x00 0x00  [newline emitted]
        // 0x00 0x00 0xD0 0xD6  [first 4-byte record, GBK char = 中]
        // 0x00 0x80           [end-of-run marker at position 1]
        let data = [
            0x01, 0x80, 0x00, 0x00, //
            0x00, 0x00, 0xD0, 0xD6, //
            0x00, 0x80, //
        ];
        let out = parse_page_text(&data, true);
        assert_eq!(out, "\n中");
    }
}

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
                let b0 = data[off];
                let b1 = data[off + 1];
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
                    let b0 = data[off + 2];
                    let b1 = data[off + 3];
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

/// Decode one GBK character from the two bytes stored little-endian in the
/// file. `b0` is the low byte, `b1` is the high byte, so the numeric code is
/// `b1 * 256 + b0`. GBK decoding itself expects `[high, low]`, hence the
/// `[b1, b0]` byte order below.
fn push_gbk(out: &mut String, b0: u8, b1: u8) {
    let code = ((b1 as u16) << 8) | (b0 as u16);
    match code {
        0xA389 => out.push('\t'),
        0xA38A => out.push('\n'),
        0xA38D => out.push('\r'),
        0xA3A0 => out.push(' '),
        _ => {
            let bytes = [b1, b0];
            let (cow, _, had_repl) = encoding_rs::GBK.decode(&bytes);
            if !had_repl && !cow.is_empty() {
                out.push_str(&cow);
            } else {
                out.push_str(&format!("<0x{:04X}>\n", code));
            }
        }
    }
}
