# CAJ-family on-disk format analysis

This document is the byte-level reference for every format the caj2pdf
toolchain has to parse. It is derived from the original Python project
(`cajparser.py`, `HNParsePage.py`, `utils.py`) and is the canonical source
of truth for the Rust parsers in `caj2pdf-core`.

If you ever need to look at a real file with a hex editor, every offset
mentioned here is taken straight from the Python and the existing parser
comments. Every layout field has been cross-checked against
`cajparser.py:23-105` (the `CAJParser.__init__` / `page_num` / `get_toc`
methods).

## 1. Overview

The CAJViewer reader (a closed-source Windows application from CNKI) emits
six different on-disk encodings for what is, at heart, the same kind of
document: paginated scanned/OCR'd Chinese journal articles. We support all
six for identification and four of them (CAJ, HN, C8, PDF) for actual
decoding; KDH can be decrypted; TEB is recognized but not yet implemented.

| Format | Magic (offset 0)        | Page count @ | Outline @    | Notes                                   |
|--------|-------------------------|--------------|--------------|-----------------------------------------|
| CAJ    | `CAJ\0`                 | 0x10         | 0x110        | embedded PDF, 308-byte TOC entries      |
| HN     | `HN\xc8\x00` *or* `HN\0\0` | 0x90      | 0x158 (or none) | two sub-variants; 20-byte PageInfo    |
| C8     | `\xc8`                  | 0x08         | (none)       | stripped-down HN; same PageInfo struct  |
| PDF    | `%PDF`                  | (n/a)        | (n/a)        | pass-through                            |
| KDH    | `KDH `                  | (n/a)        | (n/a)        | 254-byte header + 6-byte XOR key        |
| TEB    | `TEB\0`                 | (n/a)        | (n/a)        | Apabi format, not yet implemented       |

## 2. Byte-level magic identification

The first 4 bytes of the file are read into a 32-bit word and dispatched on
in this order (mirrors `cajparser.py:29-66`):

```
header[0] == 0xC8                                   → C8
header[0..2] == b"HN" && header[2..4] == b"\xc8\x00" → HN (no-TOC variant)
header[0..4] GBK-decodes to "CAJ"                    → CAJ
header[0..4] GBK-decodes to "HN"                     → HN (with-TOC variant)
header[0..4] GBK-decodes to "%PDF"                   → PDF
header[0..4] GBK-decodes to "KDH "                   → KDH
header[0..4] GBK-decodes to "TEB"                    → TEB
otherwise                                            → UnknownFormat
```

GBK decoding of the magic is the same logic the Python project uses: drop
NUL padding after decoding, then compare the remaining ASCII against the
known strings. The GBK branch also handles `HN\0\0` (and any
`HN<non-ASCII-glyph>` that strips to `"HN"` after GBK-decoding and NUL
trimming).

For the **HN no-TOC** variant, the file starts with the binary sequence
`b"HN\xc8\x00"`. This is the "short" HN file: it has no outline, and the
page-info table sits at file offset 0xD8 (vs. 0x158 + 4 + 308·toc_count for
the long form).

## 3. CAJ format

A CAJ file wraps a (typically slightly damaged) PDF document together with a
page count and an optional outline tree.

### 3.1 Header layout

| Offset | Size | Field                                       |
|--------|------|---------------------------------------------|
| 0x00   | 4    | Magic `b"CAJ\0"`                            |
| 0x10   | 4    | Page count (i32 LE)                         |
| 0x14   | 4    | Pointer P (i32 LE) into the file            |
| 0x110  | 4    | Outline entry count N (i32 LE)              |
| 0x114  | …    | N × 308-byte outline entries                |
| P      | 4    | Real PDF start offset Q (i32 LE)            |
| Q      | …    | Embedded PDF bytes (until last `endobj`+6)  |

