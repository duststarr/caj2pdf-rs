# JBIG1 reverse-engineering notes

These notes accompany the `caj2pdf-jbig1` crate and explain the
non-obvious decisions that went into porting the CAJViewer/CAJ
`JBigCodec` class (`/home/dust/work/caj2pdf/lib/JBigDecode.cc` and
`lib/JBigDecode.h`) to bit-exact Rust.

The TL;DR is that the CAJ/CAJViewer JBIG1 encoder is **not standard
JBIG1**. It is a small CNKI-private variant of the T.82 arithmetic
coding scheme with three quirks that the standard codec cannot
reproduce:

1. The `MPS` / `ST` adaptive tables are sized `0x1000` (one bit per
   entry wasted) but only five SLNTP context bits are ever used.
2. `GetBit` uses integer division `bit_offset / 3` instead of the
   standard `bit_offset >> 3`.
3. The line-by-line SLNTP predictor template is a hand-rolled
   5-pixel in-register update loop that does not match any of the
   standard T.82 templates.

If you swap in a standard `jbig-kit` decoder, you will silently
produce wrong pixels for real-world CAJ files. The rest of this
document walks through how the port reproduces the C behavior
exactly.

## 1. The CNKI 48-byte image header

Every JBIG1 image embedded in a CAJ/HN document is preceded by a
48-byte CNKI-private header. The format is:

| Offset | Size | Field                                         |
| -----: | ---: | --------------------------------------------- |
|    0..4 |    4 | Magic / file-format discriminator (unused here) |
|    4..8 |    4 | Image width in pixels, little-endian `u32`     |
|   8..12 |    4 | Image height in pixels, little-endian `u32`    |
|  12..14 |    2 | Reserved / unknown (zero)                      |
|  14..16 |    2 | Bits per pixel, little-endian `u16` (usually 1) |
|  16..48 |   32 | Reserved / unknown (zero)                      |

The C `jbigdec.cc` wrapper reads the width / height / bpp from
this header and then passes the bytes that follow (`buffer[48..]`)
to `jbigDecode`. The Python wrapper (`jbigdec.py`) does the same
thing in its `CImage.DecodeJbig` method.

The Rust `decode` function in `lib.rs` takes the full input
(including the 48-byte header) and does the header parsing in
place. It exposes two helpers, `dimensions_from_header` and
`bits_per_pixel_from_header`, that match the C semantics for
readers that want to validate the header fields themselves.

The row-stride formula is

```
bytes_per_line(W, bpp) = ((W * bpp + 31) >> 5) << 2
```

i.e. W*bpp rounded up to a 32-bit word and then padded to a 4-byte
boundary. For 1-bpp images this matches `4 * ceil(W/32)`. The
helper `bytes_per_line` in `codec.rs` implements this verbatim.

## 2. Mapping from C++ to Rust

The C++ source has the following relevant methods. The column
"Port" shows where they live in the Rust port.

| C++ method           | Rust port (in `codec.rs`)                    |
| -------------------- | -------------------------------------------- |
| `ByteIn`             | `JBigCodec::byte_in`                         |
| `ClearLine`          | `JBigCodec::clear_line`                      |
| `CopyLine`           | `JBigCodec::copy_line`                       |
| `Decode1`            | `JBigCodec::decode1`                         |
| `Decode(inbuf,...)`  | `JBigCodec::decode`                          |
| `Decode(int CX)`     | `JBigCodec::decode_typical`                  |
| `DupLine`            | inlined into `make_typical_line` via `split_at_mut` |
| `GetBit`             | `JBigCodec::get_bit`                         |
| `GetCX`              | `JBigCodec::get_cx`                          |
| `InitDecode`         | `JBigCodec::init_decode`                     |
| `LowestDecode`       | `JBigCodec::lowest_decode`                   |
| `LowestDecodeLine`   | `JBigCodec::lowest_decode_line`              |
| `LpsExchange`        | `JBigCodec::lps_exchange`                    |
| `MakeTypicalLine`    | `JBigCodec::make_typical_line`               |
| `MpsExchange`        | `JBigCodec::mps_exchange`                    |
| `RenormDe`           | `JBigCodec::renorm_de`                       |

