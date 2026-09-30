#include <stdint.h>

int64_t calc(int64_t a, int64_t b) {
    int64_t s = a + b;
    int64_t t = s * a;
    return t - b;
}