`P` is the "PDF start pointer" and `Q` is the actual PDF start. The indirection
exists because the original container can grow or be patched without rewriting
all internal offsets. The Rust parser reads `P`, then `Q` from offset `P`,
and uses `Q` as the start of the PDF blob. The PDF blob runs up to the byte
just past the last `endobj` keyword in the file.

### 3.2 Outline entry (308 bytes = 0x134)

| Sub-offset | Size | Field                                          |
|------------|------|------------------------------------------------|
| 0x000      | 256  | Title bytes, GBK-encoded, NUL-terminated        |
| 0x100      | 24   | Unknown                                        |
| 0x118      | 12   | Page number, ASCII, NUL-terminated             |
| 0x124      | 12   | Unknown                                        |
| 0x130      | 4    | Level (i32 LE), 1 = top-level                  |

`page` is 1-based. Titles are transcoded from GBK to UTF-8 by the parser.

## 4. HN format

HN is the most complex of the supported formats. It carries a stream of
zlib-compressed text records and an array of JBIG/JBIG2/JPEG image blocks per
page.

### 4.1 Header layout (with-TOC variant, magic `HN\0\0`)

| Offset | Size | Field                                          |
|--------|------|------------------------------------------------|
| 0x00   | 4    | Magic `b"HN\0\0"` (GBK-decoded "HN")           |
| 0x90   | 4    | Page count (i32 LE)                            |
| 0x158  | 4    | Outline entry count N (i32 LE)                 |
| 0x15C  | …    | N × 308-byte outline entries                   |
| end     | …    | Page-info table (20 bytes × page_count)        |
| end     | …    | Page data: text + image blocks                 |

The outline entry layout is the same 308-byte format as CAJ (§ 3.2). The
page-info struct layout is described in § 4.3.

### 4.2 Header layout (no-TOC variant, magic `HN\xc8\x00`)

| Offset | Size | Field                                          |
|--------|------|------------------------------------------------|
| 0x00   | 4    | Magic `b"HN\xc8\x00"`                          |
| 0x90   | 4    | Page count (i32 LE)                            |
| 0xD8   | …    | Page-info table (20 bytes × page_count)        |
| end     | …    | Page data: text + image blocks                 |

No outline in this variant; the page-info table starts immediately after the
header at 0xD8.

### 4.3 PageInfo struct (20 bytes)

| Offset | Size | Field                       | Notes                            |
|--------|------|-----------------------------|----------------------------------|
| 0x00   | 4    | `page_data_offset` (i32 LE) | start of this page's data        |
| 0x04   | 4    | `size_of_text_section`      | bytes of the text section        |
| 0x08   | 2    | `images_per_page` (i16 LE)  | may be negative ⇒ 0              |
| 0x0A   | 2    | `page_no` (i16 LE)          | 1-based page number              |
| 0x0C   | 2    | `unk2` (i16 LE)             | unknown                          |
| 0x0E   | 2    | `_pad` (i16 LE)             | unknown, padding                 |
| 0x10   | 4    | `next_page_data_offset`     | start of *next* page's data      |

`next_page_data_offset > page_data_offset` is the Python parser's
"old-style" flag (see `cajparser.py:334`). It determines which dispatch
record format the page's text section uses.

### 4.4 Per-page layout

Each page's data section is a flat concatenation of:

1. **Text section** (`size_of_text_section` bytes)
2. **`images_per_page` image blocks**, each consisting of:
   * 12-byte image header (described below)
   * `size_of_image_data` bytes of image payload

The text section is either raw or zlib-compressed. The on-disk shape
discriminates the two:

* Bytes 0..12 == `b"COMPRESSTEXT"` ⇒ compressed; the 4-byte expanded
  size is at offset 12, and the zlib stream starts at offset 16.
* Bytes 8..20 == `b"COMPRESSTEXT"` ⇒ compressed; the 4-byte expanded
  size is at offset 20, and the zlib stream starts at offset 24.
* Otherwise ⇒ raw bytes, no decompression.

The decompressed text is a flat stream of 2-byte little-endian dispatch
codes followed by record-specific payloads; see § 6.

