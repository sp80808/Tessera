const St = enum {
    idle,
    run,
    done,
};

fn next(s: St) St {
    return switch (s) {
        .idle => .run,
        .run => .done,
        .done => .idle,
    };
}
