typedef enum {
    IDLE,
    RUN,
    DONE,
} St;

St next(St s) {
    switch (s) {
    case IDLE:
        return RUN;
    case RUN:
        return DONE;
    default:
        return IDLE;
    }
}
