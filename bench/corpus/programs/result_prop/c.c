#include <stdint.h>

int parse(int64_t x, int64_t *out) {
    if (x < 0) {
        return 1;
    }
    *out = x;
    return 0;
}

int dbl(int64_t x, int64_t *out) {
    int64_t v;
    if (parse(x, &v)) {
        return 1;
    }
    *out = v + v;
    return 0;
}
