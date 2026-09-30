#include <stdint.h>

int64_t tri(int64_t n) {
    int64_t acc = 0;
    for (int64_t i = 0; i < n; i++) {
        acc += i;
    }
    return acc;
}
