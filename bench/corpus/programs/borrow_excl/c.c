#include <stdint.h>

void bump(int64_t *x) {
    *x += 1;
}

int64_t run(int64_t v) {
    int64_t c = v;
    bump(&c);
    return c;
}
