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
