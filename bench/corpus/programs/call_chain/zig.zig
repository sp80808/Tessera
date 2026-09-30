fn inc(x: i64) i64 {
    return x + 1;
}

fn dbl(x: i64) i64 {
    return x + x;
}

fn step(x: i64) i64 {
    return dbl(inc(x));
}
