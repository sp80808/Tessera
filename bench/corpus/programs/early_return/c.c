#include <stdint.h>

int64_t clip(int64_t x) {
    if (x < 0) {
        return 0;
    }
    if (x > 100) {
        return 100;
    }
    return x;
}
