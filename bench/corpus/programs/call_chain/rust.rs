fn inc(x: i64) -> i64 {
    x + 1
}

fn dbl(x: i64) -> i64 {
    x + x
}

fn step(x: i64) -> i64 {
    dbl(inc(x))
}
