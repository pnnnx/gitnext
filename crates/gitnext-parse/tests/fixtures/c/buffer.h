#ifndef BUFFER_H
#define BUFFER_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct buffer buffer_t;

/* Create an empty buffer; NULL when out of memory. */
buffer_t *buf_new(void);
int buf_append(buffer_t *b, const void *src, size_t n);
void buf_free(buffer_t *b);

#ifdef __cplusplus
}
#endif

#endif
