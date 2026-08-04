# JBIG2 decoder notes

`caj2pdf-jbig2` is a thin safe-Rust wrapper over the system
[`libjbig2dec`](https://github.com/ArtifexSoftware/jbig2dec) C
library.  This document records the CAJ-specific quirks that the
wrapper has to honour, plus the FFI calling convention we settled
on.

## 1. The CNKI `CImage` 48-byte header

Every JBIG2 image inside a CAJ/HN container is prefixed with a
48-byte private header that CAJViewer adds before handing the
buffer to the system codec.  Python's `jbig2dec.py` parses it as:

```python
(self.width, self.height,
 self.num_planes, self.bits_per_pixel) = struct.unpack("<IIHH", buffer[4:16])
self.bytes_per_line = ((self.width * self.bits_per_pixel + 31) >> 5) << 2
```

For 1-bpp images the layout (with all offsets from the start of
the buffer) is:

| Offset | Size | Field            | Notes                                 |
|-------:|-----:|------------------|---------------------------------------|
|   0..3 |    4 | unknown          | Always `0x00 00 00 00` in real files. |
|   4..7 |    4 | `width`          | little-endian `u32`, pixels.          |
|  8..11 |    4 | `height`         | little-endian `u32`, pixels.          |
| 12..13 |    2 | `num_planes`     | little-endian `u16`, `1` in practice. |
| 14..15 |    2 | `bits_per_pixel` | little-endian `u16`, `1` in practice. |
| 16..47 |   32 | unknown          | Always zero in real files.           |

Bytes 48..end are the actual JBIG2 segment stream that gets fed
to `jbig2dec`.

We do **not** parse any of these fields; the caller passes
`width` and `height` in explicitly, and the wrapper just skips
the first 48 bytes before calling `jbig2_data_in`.  This matches
the reference C wrapper in `caj2pdf/lib/decode_jbig2data_x.cc`,
which also ignores the header.

## 2. `bytes_per_line` and `width_in_bytes`

The two derived values from `jbig2dec.py` are:

```
bytes_per_line = ((width * bits_per_pixel + 31) >> 5) << 2
width_in_bytes = (width  * bits_per_pixel + 7)  >> 3
```

For 1-bpp images these reduce to:

```
bytes_per_line = ((width + 31) / 32) * 4      # 4-byte aligned
width_in_bytes = (width  + 7)  / 8            # 1-byte aligned
```

`bytes_per_line` is the **stride** of the output buffer (how far
to jump from the start of one row to the start of the next),
while `width_in_bytes` is the **width** of a single row of pixel
data.  In the reference C wrapper, `bytes_per_line` is used to
allocate the PBM-compatible output buffer and `width_in_bytes`
is used as the `memcpy` length when copying a row out of the
`Jbig2Image`.

In the Rust wrapper we use the `Jbig2Image.stride` field exposed
by jbig2dec instead of `bytes_per_line`, and we use
`((width + 7) / 8)` (the same as `width_in_bytes`) for the row
length.  The Python formulas are kept in `lib.rs::bytes_per_line`
/ `width_in_bytes` for parity / documentation purposes - they
are computed but not used for any allocation.  The values they
produce are guaranteed to match `stride` (which is always
4-byte aligned) and `row_bytes()` (which is always 1-byte
aligned), respectively.

## 3. FFI calling convention

We bind the lower-level `jbig2_ctx_*` API rather than the
higher-level `jbig2_decode_generic` (which only exists in
jbig2dec ≥ 0.20).  The exact call sequence is:

1. `jbig2_ctx_new_imp(NULL, JBIG2_OPTIONS_EMBEDDED, NULL, NULL,
   NULL, JBIG2_VERSION_MAJOR, JBIG2_VERSION_MINOR)` to build a
   fresh decoder context.  We pass the version constants
   explicitly so that jbig2dec's own version guard fires if the
   user is running against a future ABI break.

2. `jbig2_data_in(ctx, stream.as_ptr(), stream.len())` to feed
   the JBIG2 segment stream.  An empty stream is a no-op.

3. `jbig2_complete_page(ctx)` to simulate an end-of-page
   segment.  This is required for "broken CVision embedded
   streams" (see the comment in the reference C wrapper).

4. `jbig2_page_out(ctx)` to pop the decoded `Jbig2Image`.
   Returns `NULL` if no page was decoded (we surface that as
   `Jbig2Error::Decode(-2)`).

5. Copy the image data into a tightly-packed `Vec<u8>` row by
   row, dropping the per-row padding bytes that jbig2dec may
   have appended to reach its 4-byte-aligned `stride`.

6. `jbig2_release_page(ctx, image)` then `jbig2_ctx_free(ctx)`
   to release all jbig2dec-owned state.

All `unsafe` blocks in `decode_inner` carry a `// SAFETY:`
comment explaining why the pointers are valid and which
invariants the C library promises.

## 4. Error mapping

The public `Jbig2Error` enum only has three variants
(`Library`, `Decode(i32)`, `ShortInput`).  Internally we map
the various failure modes to `Decode(i32)` with distinct
negative codes:

| Code  | Meaning                                                |
|------:|--------------------------------------------------------|
|   -1  | `jbig2_data_in` reported a fatal error.                |
|   -2  | `jbig2_page_out` returned NULL (no page decoded).      |
|   -3  | Decoded page dimensions disagree with the caller's.    |
|  rc<0 | `jbig2_complete_page` returned a non-zero status.      |

`Library` is reserved for "could not load jbig2dec at runtime"
/ "out of memory in `jbig2_ctx_new_imp`".  `ShortInput` covers
buffers that don't even have the 48-byte CNKI header.

## 5. Installing `libjbig2dec`

The crate looks for `jbig2dec` via `pkg-config` at build time.
The relevant packages are:

* **Debian / Ubuntu**:
  ```sh
  apt install libjbig2dec0-dev
  ```
  ships `jbig2dec.pc` to
  `/usr/lib/x86_64-linux-gnu/pkgconfig/` and `jbig2.h` to
  `/usr/include/`.

* **Fedora / RHEL**:
  ```sh
  dnf install jbig2dec-devel
  ```

* **Arch**:
  ```sh
  pacman -S jbig2decdec
  ```

* **Homebrew (macOS)**:
  ```sh
  brew install jbig2dec
  ```
  (you may also need to set
  `PKG_CONFIG_PATH=/opt/homebrew/lib/pkgconfig` on Apple
  Silicon).

If you install jbig2dec into a non-standard prefix, point
`pkg-config` at it:

```sh
PKG_CONFIG_PATH=$PREFIX/lib/pkgconfig cargo build -p caj2pdf-jbig2
```

If `pkg-config` cannot find `jbig2dec` at all, the linker will
still try `-ljbig2dec` (the build script always emits the
directive) and the build will fail with an unresolved-symbol
error.  In that case, run `pkg-config --modversion jbig2dec` to
see which `jbig2dec.pc` is being picked up.

## 6. Verified against

* `jbig2dec` 0.19 (Debian package `libjbig2dec0-dev`
  0.19-3ubuntu0.1 on Ubuntu 22.04 Jammy).
* The reference C wrapper
  `caj2pdf/lib/decode_jbig2data_x.cc` (Hin-Tak Leung, 2021).

The high-level `jbig2_decode_generic` function is **not**
available in jbig2dec 0.19; this crate does not depend on it.
