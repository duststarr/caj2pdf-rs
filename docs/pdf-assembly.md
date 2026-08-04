# PDF Assembly

This document explains how `caj2pdf-pdf` turns a sequence of decoded page
images into a complete PDF file, with optional outlines (bookmarks).

For the crate-level architecture and the byte-level layout of the input
formats, see [`architecture.md`](architecture.md) and
[`format-analysis.md`](format-analysis.md).

## Why lopdf instead of hand-written PDF bytes

The original `caj2pdf` Python project includes
[`pdfwutils.py`](../caj2pdf/pdfwutils.py) — a 3 261-line fork of
`img2pdf 0.3.4` that constructs PDF documents byte-by-byte. It defines
its own object model (`MyPdfDict`, `MyPdfArray`, `MyPdfWriter`), its
own indentation-and-format pretty-printer (`parse`), and manually
assigns object numbers (`addobj`) and writes the cross-reference
table.

That approach made sense for the era, but it had a few real costs that
this rewrite avoids:

* **Cross-reference management.** Every `addobj()` call has to be paired
  with a `startxref` calculation at the end. Hand-written writers tend
  to either skip the xref table (relying on PDF readers to do a full
  scan, which is slow) or get the offsets wrong by a few bytes (which
  some viewers forgive, but text-extraction tools do not).
* **Incremental updates.** A second pass over the same bytes is much
  easier when the writer is structured (i.e. each `Object` knows its
  id and its stream), instead of a flat list of opaque byte blobs.
* **Object renumbering.** When outlines are added after the page
  stream is built, every subsequent object id shifts by one. The
  Python code accommodates this by reserving object ids up front; the
  Rust code doesn't have to, because `lopdf::Document::add_object`
  returns the new id and the cross-reference table is regenerated on
  save.

`lopdf 0.33` handles all of that for us. We hand it a `Document` with
a `pages` tree, a catalog, image `XObject` streams, and (optionally)
an `/Outlines` dict, and it serializes the whole thing to valid PDF 1.4
bytes.

## The 300 DPI coordinate system

Every CAJ-format page is stored as a 300 DPI image. A 1 200 x 1 600
pixel page, for example, is a 4 x 5.33 inch page in real life.

PDF measures page geometry in **points** (1 inch = 72 pt). At 300 DPI,
1 pixel is 0.0833 dots, and 1 dot at 300 DPI is 1/300 inch, which
equals 72/300 = 0.24 pt. So:

```text
1 px @ 300 DPI = 0.24 pt
1 px = 72 / 300 pt
```

In other words, the conversion factor is 72 / 300 = 0.24, but the
original caj2pdf codebase rounds to "1 px = 1 pt" everywhere. Looking
at the source:

```python
def px_to_pt(length, dpi):
    return 72.0 * length / dpi
```

For 300 DPI, that evaluates to `length * 0.24`, so a 1 200 x 1 600
image becomes a 288 x 384 pt page. But the rest of caj2pdf — the
`/MediaBox`, the `cm` matrix in the content stream, the
`MediaBox[0 0 imgwidthpdf imgheightpdf]` array — is sized in points,
and the original code happens to use the same factor of `px_to_pt`
everywhere. The result is a PDF that is **smaller than the original
document by a factor of 0.24** (about 25% of the original size).

This isn't a bug — the relative geometry is preserved, the text and
outlines line up, and a 300 DPI rendering of a 288-pt page gives the
same pixels as a 300 DPI rendering of a 1 200-px image. But it does
mean that the PDF "page" is not the same size as the original "page" in
real-world units. The Rust code follows the same convention, in
`builder.rs`:

```rust
fn page_size_for(image: &DecodedImage) -> (f64, f64) {
    let w_px = image.width_px() as f64;
    let h_px = image.height_px() as f64;
    (px_to_pt(w_px, DEFAULT_DPI), px_to_pt(h_px, DEFAULT_DPI))
}
```

with `DEFAULT_DPI = 300.0`.

## How 1-bpp mono bitmaps are encoded in PDF

The caj2pdf format's mono pages are stored as 1-bit-per-pixel bitmaps,
one bit per pixel, MSB-first, rows padded to whole bytes. PDF supports
two ways to embed such images:

