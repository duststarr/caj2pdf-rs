//! Raw FFI bindings to the system `libjbig2dec` C library.
//!
//! Signatures copied verbatim from the upstream jbig2.h header
//! (Debian package libjbig2dec0-dev, version 0.19).  The
//! high-level jbig2_decode_generic symbol that *later* versions
//! of jbig2dec expose is **not** available in 0.19, so we use the
//! lower-level context / data_in / page_out API that the
//! reference C wrapper in caj2pdf/lib/decode_jbig2data_x.cc also
//! relies on.

#![allow(non_camel_case_types, non_snake_case, unreachable_pub, deprecated, dead_code)]

use libc::{c_char, c_int, c_uint, c_void, size_t, uint8_t, uint32_t};

/// Major version of the libjbig2dec API.  Must match JBIG2_VERSION_MAJOR from jbig2.h.
pub const JBIG2_VERSION_MAJOR: c_int = 0;
/// Minor version of the libjbig2dec API.  Must match JBIG2_VERSION_MINOR from jbig2.h.
pub const JBIG2_VERSION_MINOR: c_int = 19;

/// Embedded-stream option bit, passed to jbig2_ctx_new_imp.
pub const JBIG2_OPTIONS_EMBEDDED: c_uint = 1;

/// Opaque decoder context.  Allocated and freed by libjbig2dec.
#[repr(C)]
pub struct Jbig2Ctx { _opaque: [u8; 0] }

/// Opaque global context for embedded streams.
#[repr(C)]
pub struct Jbig2GlobalCtx { _opaque: [u8; 0] }

/// Opaque custom allocator.  Only used when the caller wires one in.
#[repr(C)]
pub struct Jbig2Allocator { _opaque: [u8; 0] }

/// A decoded 1-bpp JBIG2 page image.  `data` points to a buffer of
/// `stride * height` bytes (1 bit per pixel, MSB-first within each
/// byte, rows contiguous).  `refcount` is owned by libjbig2dec and
/// must not be touched by the caller.
#[repr(C)]
pub struct Jbig2Image {
    pub width: uint32_t,
    pub height: uint32_t,
    pub stride: uint32_t,
    pub data: *mut uint8_t,
    pub refcount: c_int,
}

/// Error / warning severity levels reported by the optional error
/// callback.  Mirrors `Jbig2Severity` from jbig2.h.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Jbig2Severity {
    JBIG2_SEVERITY_DEBUG = 0,
    JBIG2_SEVERITY_INFO = 1,
    JBIG2_SEVERITY_WARNING = 2,
    JBIG2_SEVERITY_FATAL = 3,
}

/// User-supplied error callback.
pub type Jbig2ErrorCallback =
    Option<unsafe extern "C" fn(data: *mut c_void, msg: *const c_char, severity: Jbig2Severity, seg_idx: uint32_t)>;

extern "C" {
    /// Construct a fresh decoder context.  `jbig2_ctx_new` is a
    /// macro in the C header that expands to `jbig2_ctx_new_imp`
    /// with the version constants baked in; we bind the underlying
    /// function so we can pin the version explicitly.
    pub(crate) fn jbig2_ctx_new_imp(
        allocator: *mut Jbig2Allocator,
        options: c_uint,
        global_ctx: *mut Jbig2GlobalCtx,
        error_callback: Jbig2ErrorCallback,
        error_callback_data: *mut c_void,
        jbig2_version_major: c_int,
        jbig2_version_minor: c_int,
    ) -> *mut Jbig2Ctx;

    /// Free a decoder context and any per-context state.  Returns
    /// the allocator pointer (or NULL if a default allocator was used).
    pub(crate) fn jbig2_ctx_free(ctx: *mut Jbig2Ctx) -> *mut Jbig2Allocator;

    /// Submit a chunk of JBIG2-encoded data to the decoder.
    /// Returns 0 on success, -1 on fatal error.
    pub(crate) fn jbig2_data_in(ctx: *mut Jbig2Ctx, data: *const uint8_t, size: size_t) -> c_int;

    /// Mark the current page as complete, simulating an end-of-page
    /// segment.  Required for "broken CVision embedded streams"
    /// (see comment in the reference C wrapper).
    pub(crate) fn jbig2_complete_page(ctx: *mut Jbig2Ctx) -> c_int;

    /// Pop the most recently decoded page image out of the context.
    /// Returns NULL if no page is available.
    pub(crate) fn jbig2_page_out(ctx: *mut Jbig2Ctx) -> *mut Jbig2Image;

    /// Release a page image previously returned by jbig2_page_out.
    pub(crate) fn jbig2_release_page(ctx: *mut Jbig2Ctx, image: *mut Jbig2Image);
}