### 4.5 Image header (12 bytes)

Each image block on a page is preceded by a 12-byte header:

| Offset | Size | Field                       |
|--------|------|-----------------------------|
| 0x00   | 4    | Image type enum (i32 LE)    |
| 0x04   | 4    | Offset to image data (i32 LE) |
| 0x08   | 4    | Size of image data (i32 LE)|

`offset_to_image_data` is always `current_offset + 12`; if it is not, the
parser raises an "unusual image offset" error (this invariant is checked by
the original Python and by our Rust port).

### 4.6 CNKI image header (48 bytes, for JBIG / JBIG2)

JBIG and JBIG2 image payloads begin with a 48-byte CNKI-private header:

| Offset | Size | Field                              |
|--------|------|------------------------------------|
| 0x00   | 4    | "magic" / version bytes            |
| 0x04   | 4    | `width` (u32 LE)                   |
| 0x08   | 4    | `height` (u32 LE, may be 0)        |
| 0x0C   | 2    | `planes` (u16 LE)                  |
| 0x0E   | 2    | `bits_per_pixel` (u16 LE)          |
| 0x10   | …    | codec-specific data                |

The Rust parser reads the 4-byte width / height at offsets 4 and 8 into
`RawImage::width_px` / `RawImage::height_px` and uses them as the
"declared" dimensions. The decoders in `caj2pdf-jbig1` /
`caj2pdf-jbig2` use the same fields.

## 5. C8 format

C8 is the "compact" HN format. The on-disk differences from HN are:

* Magic byte is `\xc8` (a single byte) instead of `b"HN..."`.
* Page count is at offset 0x08 (vs 0x90 for HN).
* **No outline** – the file does not have a 308-byte outline table.
* The page-info table starts at offset 0x50 (vs 0xD8 for HN no-TOC or
  `0x158 + 4 + 308·toc_count` for HN with-TOC).
* Per-page text sections and image blocks are otherwise byte-identical to
  HN; in particular, the same 20-byte PageInfo struct is used.

The Rust parser dispatches to `hn::read_meta` / `hn::iter_pages` for both
HN and C8, varying only the offsets.

## 6. HN page text dispatch grammar

The text section of a single HN page is a stream of 2-byte little-endian
dispatch codes, each followed by a record-specific payload. The grammar
mirrors `HNParsePage.py:73-95` exactly:

| Code     | Style   | Payload                                                       |
|----------|---------|---------------------------------------------------------------|
| `0x8001` | new     | 4 bytes: GBK char (low, high) + 2 unknown bytes              |
| `0x8001` | old     | newline + 2 unknown bytes + (4 bytes/char)* until `0x80xx`    |
| `0x8070` | old     | 2 unknown bytes + (4 bytes/char)* until `0x80xx`              |
| `0x800A` | both    | 26 bytes: figure position (skipped)                           |
| other    | both    | 4 bytes total (the 2-byte code + 2 unknown bytes)             |

"New" vs "old" is selected by `next_page_data_offset > page_data_offset`
in the page-info struct. In practice, almost all modern files use the new
style; the old style is only seen in legacy documents.

### 6.1 Character encoding

GBK characters are stored as two bytes in the file, **little-endian**
(low byte first, then high byte). When we pass them to the GBK decoder, the
byte order must be reversed: pass `[high, low]` (i.e. `[data[off+1],
data[off]]` for new-style; `[data[off+3], data[off+2]]` for old-style).
This is one of the most common bugs in third-party re-implementations.

A small set of GBK code points are OCR artifacts that the original parser
maps to ASCII control characters:

| Code   | Mapped to |
|--------|-----------|
| 0xA389 | `\t`      |
| 0xA38A | `\n`      |
| 0xA38D | `\r`      |
| 0xA3A0 | ` ` (space) |

Unrecognised GBK code points are emitted as `<0xXXXX>` (a debug
placeholder), matching the Python's `KeyError` fallback.

