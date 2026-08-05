# Changelog

All notable changes to `caj2pdf-rs` are recorded here. The format is
based on [Keep a Changelog](https://keepachangelog.com/) and the
project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

* Workspace layout with five crates: `caj2pdf-core`, `caj2pdf-jbig1`,
  `caj2pdf-jbig2`, `caj2pdf-pdf`, and the `caj2pdf` CLI.
* Pure-Rust port of the custom JBIG1 decoder (5-context adaptive
  arithmetic coder from `JBigDecode.cc`).
* FFI wrapper for the system `libjbig2dec` (Debian package
  `libjbig2dec0-dev`).
* Format detection for all six known CAJ-family container formats
  (CAJ, HN, C8, KDH, PDF, TEB).
* HN-format per-page text extraction (GBK decoding with the
  CNKI-private 0xA389/0xA38A/0xA38D/0xA3A0 mapping table).
* PDF document assembly and outline injection via `lopdf`.
* `caj2pdf show`, `caj2pdf convert`, `caj2pdf outlines`,
  `caj2pdf text-extract`, and `caj2pdf parse` subcommands.

### Verified on real files

Tested on 10 sample CNKI papers in `/home/dust/work/paper0804/caj/`
(9 KDH + 1 PDF):

| Format | Files | Result |
| --- | --- | --- |
| KDH   | 9 | `convert` produces valid PDFs; `pdftotext` recovers the full Chinese text; `pdftoppm` renders pages correctly |
| PDF   | 1 | Pass-through copy works; output identical to input |
| HN/C8 | 0 | No samples in this dataset; covered by unit tests only |
| CAJ   | 0 | No samples in this dataset; covered by synthetic e2e test |

### Fixed

* `convert_kdh` no longer logs a misleading "xref repair TODO"
  message — the KDH container holds a complete PDF (PDF 1.5+ with
  a cross-reference stream), no repair is needed. Now verifies
  `%PDF-` header and `%%EOF` marker and reports the byte count.
* `extract_pdf` now preserves the xref section when the embedded
  slice contains both `xref` and `%%EOF` (helps callers that wrap a
  complete PDF in a CAJ container; no-op for real CAJ files).

## [0.1.0] - 2026-08-04

Initial release.

