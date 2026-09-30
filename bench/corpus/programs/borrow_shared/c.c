#include <stdint.h>

int64_t peek(const int64_t *x) {
    return *x + 1;
}

int64_t look(int64_t v) {
    return peek(&v);
}
