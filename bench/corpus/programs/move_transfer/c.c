#include <stdint.h>

typedef struct {
    int64_t n;
} Buf;

int64_t eat(Buf b) {
    return b.n;
}

int64_t feed(Buf b) {
    return eat(b);
}
