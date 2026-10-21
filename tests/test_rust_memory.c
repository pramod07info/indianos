/* Exact no_std memory builtin ABI parity with libc semantics.
 * This executable is host-only; UART I/O requires QEMU instead. */
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <assert.h>

typedef void *(*memmove_function)(void *, const void *, size_t);
typedef void *(*memset_function)(void *, int, size_t);
typedef void *(*memcpy_function)(void *, const void *, size_t);

int main(void) {
    volatile memmove_function mov = memmove;
    volatile memset_function set = memset;
    volatile memcpy_function cpy = memcpy;
    char data[9] = "abcdefgh";
    assert(mov(data + 2, data, 6) == data + 2);
    assert(memcmp(data, "ababcdef", 8) == 0);
    assert(mov(data, data + 2, 6) == data);
    assert(memcmp(data, "abcdefef", 8) == 0);
    assert(set(data, 'X', 2) == data);
    assert(memcmp(data, "XXcdefef", 8) == 0);
    char other[9] = {0};
    assert(cpy(other, data, 8) == other);
    assert(memcmp(other, data, 8) == 0);
    assert(mov(data, data, 8) == data);
    assert(set(data, 0, 0) == data);
    puts("IndianOS Rust memory runtime C ABI tests passed");
    return 0;
}
