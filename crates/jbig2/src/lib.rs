//! # caj2pdf-jbig2
//!
//! JBIG2 stream decoder for caj2pdf-rs. Uses FFI to the system
//! `libjbig2dec` library (Debian/Ubuntu package `libjbig2dec0-dev`,
//! version 0.19+).
//!
//! At runtime, `jbig2dec.h` provides the lower-level context API
//! used here; the reference wrapper in
//! `caj2pdf/lib/decode_jbig2data_x.cc` shows the exact same call
//! sequence.  See `docs/jbig2-notes.md` for the full CNKI header
//! layout and the `bytes_per_line` / `width_in_bytes` math.

#![deny(missing_debug_implementations)]
#![warn(unreachable_pub)]

mod ffi;

use thiserror::Error;

/// Errors produced by the JBIG2 decoder.
#[derive(Debug, Error)]
pub enum Jbig2Error {
    #[error("libjbig2dec could not be loaded: {0}")]
    Library(String),
    #[error("decoder returned error code {0}")]
    Decode(i32),
    #[error("input buffer is too short: need {need} bytes, got {got}")]
    ShortInput { need: usize, got: usize },
}

pub type Jbig2Result<T> = std::result::Result<T, Jbig2Error>;

/// The decoded bitmap (1 bit per pixel, MSB-first within each byte).
#[derive(Debug, Clone)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub bits: Vec<u8>,
}

impl Bitmap {
    pub fn row_bytes(&self) -> usize {
        ((self.width as usize) + 7) / 8
    }
}

/// Number of header bytes prepended to every JBIG2 image in a CNKI
/// `CImage` blob.  See `docs/jbig2-notes.md` for the full layout.
const CNKI_HEADER_LEN: usize = 48;

/// Compute the 4-byte-aligned row stride used by the CNKI C wrapper.
///
/// Equivalent to the Python helper:
/// `(width * bits_per_pixel + 31) >> 5 << 2`
/// For 1-bpp images this reduces to `((width + 31) / 32) * 4`.
fn bytes_per_line(width: u32, bits_per_pixel: u32) -> u32 {
    ((width * bits_per_pixel + 31) >> 5) << 2
}

/// Compute the unpadded width in bytes for a row of `width` pixels at
/// `bits_per_pixel` bits each.  Equivalent to the Python helper:
/// `(width * bits_per_pixel + 7) >> 3`
fn width_in_bytes(width: u32, bits_per_pixel: u32) -> u32 {
    (width * bits_per_pixel + 7) >> 3
}

/// Decode a JBIG2 image extracted from a CNKI `CImage` blob.
pub fn decode(input: &[u8], width: u32, height: u32) -> Jbig2Result<Bitmap> {
    if input.len() < CNKI_HEADER_LEN {
        return Err(Jbig2Error::ShortInput {
            need: CNKI_HEADER_LEN,
            got: input.len(),
        });
    }
    let stream = &input[CNKI_HEADER_LEN..];
    unsafe { decode_inner(stream, width, height) }
}

unsafe fn decode_inner(stream: &[u8], width: u32, height: u32) -> Jbig2Result<Bitmap> {
    use ffi::{
        jbig2_complete_page, jbig2_ctx_free, jbig2_ctx_new_imp, jbig2_data_in, jbig2_page_out,
        jbig2_release_page, JBIG2_OPTIONS_EMBEDDED, JBIG2_VERSION_MAJOR, JBIG2_VERSION_MINOR,
    };

    let _ = (bytes_per_line(width, 1), width_in_bytes(width, 1));

    // SAFETY: all four pointer arguments are NULL and the version
    // constants come from jbig2.h.  jbig2dec returns NULL only if
    // malloc fails.
    let ctx = jbig2_ctx_new_imp(
        std::ptr::null_mut(),
        JBIG2_OPTIONS_EMBEDDED,
        std::ptr::null_mut(),
        None,
        std::ptr::null_mut(),
        JBIG2_VERSION_MAJOR,
        JBIG2_VERSION_MINOR,
    );
    if ctx.is_null() {
        return Err(Jbig2Error::Library(
            "jbig2_ctx_new_imp returned NULL (out of memory)".into(),
        ));
    }

    struct Guard {
        ctx: *mut ffi::Jbig2Ctx,
        page: *mut ffi::Jbig2Image,
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe {
                if !self.page.is_null() {
                    jbig2_release_page(self.ctx, self.page);
                }
                if !self.ctx.is_null() {
                    jbig2_ctx_free(self.ctx);
                }
            }
        }
    }
    let mut guard = Guard { ctx, page: std::ptr::null_mut() };

    // SAFETY: ctx is a live context, stream.as_ptr() points to
    // stream.len() valid bytes (or is dangling-but-valid for empty
    // slices).
    let rc = jbig2_data_in(guard.ctx, stream.as_ptr(), stream.len());
    if rc != 0 {
        return Err(Jbig2Error::Decode(-1));
    }

    // SAFETY: guard.ctx is live; jbig2_complete_page does not retain the pointer.
    let rc = jbig2_complete_page(guard.ctx);
    if rc != 0 {
        return Err(Jbig2Error::Decode(rc));
    }

    // SAFETY: guard.ctx is live; the returned pointer is owned by the context.
    let image = jbig2_page_out(guard.ctx);
    if image.is_null() {
        return Err(Jbig2Error::Decode(-2));
    }
    guard.page = image;

    // SAFETY: image is a non-null pointer returned by jbig2_page_out;
    // the struct layout matches the C definition.
    let decoded_w = (*image).width;
    let decoded_h = (*image).height;
    let stride = (*image).stride as usize;
    let data = (*image).data;

    if decoded_w != width || decoded_h != height {
        return Err(Jbig2Error::Decode(-3));
    }

    let row_bytes = ((width as usize) + 7) / 8;
    let total = row_bytes * (height as usize);
    let mut bits = vec![0u8; total];

    if stride == row_bytes {
        // SAFETY: data points to stride*height valid bytes
        // (guaranteed by jbig2dec), bits has length total == stride*height.
        std::ptr::copy_nonoverlapping(data, bits.as_mut_ptr(), total);
    } else {
        for row in 0..(height as usize) {
            // SAFETY: each data.add(row * stride) is a valid
            // pointer to row_bytes bytes.
            let src = data.add(row * stride);
            let dst = bits.as_mut_ptr().add(row * row_bytes);
            std::ptr::copy_nonoverlapping(src, dst, row_bytes);
        }
    }

    Ok(Bitmap { width, height, bits })
}