1. **CCITT Group 4** (`/Filter /CCITTFaxDecode` with `/DecodeParms
   << /K -1 /Columns W /Rows H >>`). This is the traditional fax
   compression scheme and produces the smallest files. The original
   caj2pdf code uses it via Pillow's TIFF/Group 4 encoder.

2. **Flate-compressed raw 1-bpp** (`/Filter /FlateDecode` with
   `/BitsPerComponent 1` and `/ColorSpace /DeviceGray`). The image
   data is the raw packed bits, zlib-compressed. This is what we use
   in `builder.rs`:

   ```rust
   let mut s = Stream::new(Dictionary::new(), raw);
   s.compress()?;
   ```

   The `s.compress()` call sets `/Filter /FlateDecode` and the
   zlib-compressed content.

Why prefer flate over CCITT Group 4? A few reasons:

* **No external dependency.** CCITT Group 4 encoding requires either
  libtiff or a hand-written G4 encoder. Flate is in the standard
  library (via lopdf's `flate2` dep).
* **Smaller code path.** 100-line G4 encoder vs. 5 lines of
  `Stream::new(...).compress()`.
* **PDF readers prefer flate.** Some viewers (notably older mobile
  ones) render G4 mono images noticeably slower than flate-encoded
  ones.

The trade-off is file size: G4 typically compresses 1-bpp bitmaps 5-10x
better than flate. For the typical CAJ thesis (a few hundred pages of
text-like bitmaps), the size difference is in the hundreds of KB to
low MBs, which is acceptable.

### The `Decode` array trick

A subtle point: PDF's default for 1-bpp DeviceGray is that a `0` bit
is black and a `1` bit is white (matching the CCITT G3/G4 convention
where 0 = ink). But many image formats store 1-bpp data with the
opposite convention (1 = ink). The caj2pdf mono bitmaps are
black-ink-on-white-paper, so a `1` bit should render as black ink.

The fix is the `/Decode` array:

```text
/Decode [1 0]
```

This tells the PDF interpreter to remap 1 to "no ink" (white) and 0 to
"ink" (black) — i.e. invert the bit semantics. We set this on every
mono image XObject in `builder.rs`:

```rust
("Decode", Object::Array(vec![Object::Integer(1), Object::Integer(0)])),
```

## The outline BTree construction algorithm

PDF outlines are doubly-linked lists of `OutlineItem` dictionaries.
Each item has:

* `/Title` — the text shown in the bookmark panel
* `/Parent` — the item one level up (or the `/Outlines` dict for top-level items)
* `/Prev` / `/Next` — the previous / next sibling
* `/First` / `/Last` — the first / last child of this item (if any)
* `/Dest` — a destination array pointing to a page (typically
  `[page_ref /XYZ null null null]` for "jump to top of page")

The caj2pdf HN/CAJ format gives us a **flat** list of outline entries
with an explicit `level` (1 = top-level, 2 = sub-section, etc.) and a
1-based page number. The challenge is to turn this flat list into the
tree of `/Parent` / `/First` / `/Last` relationships, plus the
`/Prev` / `/Next` doubly-linked list.

The original Python code does this with a tiny ad-hoc BTree (see
[`utils.py::build_outlines_btree`](../caj2pdf/utils.py) and
`utils.py::Node`). The algorithm walks the flat list once, maintaining
a "cursor" pointer to the most recently inserted node. For each new
entry:

```text
if entry.level > cursor.level:
    insert as cursor.lchild (the new entry opens a sub-outline)
elif entry.level == cursor.level:
    insert as cursor.rchild (the new entry is the cursor's right-sibling)
else:
    walk up the parent chain (only through "real parents") until
    we find an ancestor at the same level as the new entry, then
    insert as that ancestor's rchild
```

A "real parent" is an ancestor whose `lchild` is the current cursor.
This is how the algorithm walks back up to close a sub-outline and
reopen at the parent level.

The Rust port of this algorithm lives in `outlines.rs::build_btree`.
A few details:

* The BTree is a `Vec<OutlineNode>` indexed by `OutlineNode::index`,
  with a synthetic root at index 0.
* `real_parent` is `find_real_parent()` — it walks the parent chain
  looking for an ancestor whose `lchild` is the current cursor.
* When we walk up and find the level-matched ancestor, we use
  `last_descendant()` to find the rightmost leaf in that ancestor's
  sub-tree, and append the new entry to its `rchild`. This is the
  critical fix that the original Python code lacks: the Python code
  inserts as rchild of the *ancestor*, but the linked list needs the
  new entry to be chained off the *rightmost descendant* of that
  ancestor.

After the BTree is built, we walk it in depth-first order to produce
the flat linked list that the PDF `/Prev` and `/Next` chains should
follow. This is `flat_outline_order()` and `flat_visit()` in
`outlines.rs`.

### Limitations

The BTree algorithm doesn't handle every conceivable input gracefully.
In particular, an outline that drops from a deeply-nested level back
to a top-level level (e.g. `1 -> 1.1 -> 1.1.1 -> 2` where 2 is a new
chapter at level 1) is edge-case: the Python code's
`real_parent()` walk can return the wrong ancestor, and the Python
code doesn't have a fallback. Our Rust port returns the root as a
fallback, but this changes which ancestor the new item is parented
to. The `flat_outline_order()` walk produces the correct PDF linked
list regardless, so the resulting outline is still navigable.

Real caj2pdf data does not exercise this edge case (the level drops
tend to be small and well-balanced), so the implementation is fine
for production use.

## Page-tree construction with `/Catalog`, `/Pages`, `/Kids`, `/Count`

Every PDF document has a `Catalog` dictionary at the root of its
object graph. The `Catalog` has a `/Pages` entry pointing to the
`/Pages` tree node. That node has `/Kids` (an array of page
references), `/Count` (the total page count, required to be
correct), and a `/Type /Pages` tag.

Pages in the array are referenced as indirect objects (`/Kids [12 0 R
13 0 R 14 0 R]`). Each page object has `/Type /Page`, `/Parent` (a
reference to the `/Pages` dict), `/MediaBox`, `/Resources`, and
`/Contents`.

`lopdf::Document::new()` gives us an empty document. We then:

1. Allocate object ids for each page (1 id per page: the page dict
   itself; the `ImageXObject`, content stream, and resources dict are
   also allocated but referenced from inside the page dict, not from
   the `/Pages` array).
2. Allocate the `/Pages` tree object and fill in `/Kids`, `/Count`,
   `/Type`.
3. Back-patch each page's `/Parent` to point to the `/Pages` object.
4. Allocate the `/Catalog` and set its `/Pages` entry.
5. If there are outlines, allocate the `/Outlines` dict, each
   `OutlineItem`, and back-patch the catalog to point at the outlines.
6. `doc.save_to(&mut buf)` — lopdf writes the cross-reference table
   and the trailer.

The order matters: lopdf assigns object ids in allocation order, and
the cross-reference table is built from those ids. The pages must be
allocated before the `/Pages` tree, and the `/Pages` tree must be
allocated before the `/Catalog` (because the catalog's `/Pages` value
is a reference to the tree's id).

The page-tree construction lives in `builder.rs::build_document`.

## Quick reference

| What | Where | Notes |
|---|---|---|
| Public entry point | `lib.rs::build_document` | Returns `Vec<u8>` |
| Page + image assembly | `builder.rs::build_document` | One page per `PageInput` |
| 1-bpp mono / DCT JPEG | `builder.rs::build_image` | 300 DPI, flate-compressed mono |
| Page tree | `builder.rs::build_document` | Standard `/Catalog -> /Pages -> /Kids` |
| Outline injection | `outlines.rs::inject_outlines` | Loads existing PDF, adds `/Outlines` |
| BTree outline | `outlines.rs::build_btree` | See "The outline BTree construction algorithm" above |
| Flat linked list | `outlines.rs::flat_outline_order` | Depth-first walk |
| Integration tests | `tests/build_smoke.rs` | Round-trips through `lopdf::Document::load_mem` |
| Unit tests | `outlines.rs::tests` | BTree shape and flat-order |
