# Cross-compilation

This document records the cross-compilation setup for `caj2pdf-rs`.

## Status matrix

| Target | Binary | Size | Status | System deps |
| --- | --- | --- | --- | --- |
| `x86_64-unknown-linux-gnu` | `target/release/caj2pdf-gui` | 16 MB | ✅ | none (uses system glibc) |
| `aarch64-unknown-linux-musl` | `target/aarch64-unknown-linux-musl/release/caj2pdf-gui` | 15 MB | ✅ **statically linked** | none (libc bundled) |
| `x86_64-pc-windows-gnu` | `target/x86_64-pc-windows-gnu/release/caj2pdf-gui.exe` | 9.5 MB | ✅ **PE32+ GUI** | none (MinGW CRT statically linked) |
| `x86_64-pc-windows-gnullvm` | — | — | not pursued; the gnu target works |
| `aarch64-pc-windows-gnullvm` | — | — | not attempted |

## Building

### Linux x86_64 (default)

```bash
cargo build --release -p caj2pdf-gui
# → target/release/caj2pdf-gui
```

### Linux aarch64 (Kylin / 麒麟 / any ARM Linux)

```bash
rustup target add aarch64-unknown-linux-musl
# one-time: create the gcc-ism → lld wrapper
sudo tee /usr/local/bin/lld-aarch64-gnu.sh >/dev/null <<'EOF'
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
sudo chmod +x /usr/local/bin/lld-aarch64-gnu.sh

# build
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=/usr/local/bin/lld-aarch64-gnu.sh \
  cargo build --release -p caj2pdf-gui --target aarch64-unknown-linux-musl
# → target/aarch64-unknown-linux-musl/release/caj2pdf-gui
```

The wrapper strips gcc-specific flags and translates `-ldl` → `-lc`
(musl has no separate libdl). The resulting binary is a **fully
static** ARM aarch64 ELF that runs on Kylin V10/V11, Ubuntu ARM,
UOS, or any aarch64 Linux without `apt install`.

### Windows x86_64

```bash
sudo apt install mingw-w64            # provides x86_64-w64-mingw32-dlltool
rustup target add x86_64-pc-windows-gnu
cargo build --release -p caj2pdf-gui --target x86_64-pc-windows-gnu
# → target/x86_64-pc-windows-gnu/release/caj2pdf-gui.exe
```

The output is a **PE32+ GUI executable** (no console window, opens
a native window on double-click). The MinGW C runtime is
statically linked, so the .exe runs on any Windows 7+ x64 system
with no DLL dependencies.

The `.cargo/config.toml` in the repo root sets
`rustflags = ["-C", "link-arg=-mwindows"]` for both windows-gnu
targets, which is what makes the .exe a GUI application instead
of a console one. Without this flag, a black cmd window would pop
up on launch.

## Pre-built binaries

Latest release artifacts are in `dist/`:

| File | Target | Size | Run on |
| --- | --- | --- | --- |
| `dist/caj2pdf-gui-aarch64-kylin` | aarch64-unknown-linux-musl | 15 MB | Kylin, Ubuntu ARM, UOS, any aarch64 Linux |
| `dist/caj2pdf-gui-x86_64-windows.exe` | x86_64-pc-windows-gnu | 9.5 MB | Windows 7+ x64 |

The Linux binary is fully static (`file` reports `statically
linked`); the Windows .exe is also static (MinGW import libraries
baked in, no runtime DLL needed).

## Why the GUI is statically linked

`caj2pdf-rs` is 100 % pure Rust — no FFI, no C deps. (Earlier versions
used `libjbig2dec` via FFI; that was replaced with the pure-Rust
`pdfluent-jbig2` crate in v0.1.1.) The `eframe` GUI pulls in
`winit`, `accesskit`, `glow`, etc., all pure Rust. So the only
remaining C dependency at link time is the system libc, which the
musl and MinGW targets provide statically.

## What does NOT work in this sandbox (without sudo)

The first attempt at Windows cross-compilation spent a long time
trying workarounds for the missing `dlltool`. The fix was a single
`sudo apt install mingw-w64`. Lesson: ask for the password before
going down the workaround rabbit hole.
