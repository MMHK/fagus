//! A C ABI shared library for Fagus, for PHP's FFI extension.
//!
//! **Spike.** The ABI, the header and the build are the real ones; `fagus_render` is a stub and
//! `fagus_spike_probe` renders one tiny document so that the engine, its fonts and the whole
//! pipeline are linked into the library. The point is to answer, in CI and before the real work
//! starts, whether a `cdylib` with this surface loads in PHP 7.4, 8.0, 8.3 and 8.4, and what the
//! library depends on. See `docs/plans/2026-11-20-php-ffi-cdylib.md`.

// The workspace denies `unsafe_code`; this crate is the one place that needs it.
#![allow(unsafe_code, clippy::not_unsafe_ptr_arg_deref)]

use std::ffi::{c_char, c_void};

/// The version of this ABI, as `include/fagus.h` declares it.
const ABI_VERSION: u32 = 1;

/// The version of the library, NUL-terminated for C.
static VERSION: &[u8] = b"0.2.0-spike\0";

/// The message `fagus_render` leaves behind until rendering is implemented.
static NOT_IMPLEMENTED: &[u8] = b"spike: rendering is not implemented\0";

#[unsafe(no_mangle)]
pub extern "C" fn fagus_abi_version() -> u32 {
    ABI_VERSION
}

#[unsafe(no_mangle)]
pub extern "C" fn fagus_version() -> *const c_char {
    VERSION.as_ptr().cast()
}

/// An engine, owned by C: pass it back to [`fagus_engine_free`] exactly once.
#[unsafe(no_mangle)]
pub extern "C" fn fagus_engine_new(_config_json: *const u8, _config_len: usize) -> *mut c_void {
    Box::into_raw(Box::new(fagus::Engine::new())).cast()
}

#[unsafe(no_mangle)]
pub extern "C" fn fagus_engine_free(engine: *mut c_void) {
    if !engine.is_null() {
        // SAFETY: `engine` is a pointer from `fagus_engine_new`, freed once by its caller.
        drop(unsafe { Box::from_raw(engine.cast::<fagus::Engine>()) });
    }
}

/// SPIKE: returns -1 and writes no buffer; the real ABI takes the protocol's JSON request.
#[unsafe(no_mangle)]
pub extern "C" fn fagus_render(
    _engine: *mut c_void,
    _request_json: *const u8,
    _request_len: usize,
    _blob_data: *const u8,
    _blob_data_len: usize,
    _pdf: *mut *mut u8,
    _pdf_len: *mut usize,
    _meta: *mut *mut u8,
    _meta_len: *mut usize,
) -> i32 {
    -1
}

/// Nothing is allocated by the stub `fagus_render`; kept so that the header is final.
#[unsafe(no_mangle)]
pub extern "C" fn fagus_bytes_free(_ptr: *mut u8, _len: usize) {}

#[unsafe(no_mangle)]
pub extern "C" fn fagus_last_error() -> *const c_char {
    NOT_IMPLEMENTED.as_ptr().cast()
}

/// SPIKE: renders one tiny document in-process and returns the length of the PDF, or 0.
///
/// This is what forces the layout, paint and PDF crates, the built-in fonts and the text stack
/// into the library, so that the glibc and DLL dependency checks see the real graph.
#[unsafe(no_mangle)]
pub extern "C" fn fagus_spike_probe() -> usize {
    std::panic::catch_unwind(|| {
        let engine = fagus::Engine::new();
        engine
            .render(&fagus::Document::html("<p>spike</p>"))
            .map_or(0, |output| output.pdf.len())
    })
    .unwrap_or(0)
}
