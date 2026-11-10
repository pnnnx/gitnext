/* Growable byte buffer. */
#include <stdlib.h>
#include <string.h>
#include "buffer.h"

#define BUF_MIN 16
#define BUF_GROW(n) ((n) * 2)

typedef struct buffer {
    unsigned char *data;
    size_t len;
    size_t cap;
} buffer_t;

typedef int (*visit_fn)(unsigned char byte, void *ctx);

enum buf_status { BUF_OK = 0, BUF_NOMEM = 1 };

static int ensure(buffer_t *b, size_t need);

/* Create an empty buffer. */
buffer_t *buf_new(void) {
    buffer_t *b = calloc(1, sizeof *b);
    if (b && ensure(b, BUF_MIN) != BUF_OK) {
        free(b);
        return NULL;
    }
    return b;
}

/* Grow the storage to at least `need` bytes. */
static int ensure(buffer_t *b, size_t need) {
    if (need <= b->cap) return BUF_OK;
    size_t cap = b->cap ? b->cap : BUF_MIN;
    while (cap < need) cap = BUF_GROW(cap);
    unsigned char *p = realloc(b->data, cap);
    if (!p) return BUF_NOMEM;
    b->data = p;
    b->cap = cap;
    return BUF_OK;
}

int buf_append(buffer_t *b, const void *src, size_t n) {
    int rc = ensure(b, b->len + n);
    if (rc != BUF_OK) return rc;
    memcpy(b->data + b->len, src, n);
    b->len += n;
    return BUF_OK;
}

/* Call `fn` for every byte; stops when it returns non-zero. */
int buf_each(const buffer_t *b, visit_fn fn, void *ctx) {
    for (size_t i = 0; i < b->len; i++) {
        int r = fn(b->data[i], ctx);
        if (r) return r;
    }
    return 0;
}

void buf_free(buffer_t *b) {
    if (!b) return;
    free(b->data);
    free(b);
}
