/* Benchmark validator only. This code is never linked into the application. */
#include <stddef.h>
#include <stdint.h>

uint64_t stream_benchmark_fnv1a64(const unsigned char *data, size_t length) {
    uint64_t hash = UINT64_C(0xcbf29ce484222325);
    for (size_t index = 0; index < length; ++index) {
        hash ^= data[index];
        hash *= UINT64_C(0x100000001b3);
    }
    return hash;
}
