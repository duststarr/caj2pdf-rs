# caj2pdf-rs

[![License: GPL-2.0-or-later](https://img.shields.io/badge/License-GPL--2.0--or--later-blue.svg)](LICENSE)
[![Rust: 1.74+](https://img.shields.io/badge/rust-1.74%2B-orange.svg)](https://www.rust-lang.org)

A pure-Rust reimplementation of the [caj2pdf](https://github.com/JeziL/caj2pdf)
toolchain for converting Chinese academic journal (CAJ / HN / C8 / KDH) files
to PDF.

## Why

The CNKI / 知网 academic database distributes some theses and dissertations
in a proprietary `.caj` format that only opens in their CAJViewer software,
which is Windows-only. The print-to-PDF trick CAJViewer offers loses both
text-selectability and the document's outline (bookmark) tree.

This tool reads the CAJ / HN / C8 / KDH containers directly, decodes the
embedded JBIG1, JBIG2, and JPEG image streams, and produces a clean PDF
with the original outline tree preserved.

## What this project does

* Reads all six known CAJ-family container formats: **CAJ, HN, C8, KDH, PDF, TEB**.
* Extracts and decodes the per-page image streams (JBIG1, JBIG2, JPEG).
* Extracts the document's outline (table of contents) tree.
* Extracts the per-page plain text (HN format only).
* Writes a fresh PDF file with the outline tree, page text overlays, and
  the decoded images, using the well-tested `lopdf` crate.

## What this project does *not* do (yet)

* **TEB** (Apabi) format — detected but not converted.
* **Text re-flow** — HN pages have text laid out as positioned glyphs, but
  this project only extracts the linear text; the PDF shows it as an image
  with an invisible text overlay (selectable but not positioned).
* **Encrypted HN** variants — only the KDH XOR key (`"FZHMEI"`) is handled.

## Quick start

### Build

```bash
# Debian / Ubuntu: install the JBIG2 system library
sudo apt install libjbig2dec0-dev

git clone https://github.com/duststarr/caj2pdf-rs.git
cd caj2pdf-rs
cargo build --release
```

The binary lands at `target/release/caj2pdf`.

### Use

```bash
# Show file info
caj2pdf show thesis.caj

# Convert to PDF (output path optional; defaults to <input>.pdf)
caj2pdf convert thesis.caj -o thesis.pdf

# Extract text only (HN files)
caj2pdf text-extract thesis.hn

# Add the outline of a CAJ file to a PDF you printed from CAJViewer
caj2pdf outlines thesis.caj -o printed.pdf
```

## Project layout

```
crates/
  core/   format detection, CAJ/HN parsing, page iteration
  jbig1/  pure-Rust port of the custom JBIG1 decoder (port of JBigDecode.cc)
  jbig2/  FFI wrapper around the system libjbig2dec library
  pdf/    lopdf-based PDF assembly and outline injection
  cli/    clap-based CLI binary
docs/
  architecture.md
  format-analysis.md
  jbig1-reverse-notes.md
  jbig2-notes.md
  pdf-assembly.md
  development.md
```

## Why a from-scratch port instead of `caj2pdf` + ctypes?

The original `caj2pdf` (Python) loads two custom-built shared libraries
(`libjbigdec.so`, `libjbig2codec.so`) that wrap reverse-engineered
code from CNKI's proprietary `libreaderex_x64.so`. The JBIG1 codec
in particular uses a **non-standard 5-context adaptive arithmetic
coder** (the standard T.82 codec uses 14 contexts), so a "drop-in
standard jbig-kit" replacement would silently produce wrong pixels.

This project ports the JBIG1 codec to pure safe Rust, binds JBIG2
through the standard `libjbig2dec`, and uses `lopdf` for PDF
construction — replacing the original tool's ~5,000 lines of Python
and ~500 lines of C/C++ with a single, statically linkable Rust
binary.

See [`docs/jbig1-reverse-notes.md`](docs/jbig1-reverse-notes.md) for the
line-by-line port and [`docs/format-analysis.md`](docs/format-analysis.md)
for the on-disk layout of every supported format.

## Testing

```bash
# Unit tests for every crate
cargo test --workspace

# Run with a real CAJ file (you supply the test data)
./target/release/caj2pdf show /path/to/test.caj
./target/release/caj2pdf convert /path/to/test.caj -o /tmp/out.pdf
```

## License

GPL-2.0-or-later, matching the original `caj2pdf` project's intent.
The JBIG1 codec port is derived from `JBigDecode.cc` (Copyright
2020-2021 Hin-Tak Leung, FreeType Project License) — see
[`docs/jbig1-reverse-notes.md`](docs/jbig1-reverse-notes.md) for
attribution.

## Acknowledgments

* **Hin-Tak Leung** — original reverse engineering of the CAJ
  container and the custom JBIG1 codec.
* **The original `caj2pdf` team** — `JeziL` and contributors, for
  documenting the format and providing the reference Python code.
* **`img2pdf` (Johannes 'josch' Schauer)** — the original PDF
  writer logic that the Python tool forked.
