#include <stdint.h>

int64_t inc(int64_t x) {
    return x + 1;
}

int64_t dbl(int64_t x) {
    return x + x;
}

int64_t step(int64_t x) {
    return dbl(inc(x));
}