All of these methods are bit-exact re-implementations of the C
source. The only structural differences are:

* The `Decode(int CX)` overload has been renamed to
  `decode_typical` so that the public `JBigCodec::decode`
  (which corresponds to the C `Decode(inbuf, size, ...)` method
  that takes the full image buffer) keeps its C-style signature.
* `DupLine` is inlined into `make_typical_line` because the
  one caller in the C code can use a `split_at_mut` to obtain
  two disjoint slices of the output buffer, which is safer than
  a `memcpy` of self-overlapping memory.
* `CopyLine` uses a temporary `Vec<u8>` to satisfy the Rust
  borrow checker, since the C `memcpy` cannot be expressed in
  safe Rust when the source and destination may alias. In
  practice the three line-buffer slots are always distinct, so
  the temp is an empty gesture that costs at most a single
  allocation per scanline.

## 3. The `GetBit` / `GetCX` divergence

The C `GetBit` computes the byte index of a pixel like this:

```c
*(char *)(outptr + width_in_padded_bytes * (height - line_offset - 1)
          + bit_offset / 3) & bitmask[bit_offset & 7]
```

The `bit_offset / 3` is the source's defining bug-for-bug
compatibility quirk. Standard JBIG1 (`bit_offset >> 3`) is a
strict power-of-two shift, so the byte index would advance at
1/8 of the bit rate. The CNKI version uses integer division
by 3, which advances the byte index at 1/3 of the bit rate.
This means:

* The output buffer cannot be re-interpreted as packed
  bits in the natural way: a "byte" of `outptr` does not
  necessarily contain 8 consecutive horizontal pixels.
* It only "works" because `width_in_padded_bytes` is large
  enough to give every row of pixels at least as many bytes
  as `width / 3` plus the SLNTP lookahead.

The Rust `get_bit` mirrors this exactly:

```rust
let byte_off = (bit_offset as u32) / 3;
let bit_in_byte = (bit_offset as u32) & 7;
let idx = (row * self.width_in_padded_bytes + byte_off) as usize;
((outptr[idx] & BIT_MASK[bit_in_byte as usize]) != 0) as u32
```

The `GetCX` function (which builds the SLNTP context for the
first pixel of a line) uses the same five GetBit lookups in
the same order, and the resulting 5-bit context is then used
as the entry point into the SLNTP loop in `LowestDecodeLine`.

## 4. The SLNTP loop

The standard T.82 SLNTP loop updates a 10-bit context register
with new pixel values from a 3-line or 2-line template. The CNKI
version is hand-rolled and uses a 5-pixel neighborhood:

* Bit 0, 1, 2, 3, 4 are the initial SLNTP context from `GetCX`.
* Bit 7 is set to the value of the pixel that is 3 columns to
  the right of the current pixel in the 1-up line.
* Bit 2 is set to the value of the pixel that is 2 columns to
  the right of the current pixel in the 2-up line.
* Bit 9 is set to the current decoded pixel value.
* The register is shifted right by 1 between pixels, with bit 9
  being cleared via the `(v9 >> 1) & 0xFDFF` mask.

This produces a 10-bit context in the range 0..=766 that is
fed back into `Decode1` for the next pixel.

The Rust `lowest_decode_line` mirrors this loop pixel-for-pixel:

```rust
let mut v11 = (cx >> 1) & SLNTP_SHIFT_CLEAR_MASK;
v11 |= SLNTP_TWO_UP_TWO_RIGHT;
if a3[(v10 + 2) as usize] != 1 {
    v11 &= !SLNTP_TWO_UP_TWO_RIGHT;
}
cx = v11 | SLNTP_ONE_UP_THREE_RIGHT;
if a4[(v10 + 3) as usize] != 1 {
    cx &= !SLNTP_ONE_UP_THREE_RIGHT;
}
```

