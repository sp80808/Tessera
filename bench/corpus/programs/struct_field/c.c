#include <stdint.h>

typedef struct {
    int64_t x;
    int64_t y;
} P;

int64_t norm1(P p) {
    return p.x + p.y;
}
