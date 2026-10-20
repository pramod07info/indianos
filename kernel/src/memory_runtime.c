/* Minimal freestanding C memory primitives for compiler-generated
 * initializers/copies. NOT fault-safe copy_from_user functionality. */
#include <stddef.h>
#include <stdint.h>

void *memset(void *destination, int value, size_t count) {
    volatile unsigned char *out = (volatile unsigned char *)destination;
    while (count != 0) {
        *out++ = (unsigned char)value;
        --count;
    }
    return destination;
}

void *memcpy(void *destination, const void *source, size_t count) {
    volatile unsigned char *out = (volatile unsigned char *)destination;
    const volatile unsigned char *in = (const volatile unsigned char *)source;
    while (count != 0) {
        *out++ = *in++;
        --count;
    }
    return destination;
}

void *memmove(void *destination, const void *source, size_t count) {
    volatile unsigned char *out = (volatile unsigned char *)destination;
    const volatile unsigned char *in = (const volatile unsigned char *)source;
    if ((const unsigned char *)destination == (const unsigned char *)source || count == 0) {
        return destination;
    }
    if ((uintptr_t)destination < (uintptr_t)source) {
        while (count-- != 0) *out++ = *in++;
    } else {
        while (count != 0) {
            --count;
            out[count] = in[count];
        }
    }
    return destination;
}