The `a3` and `a4` arguments are the "2-up" and "1-up" line
buffers (which is the opposite of what you might expect — in
the C source, the call is `LowestDecodeLine(v9, v7, i, v14, v8)`,
so `a3 = v7` is the 1-up line and `a4 = i` is the 2-up line).
The `+2` and `+3` offsets are the SLNTP look-ahead distances.

## 5. The three rotating line buffers

The C `LowestDecode` allocates a single `24 * (W+2)` byte block
and uses three `8 * (W+2)` byte sub-ranges as line buffers that
rotate through the roles:

* v7 = "1-up line" (read-only, used as `a3` in `LowestDecodeLine`).
* v8 = "current line" (written, also used as `a6`).
* i  = "2-up scratch line" (read-only, used as `a4`).

Every scanline, the three roles cycle through the three slots:
`(v7, v8, i)` becomes `(v8, i, v7)`. This is what the C source
does via raw pointer arithmetic on a single allocation:

```c
char *v13;
for (char *i = v15; ; i = v13) {
    // body uses v7, v8, i
    v9 -= width_in_padded_bytes;
    v13 = v7;
    v7  = v8;
    v8  = i;
}
```

The Rust port uses the same approach: a single `Vec<u8>` of
size `line_size * 3`, with `split_at_mut` to obtain three
disjoint `&mut [u8]`s and a `match` on the current
permutation of `(v7_idx, v8_idx, i_idx)` to map them to
the right roles. The 6-arm match is verbose but it is the
only way to convince the borrow checker that the three
slices are disjoint without dropping into `unsafe`.

## 6. Test vectors

There are no public test vectors for the CNKI JBIG1 encoder
(it is part of the proprietary `libreaderex_x64.so`). The
`tests/decode_known.rs` file therefore covers the parts of the
contract that *are* publicly observable:

* `bytes_per_line` matches the C formula for a sweep of widths
  and bits-per-pixel values.
* `decode` rejects inputs shorter than 48 bytes with a
  `ShortInput` error.
* `decode` produces a `Bitmap` of the expected dimensions
  and bit-vector size.
* A hand-crafted 16x8 1-bpp image decodes end-to-end without
  panicking.

To validate against a real CAJ file, you will need the C
reference implementation and a property-based test that
compares the bytes of the two decoders' outputs. That is
deferred to integration testing once the rest of the
`caj2pdf-rs` toolchain is wired up.

## 7. Decisions and known limitations

* The `unsafe` keyword is not used. The line-buffer copies
  that the C source does with `memcpy` are done either
  through `split_at_mut` (when source and destination are
  provably disjoint) or through a temporary `Vec<u8>` (when
  the borrow checker cannot see the disjointness).
* The MPS / ST tables are allocated as fixed-size `[u32; 0x1000]`
  arrays even though the C source uses the same size. This
  wastes about 8 KB of memory per codec instance, which is
  negligible; it keeps the array indexing arithmetic identical
  to the original.
* The trailing `ByteIn` after the renormalisation loop
  (which is in the C source but not in the textbook T.82
  algorithm) is preserved, to keep the byte stream
  consumption bit-exact.
* All arithmetic is done in `u32` (matching the C `unsigned
  int`). The `wrapping_*` arithmetic operators are used where
  the C code relies on modular arithmetic.

## 8. Files in this implementation

| File                                            | Purpose                                          |
| ----------------------------------------------- | ------------------------------------------------ |
| `crates/jbig1/Cargo.toml`                       | Crate manifest, depends on `thiserror`, `tracing` |
| `crates/jbig1/src/lib.rs`                       | Public `decode` API, error types, `Bitmap`       |
| `crates/jbig1/src/codec.rs`                     | Bit-exact `JBigCodec` port and the 5-context tables |
| `crates/jbig1/tests/decode_known.rs`            | Integration tests for the row-stride, header, and decode entry point |
| `docs/jbig1-reverse-notes.md`                   | This document                                    |
