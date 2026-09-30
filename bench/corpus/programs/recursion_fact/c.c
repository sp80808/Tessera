#include <stdint.h>

int64_t fact(int64_t n) {
    return n < 2 ? 1 : n * fact(n - 1);
}
