# Architecture

This document describes the module-level architecture of `caj2pdf-rs`.
For the byte-level layout of every supported file format, see
[`format-analysis.md`](format-analysis.md). For the JBIG1 decoder's
design rationale, see [`jbig1-reverse-notes.md`](jbig1-reverse-notes.md).

## High-level data flow

```
                       ┌─────────────────────┐
                       │ .caj / .hn / .c8 /  │
                       │ .kdh / .pdf / .teb  │
                       └──────────┬──────────┘
                                  │
                                  ▼
            ┌─────────────────────────────────────┐
            │  caj2pdf-core                        │
            │  ── format detection (FileFormat)    │
            │  ── page count + TOC (CajDocument)   │
            │  ── per-page raw bytes (Page)        │
            │     ├─ text (HN only)                │
            │     └─ image blocks (RawImage)       │
            └──────────┬──────────────────────────┘
                       │  RawImage
                       ▼
        ┌──────────────────────────────┐
        │  caj2pdf-jbig1  (pure Rust)  │ ◄── non-standard 5-context
        │  caj2pdf-jbig2  (FFI)        │     arithmetic coder
        │  built-in JPEG SOF parser    │
        └──────────┬───────────────────┘
                   │  DecodedImage
                   ▼
        ┌──────────────────────────────┐
        │  caj2pdf-pdf (lopdf)         │
        │  ── build_document()         │  1-bpp / DCT image XObjects
        │  ── inject_outlines()        │  BTree → PDF /Outlines dict
        └──────────┬───────────────────┘
                   │  Vec<u8>
                   ▼
              ┌───────────┐
              │  .pdf     │
              └───────────┘
```

## Crate boundaries

| Crate | Responsibility | Public API (top-level) |
|---|---|---|
| `caj2pdf-core` | Format detection, page count, TOC, per-page text dispatch | `CajDocument::open`, `CajDocument::format`, `CajDocument::page_count`, `CajDocument::toc`, `CajDocument::pages`, `CajDocument::extract_pdf`, `convert::convert`, `convert::decrypt_kdh` |
| `caj2pdf-jbig1` | Pure-Rust port of the custom 5-context JBIG1 decoder | `decode(&[u8], u32, u32) -> JbigResult<Bitmap>` |
| `caj2pdf-jbig2` | FFI wrapper around the system `libjbig2dec` | `decode(&[u8], u32, u32) -> Jbig2Result<Bitmap>` |
| `caj2pdf-pdf` | PDF document assembly and outline injection | `build_document(&[PageInput], &[OutlineEntry]) -> PdfResult<Vec<u8>>`, `inject_outlines(&[u8], &[OutlineEntry]) -> PdfResult<Vec<u8>>` |
| `caj2pdf` (CLI) | Argument parsing, subcommand dispatch, orchestration | binary only |

The crates form a strict DAG: `cli → {core, jbig1, jbig2, pdf} → core`. There
are no cycles, and no crate depends on the `cli` binary.

## Why this layout?

* **Core is at the bottom** because every other crate needs the data
  model. Putting it in its own crate prevents the JBIG/JBIG2/PDF crates
  from pulling in I/O and format-detection code they don't need.
* **jbig1 and jbig2 are separate crates** because they have radically
  different dependency stories: `jbig1` is pure Rust, `jbig2` requires
  the system `libjbig2dec`. A consumer who only needs JBIG1 shouldn't
  pay the FFI / pkg-config cost.
* **`pdf` is its own crate** so it can be reused independently of the
  CAJ toolchain — e.g. by a future "build PDF from image folder" tool.
* **CLI is just glue** — keeping it in its own binary crate means the
  libraries can be used by other Rust programs without dragging in
  `clap`.

## Error handling

* Every crate defines its own `*Error` enum with `thiserror`.
* `cli::main` uses `anyhow::Result` and converts with `?` / `.context()`.
* Internal `Result` types are explicit (no `unwrap()` in library code).

## Threading

The current implementation is single-threaded. The JBIG1 and JBIG2
decoders are pure functions and could be parallelized at the page
level via `rayon` if needed for very large documents, but for the
typical thesis (50–500 pages) the single-threaded performance is
already faster than the original Python implementation.

## Testing strategy

* **Unit tests** in each crate test the public API against synthetic
  inputs (e.g. constructed 16-byte headers, hand-crafted JBIG1 patterns).
* **Integration tests** in `crates/*/tests/*.rs` test cross-crate flows.
* **End-to-end tests** require real CAJ files. The project does not
  ship test data (CNKI files are copyrighted); the `tests/README.md`
  documents how to run them locally.

## Logging

All crates use `tracing`. The CLI initializes a `tracing-subscriber`
with `RUST_LOG` (default `info`). Per-page progress is logged at
`info`, JBIG/JBIG2 codec warnings at `warn`, full per-byte trace
output at `trace` (rarely needed).
