# Development guide

How to set up a development environment, build, test, and contribute to
`caj2pdf-rs`.

## Prerequisites

* **Rust 1.74 or later** (the project uses edition 2021 and a few
  newer stable features). Install via [rustup](https://rustup.rs).

No C compiler, no system libraries, no `pkg-config`. Every codec
(JBIG1, JBIG2, GBK, zlib, PDF) is implemented in pure Rust, so the
toolchain is just `cargo build` and you're done.

## Initial build

```bash
git clone https://github.com/duststarr/caj2pdf-rs.git
cd caj2pdf-rs
cargo build
```

The first build will take a few minutes as it compiles all
dependencies (`lopdf`, `clap`, `encoding_rs`, `flate2`, etc.).

## Running tests

```bash
# Run all tests in the workspace
cargo test --workspace

# Run tests for a single crate
cargo test -p caj2pdf-jbig1
cargo test -p caj2pdf-core
cargo test -p caj2pdf-pdf
cargo test -p caj2pdf-jbig2

# Run with logging
RUST_LOG=info cargo test --workspace -- --nocapture
```

## Linting

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

## Branching model

The project uses a slightly relaxed [git-flow](https://nvie.com/posts/a-successful-git-branching-model/):

* `master` — always green, always releasable. Only fast-forwarded from `develop`.
* `develop` — integration branch. Feature branches merge in here.
* `feature/<name>` — one feature, one crate (or a small set of related
  changes). Branch from `develop`, merge back to `develop` when done.
* `hotfix/<name>` — urgent fixes that bypass `develop` and go straight
  to `master`.

In a single-developer / single-session context we collapse the merge
to `develop` and the final release to `master` into one step.

## Commit message conventions

We use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[body]

[footer]
```

Common types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`,
`chore`, `perf`. The scope is usually the crate name
(`core`, `jbig1`, `jbig2`, `pdf`, `cli`).

Examples:

```
feat(jbig1): port custom 5-context arithmetic coder
fix(pdf): outline /Parent links for top-level entries
docs(format-analysis): document HN page-info struct
test(core): add format-detection tests for all 6 formats
```

## Adding a new file format

1. Add a variant to `FileFormat` in `crates/core/src/lib.rs`.
2. Implement the format detection rule in `detect_format`.
3. Add a `Layout` variant if the page-data layout differs.
4. Implement the meta-reader in a new `crates/core/src/<format>.rs`
   module and call it from `CajDocument::read_page_count_and_toc`.
5. Implement the per-page reader if the format has per-page image blocks.
6. Add the conversion branch in `crates/core/src/convert.rs`.
7. Add format-detection tests in `crates/core/tests/`.
8. Document the format in `docs/format-analysis.md`.

## Adding a new image codec

1. Create a new crate (e.g. `crates/jbigx`).
2. Add it to the workspace `Cargo.toml` and `crates/cli/Cargo.toml`.
3. Re-export the decode function from `caj2pdf-core::ImageKind` if
   the new codec is encountered in any known format.
4. Wire the call site in `crates/cli/src/main.rs::cmd_convert`.

## Release process

1. Bump versions in `Cargo.toml` and every `crates/*/Cargo.toml`
   (we keep them in lock-step).
2. `cargo build --release` and run the smoke test with a real CAJ file.
3. Tag with `vX.Y.Z` on `master`.
4. `git push --tags`.

## Building the desktop GUI

```bash
cargo build --release -p caj2pdf-gui
```

* **Linux**: needs X11 or Wayland runtime libraries. The `wayland` and
  `x11` eframe features are both enabled by default, so it works on
  both. No `libgtk`, no `libwebkit`.
* **macOS**: no system deps. For a universal binary, build with
  `cargo build --release --target universal2-apple-darwin -p caj2pdf-gui`.
* **Windows**: no system deps. The default MSVC CRT is statically
  linked.

GUI tests are not run in CI (eframe needs a display); CI only
compiles the crate. To run the GUI locally:

```bash
./target/release/caj2pdf-gui
```

The window opens, accepts dragged `.caj`/`.hn`/`.c8`/`.kdh`/`.pdf`
files, and shows a status row per file (pending → running → done /
error). "Convert all" spawns one thread per pending file; the UI
stays responsive.

## Where to ask questions

* Open a GitHub issue.
* For reverse-engineering questions about the CAJ container format,
  the original `caj2pdf` project's [Wiki](https://github.com/JeziL/caj2pdf/wiki)
  is the most complete public source.

## Code style

* 4-space indents (rustfmt default).
* No `unsafe` in library code unless absolutely necessary (the JBIG2
  FFI module is the only place it should appear, and only inside
  `extern "C"` wrappers).
* All public items get `///` doc comments.
* Errors via `thiserror`; `Result<T, MyError>` everywhere — no
  `Box<dyn Error>` in library code.
* Logging via `tracing`, never `println!` / `eprintln!` in library code.
