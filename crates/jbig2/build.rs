// Build script: locate libjbig2dec via pkg-config and emit the link directives
// needed by the FFI bindings in src/lib.rs.

use std::process::Command;

fn main() {
    let pkg_name = "jbig2dec";

    // Probe pkg-config separately for cflags and libs.
    let cflags = Command::new("pkg-config")
        .args(["--cflags", pkg_name])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();

    let libs_str = Command::new("pkg-config")
        .args(["--libs", pkg_name])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();

    // Extract include dirs from cflags and feed them to rustc.
    for tok in cflags.split_whitespace() {
        if let Some(dir) = tok.strip_prefix("-I") {
            println!("cargo:include={}", dir);
        }
    }

    // If pkg-config succeeded, link with the libraries it reported; otherwise
    // rely on the system linker to find libjbig2dec.so.
    if libs_str.is_empty() {
        println!("cargo:warning=libjbig2dec not found via pkg-config; falling back to system linker");
        println!("cargo:rustc-link-lib=dylib=jbig2dec");
    } else {
        for tok in libs_str.split_whitespace() {
            if let Some(dir) = tok.strip_prefix("-L") {
                println!("cargo:rustc-link-search=native={}", dir);
            } else if let Some(lib) = tok.strip_prefix("-l") {
                println!("cargo:rustc-link-lib=dylib={}", lib);
            }
        }
    }

    println!("cargo:rerun-if-changed=build.rs");
}
