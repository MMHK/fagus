/*
 * The C ABI that PHP's FFI extension sees. This header is the contract: keep it in step with
 * crates/fagus-ffi/src/lib.rs and bump fagus_abi_version() when a change is incompatible.
 *
 * Rules: no structs (only scalars and byte buffers, so that no compiler's layout matters), no
 * C99 bool, input always (pointer, length), and every buffer that Rust allocates is freed by
 * fagus_bytes_free().
 */
#ifndef FAGUS_FFI_H
#define FAGUS_FFI_H

#include <stdint.h>
#include <stddef.h>

/* The version of this ABI. Check it after loading. */
uint32_t    fagus_abi_version(void);
const char *fagus_version(void);

/* An engine, owned by the caller: pass it to fagus_engine_free() exactly once. */
void       *fagus_engine_new(const uint8_t *config_json, size_t config_len);
void        fagus_engine_free(void *engine);

/*
 * Renders a document. request_json is the header of the worker protocol's render request, with
 * "blobLengths" added; blob_data holds the blobs one after another. Returns 0 on success.
 * Buffers written to *pdf and *meta are freed with fagus_bytes_free().
 */
int32_t     fagus_render(void *engine,
                         const uint8_t *request_json, size_t request_len,
                         const uint8_t *blob_data,    size_t blob_data_len,
                         uint8_t **pdf,  size_t *pdf_len,
                         uint8_t **meta, size_t *meta_len);

void        fagus_bytes_free(uint8_t *ptr, size_t len);

/* The last error of this thread, NUL-terminated, owned by Fagus. */
const char *fagus_last_error(void);

/* SPIKE: renders one tiny document in-process and returns the length of the PDF, or 0. */
size_t      fagus_spike_probe(void);

#endif
