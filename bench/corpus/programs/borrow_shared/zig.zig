fn peek(x: *const i64) i64 {
    return x.* + 1;
}

fn look(v: i64) i64 {
    return peek(&v);
}
