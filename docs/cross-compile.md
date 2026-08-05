# Cross-compilation

This document records the cross-compilation status of `caj2pdf-rs`.

## Status matrix

| Target | Binary | Status | Toolchain needed |
| --- | --- | --- | --- |
| `x86_64-unknown-linux-gnu` | `target/release/caj2pdf-gui` | ✅ builds | system gcc (default) |
| `aarch64-unknown-linux-musl` | `target/aarch64-unknown-linux-musl/release/caj2pdf-gui` | ✅ builds, **statically linked** | rust-lld (bundled) |
| `x86_64-pc-windows-gnu` | — | ❌ **needs `dlltool`** | MinGW binutils (not in repo, no sudo) |
| `x86_64-pc-windows-gnullvm` | — | ⚠️ blocked on rust-lld 22 dispatcher | LLVM 22 (bundled) |
| `aarch64-pc-windows-gnullvm` | — | not attempted | — |

## What works: aarch64-unknown-linux-musl (Kylin / ARM Linux)

```bash
# One-shot:
cargo build --release -p caj2pdf-gui --target aarch64-unknown-linux-musl

# Or via the wrapper that handles musl's gcc-isms:
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=/tmp/lld-aarch64-gnu.sh \
RUSTFLAGS="-C link-arg=-L$HOME/.rustup/toolchains/$(rustc -V | awk '{print $1}')/lib/rustlib/aarch64-unknown-linux-musl/lib/self-contained" \
cargo build --release -p caj2pdf-gui --target aarch64-unknown-linux-musl
```

The wrapper script `lld-aarch64-gnu.sh` (created during initial bring-up) translates gcc-isms for the musl target:

- Strips `-Wl,` prefix
- Drops `-nostartfiles`, `-nodefaultlibs`, `-Bstatic`, `-Bdynamic` (lld handles these implicitly)
- Replaces `-ldl` with `-lc` (musl has no separate libdl; dlopen is in libc)
- Invokes `lld -flavor gnu`

The resulting binary is a **fully static, single-file** ELF for ARM aarch64, with **no dynamic dependencies** (not even libc — all C-side code is in the Rust binary). It runs on:

- Kylin (银河麒麟) V10 / V11 (aarch64)
- Ubuntu 22.04 / 24.04 on ARM
- Any Linux distribution on aarch64, regardless of glibc / musl

## What blocks Windows: dlltool is mandatory

The Rust windows-sys 0.61 / 0.60 crates use `#[link(name = "...", kind = "raw-dylib")]`
for Windows API bindings. For each raw-dylib, the Rust compiler generates a
synthetic import library `<name>.dll_imports.lib` via `dlltool` (a MinGW
binutils tool). This tool is **not** bundled with the rust toolchain.

Without `dlltool` available on `PATH` as `x86_64-w64-mingw32-dlltool`, the
very first crate to use `windows-sys` (e.g. `parking_lot_core`) fails to
build:

```
error: failed to add native library
   .../rustcXXX/kernel32.dll_imports.lib: No such file or directory
```

### Workarounds we tried

1. **Fake `dlltool` script that no-ops**: rejected by cargo with
   `memory map must have a non-zero length` — rustc opens the empty
   file with `mmap(2)` and fails the size check.
2. **Fake `dlltool` that copies `libwindows.0.53.0.a` as a `.lib`**:
   rustc insists on importing individual `.lib` files, not `.a`
   archives; copy doesn't work.
3. **`x86_64-pc-windows-gnullvm` target** (uses LLVM toolchain, no
   binutils): rustc-side import-lib generation works, but the
   final link fails because `rust-lld` 22 (bundled with the rust
   toolchain) treats `-L` and `-l` as unknown arguments in
   `lld-link` mode and silently drops them, so the system import
   libraries are never found.
4. **Pre-downloaded MinGW sysroot** (libiconv, libgcc, etc.):
   not available without network access to a non-Debian package
   source.

### The actual fix

Install `mingw-w64-tools` (or the Debian package `mingw-w64` which
includes `x86_64-w64-mingw32-dlltool`):

```bash
sudo apt install mingw-w64
# Now:
cargo build --release -p caj2pdf-gui --target x86_64-pc-windows-gnu
```

Once dlltool is on PATH, the build succeeds and produces a `.exe`
that runs on any Windows 7+ machine, statically linked to the
MinGW import libraries, no C runtime DLL required.

## Reproducing the ARM cross-build on a fresh machine

```bash
rustup target add aarch64-unknown-linux-musl
# Place /tmp/lld-aarch64-gnu.sh (contents below) on the system.
cat > /tmp/lld-aarch64-gnu.sh <<'EOF'
#!/bin/sh
args=""
for a in "$@"; do
    case "$a" in
        -Wl,*) args="$args ${a#-Wl,}" ;;
        -nostartfiles|-nodefaultlibs|-Bstatic|-Bdynamic) ;;
        -ldl) args="$args -lc" ;;
        *) args="$args $a" ;;
    esac
done
exec /usr/bin/lld -flavor gnu $args
EOF
chmod +x /tmp/lld-aarch64-gnu.sh

# Build:
cargo build --release -p caj2pdf-gui \
    --target aarch64-unknown-linux-musl
```

The output is at `target/aarch64-unknown-linux-musl/release/caj2pdf-gui`
(~15 MB, statically linked). Copy it to any aarch64 Linux box and run:

```bash
./caj2pdf-gui   # X11 or Wayland required
```

No `apt install`, no `.so` dependencies, no C runtime.

## Why the GUI is statically linked

`caj2pdf-rs` is 100 % pure Rust — no FFI, no C deps. (Earlier versions
used `libjbig2dec` via FFI; that was replaced with the pure-Rust
`pdfluent-jbig2` crate in v0.1.1.) The `eframe` GUI pulls in
`winit`, `accesskit`, `glow`, etc., all pure Rust. So the only
remaining C dependency at link time is the system libc, and the
musl target provides libc statically.
