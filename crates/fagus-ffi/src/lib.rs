//! A C ABI shared library for Fagus, for PHP's FFI extension.
//!
//! The ABI is small on purpose: no structs, only scalars and byte buffers, so that no compiler's
//! layout matters; memory is allocated and freed here; and every entry point catches panics, which
//! must not unwind into PHP. See `include/fagus.h` and
//! `docs/plans/2026-11-20-php-ffi-cdylib.md`.

// The workspace denies `unsafe_code`; this crate is the one place that needs it.
#![allow(unsafe_code, clippy::not_unsafe_ptr_arg_deref)]

use std::cell::RefCell;
use std::ffi::{CString, c_char, c_void};
use std::path::PathBuf;

use fagus::Engine;
use fagus_protocol::Request;
use serde::Deserialize;
use serde_json::Value;

/// The version of this ABI, as `include/fagus.h` declares it.
const ABI_VERSION: u32 = 1;

/// The version of the library, NUL-terminated for C.
static VERSION: &[u8] = concat!(env!("CARGO_PKG_VERSION"), "\0").as_bytes();

/// The last error of each thread, as `fagus_last_error` returns it.
thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

/// Remembers `message` as this thread's last error.
fn set_error(message: impl Into<String>) {
    let message = message.into().replace('\0', " ");
    LAST_ERROR.with(|last| *last.borrow_mut() = CString::new(message).ok());
}

#[unsafe(no_mangle)]
pub extern "C" fn fagus_abi_version() -> u32 {
    ABI_VERSION
}

#[unsafe(no_mangle)]
pub extern "C" fn fagus_version() -> *const c_char {
    VERSION.as_ptr().cast()
}

/// The settings of an engine, as `fagus_engine_new` takes them.
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    /// Directories of font files, searched recursively.
    #[serde(default)]
    font_dirs: Vec<PathBuf>,
    /// Whether to use the fonts installed on the machine as well.
    #[serde(default)]
    system_fonts: bool,
    #[serde(default)]
    locale: Option<String>,
    #[serde(default)]
    sans_serif: Option<String>,
    #[serde(default)]
    serif: Option<String>,
    #[serde(default)]
    monospace: Option<String>,
    /// The memory budget of each document in bytes (0 or absent for none).
    #[serde(default)]
    memory_budget: Option<u64>,
    /// The time limit of each document in milliseconds (0 or absent for none).
    #[serde(default)]
    time_limit: Option<f64>,
}

/// An engine, owned by C: pass it back to [`fagus_engine_free`] exactly once.
///
/// Returns null, with `fagus_last_error` set, when the settings cannot be applied.
#[unsafe(no_mangle)]
pub extern "C" fn fagus_engine_new(config_json: *const u8, config_len: usize) -> *mut c_void {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let config: Config = if config_len == 0 {
            Config::default()
        } else {
            let bytes = bytes(config_json, config_len)
                .ok_or_else(|| "the engine settings are missing".to_owned())?;
            serde_json::from_slice(bytes).map_err(|error| format!("invalid settings: {error}"))?
        };
        let mut engine = Engine::new();
        for directory in &config.font_dirs {
            engine
                .add_font_dir(directory)
                .map_err(|error| format!("reading fonts in {}: {error}", directory.display()))?;
        }
        if config.system_fonts {
            let _ = engine.add_system_fonts();
        }
        engine.set_locale(config.locale);
        if config.sans_serif.is_some() || config.serif.is_some() || config.monospace.is_some() {
            engine.set_generic_families(config.sans_serif, config.serif, config.monospace);
        }
        if config.memory_budget.is_some() || config.time_limit.is_some() {
            let mut limits = engine.limits();
            if let Some(bytes) = config.memory_budget {
                limits = limits.memory((bytes > 0).then_some(bytes));
            }
            if let Some(milliseconds) = config.time_limit {
                let limit = std::time::Duration::try_from_secs_f64(milliseconds / 1000.0)
                    .map_err(|_| format!("invalid time limit `{milliseconds}`"))?;
                limits = limits.time((!limit.is_zero()).then_some(limit));
            }
            engine.set_limits(limits);
        }
        Ok(Box::into_raw(Box::new(engine)).cast::<c_void>())
    })) {
        Ok(Ok(engine)) => engine,
        Ok(Err(message)) => {
            set_error(message);
            std::ptr::null_mut()
        }
        Err(_) => {
            set_error("panicked while creating the engine");
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn fagus_engine_free(engine: *mut c_void) {
    if !engine.is_null() {
        // SAFETY: `engine` is a pointer from `fagus_engine_new`, freed once by its caller.
        drop(unsafe { Box::from_raw(engine.cast::<Engine>()) });
    }
}

/// Renders a document. See `include/fagus.h` for the request and the buffers.
///
/// Returns 0 on success, -1 on any error (with `fagus_last_error` set).
#[unsafe(no_mangle)]
pub extern "C" fn fagus_render(
    engine: *mut c_void,
    request_json: *const u8,
    request_len: usize,
    blob_data: *const u8,
    blob_data_len: usize,
    pdf: *mut *mut u8,
    pdf_len: *mut usize,
    meta: *mut *mut u8,
    meta_len: *mut usize,
) -> i32 {
    if engine.is_null() || pdf.is_null() || pdf_len.is_null() || meta.is_null() || meta_len.is_null() {
        set_error("fagus_render was given a null pointer");
        return -1;
    }
    // SAFETY: the caller promises exclusive use of the engine for this call.
    let engine = unsafe { &*engine.cast::<Engine>() };

    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        render(engine, request_json, request_len, blob_data, blob_data_len)
    })) {
        Ok(Ok((pdf_bytes, meta_json))) => {
            let (pdf_ptr, pdf_bytes_len) = into_buffer(pdf_bytes);
            let (meta_ptr, meta_bytes_len) = into_buffer(meta_json);
            // SAFETY: the out pointers were checked above and are the caller's.
            unsafe {
                *pdf = pdf_ptr;
                *pdf_len = pdf_bytes_len;
                *meta = meta_ptr;
                *meta_len = meta_bytes_len;
            }
            0
        }
        Ok(Err(message)) => {
            set_error(message);
            -1
        }
        Err(_) => {
            set_error("panicked while rendering");
            -1
        }
    }
}

