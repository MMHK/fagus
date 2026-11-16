// Simulated engine: reads the request, "renders" by producing out_len bytes.
// Modes: one-shot (read stdin to EOF, write output) or --worker (length-prefixed frames, loop).
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <unistd.h>

static int read_full(int fd, void *buf, size_t n) {
    size_t got = 0;
    while (got < n) { ssize_t r = read(fd, (char *)buf + got, n - got); if (r <= 0) return -1; got += (size_t)r; }
    return 0;
}
static int write_full(int fd, const void *buf, size_t n) {
    size_t put = 0;
    while (put < n) { ssize_t w = write(fd, (const char *)buf + put, n - put); if (w <= 0) return -1; put += (size_t)w; }
    return 0;
}
static char *render(const char *in, size_t in_len, size_t out_len) {
    char *out = malloc(out_len ? out_len : 1);
    for (size_t i = 0; i < out_len; i++) out[i] = in_len ? in[i % in_len] : 'x';
    return out;
}
// FFI entry point
size_t engine_render(const char *in, size_t in_len, size_t out_len, char **out) {
    *out = render(in, in_len, out_len);
    return out_len;
}
void engine_free(char *p) { free(p); }

#ifndef LIBRARY
int main(int argc, char **argv) {
    size_t out_len = argc > 1 ? strtoul(argv[argc - 1], NULL, 10) : 70000;
    if (argc > 1 && strcmp(argv[1], "--worker") == 0) {
        uint32_t len;
        while (read_full(0, &len, 4) == 0) {
            char *in = malloc(len ? len : 1);
            if (read_full(0, in, len) != 0) return 1;
            char *out = render(in, len, out_len);
            uint32_t ol = (uint32_t)out_len;
            write_full(1, &ol, 4); write_full(1, out, out_len);
            free(in); free(out);
        }
        return 0;
    }
    size_t cap = 1 << 16, len = 0; char *in = malloc(cap); ssize_t r;
    while ((r = read(0, in + len, cap - len)) > 0) { len += (size_t)r; if (len == cap) { cap *= 2; in = realloc(in, cap); } }
    char *out = render(in, len, out_len);
    write_full(1, out, out_len);
    return 0;
}
#endif