### 6.2 Figure records

`0x800A` is the "figure position" record. It is 26 bytes long, parsed by
the Python as `struct.unpack("<HHHHHIIII", ...)`:

* 5 × u16: ignore1, offset_x, offset_y, size_x, size_y (in 1/2.473 pixels)
* 4 × u32: int2, int3, int4, int5 (unknown)

The Rust parser skips these 26 bytes (it does not yet reconstruct the
figure position list) and emits no characters for them.

## 7. Image type enum

The 4-byte integer at the start of every image block is one of:

| Value | Codec                              | Notes                                 |
|-------|------------------------------------|---------------------------------------|
| 0     | Custom JBIG1 (`ImageKind::Jbig1`)  | Port of `JBigDecode.cc`               |
| 1     | JPEG (`ImageKind::Jpeg { upside_down: false }`) | right-side-up                |
| 2     | JPEG (`ImageKind::Jpeg { upside_down: true }`)  | upside-down                  |
| 3     | JBIG2 (`ImageKind::Jbig2`)         | Uses `libjbig2dec` via FFI            |

Any other value triggers a "page N unknown image type" error. The Rust
parser stores the data and the declared `(width_px, height_px)` so the
downstream decoders don't have to re-parse the 48-byte CNKI header.

## 8. KDH format

KDH is a deliberately weakened encryption format used by some
superstar/kuandai readers. The "encryption" is a single-byte XOR against a
6-byte passphrase applied cyclically. The 254-byte container header is
discarded.

### 8.1 Key

The passphrase is the ASCII string `FZHMEI` (6 bytes), exposed in the Rust
crate as `caj2pdf_core::convert::KDH_PASSPHRASE`.

### 8.2 Decryption steps

These match `cajparser.py:605-640` line-for-line:

1. Read the whole file into memory.
2. Drop the first 254 bytes (the container header).
3. For each remaining byte at position `i`, XOR it with
   `KDH_PASSPHRASE[i % 6]`.
4. Find the **last** occurrence of the substring `%%EOF` in the
   decrypted bytes. Truncate the output to one byte past that
   occurrence. If no `%%EOF` is found, the original Python raises; the
   Rust port returns the full decrypted blob (the caller is expected to
   be tolerant of slightly damaged KDH files).

The result is a (typically still slightly broken) PDF. Downstream tooling
(`mutool clean`, `lopdf`, our own `caj2pdf-pdf`) is responsible for
repairing the xref table.

## 9. TEB format

TEB is the Apabi reader format from Founder Electronics. We identify it
(`magic == "TEB"`) but do not yet decode it. The integration roadmap
calls for either a TEB-to-PDF pass-through (preserving the original Apabi
container) or a per-page rasterisation, depending on what the user wants.

For now, `convert::convert` returns
`CajError::Unsupported("TEB format not yet implemented")`.

## 10. The `find_redundant_images` heuristic

A common publisher shortcut is to take a 1-up page and emit it as a
N x N grid of identical images, but write each tile N times to the
file (so the file has N² image blocks of equal size). The Python
`utils.find_redundant_images` heuristic catches this:

* Only apply to image counts that are perfect squares between 4 and
  100 (i.e. 2×2, 3×3, ..., 10×10).
* If the image sizes for the second N-tile are identical to the
  first N-tile, the page is a redundant N x N grid.
* Return `(true, N)` (the stride) and the consumer can drop the
  duplicates.

The Rust port lives in `caj2pdf_core::hn::find_redundant_images` with
the same semantics.

## 11. Cross-references into the source

* Layout constants for HN / C8: `crates/core/src/hn.rs` (top of file)
* Layout constants for CAJ: `crates/core/src/caj.rs` (top of file)
* Format detection: `crates/core/src/lib.rs::detect_format_inner`
* High-level conversion: `crates/core/src/convert.rs`
* Tests: `crates/core/tests/format_detection.rs`