/// Renders one document, returning its PDF and its metadata as JSON.
#[allow(clippy::too_many_arguments)]
fn render(
    engine: &Engine,
    request_json: *const u8,
    request_len: usize,
    blob_data: *const u8,
    blob_data_len: usize,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let json = bytes(request_json, request_len).ok_or_else(|| "the request is missing".to_owned())?;
    let mut value: Value =
        serde_json::from_slice(json).map_err(|error| format!("invalid request: {error}"))?;
    let lengths: Vec<usize> = match value.as_object_mut().and_then(|object| object.remove("blobLengths")) {
        Some(lengths) => serde_json::from_value(lengths).map_err(|_| "invalid `blobLengths`".to_owned())?,
        None => Vec::new(),
    };
    let request: Request =
        serde_json::from_value(value).map_err(|error| format!("invalid request: {error}"))?;
    if request.signature.is_some() {
        return Err("signatures need the worker, not the C ABI, for now".to_owned());
    }

    let data = bytes(blob_data, blob_data_len).unwrap_or(&[]);
    let total = lengths
        .iter()
        .try_fold(0_usize, |total, &length| total.checked_add(length))
        .ok_or_else(|| "invalid `blobLengths`".to_owned())?;
    if total != data.len() {
        return Err(format!(
            "the request's `blobLengths` sum to {total} bytes, but {} were given",
            data.len()
        ));
    }
    let mut blobs = Vec::with_capacity(lengths.len());
    let mut offset = 0;
    for length in lengths {
        blobs.push(Some(data[offset..offset + length].to_vec()));
        offset += length;
    }

    let document = fagus_protocol::document(request, &mut blobs, engine.limits(), None)?;
    let output = engine
        .render_with_diagnostics(&document)
        .map_err(|failure| failure.error.to_string())?;
    let meta = fagus_protocol::output_json(&output);
    Ok((output.pdf, serde_json::to_vec(&meta).map_err(|error| error.to_string())?))
}

/// A byte slice of `len` bytes at `ptr`, or `None` when the pointer is null.
fn bytes<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if ptr.is_null() || len == 0 {
        None
    } else {
        // SAFETY: the caller promises `len` readable bytes at `ptr` for the call.
        Some(unsafe { std::slice::from_raw_parts(ptr, len) })
    }
}

/// A buffer whose address C may keep until `fagus_bytes_free`, with its length.
///
/// The allocation is exactly as long as the slice, so that `fagus_bytes_free` can rebuild the
/// `Vec` from the pointer and the length alone.
fn into_buffer(bytes: Vec<u8>) -> (*mut u8, usize) {
    let bytes = bytes.into_boxed_slice();
    let len = bytes.len();
    (Box::into_raw(bytes).cast::<u8>(), len)
}

/// Frees a buffer from `fagus_render`.
#[unsafe(no_mangle)]
pub extern "C" fn fagus_bytes_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    // SAFETY: `ptr` and `len` come from `fagus_render`; the allocation is exactly `len` bytes,
    // so the rebuilt `Vec` has the capacity it was allocated with.
    drop(unsafe { Vec::from_raw_parts(ptr, len, len) });
}

#[unsafe(no_mangle)]
pub extern "C" fn fagus_last_error() -> *const c_char {
    static NONE: &[u8] = b"\0";
    LAST_ERROR.with(|last| {
        last.borrow()
            .as_ref()
            .map_or(NONE.as_ptr().cast(), |message| message.as_ptr())
    })
}
