fn parse(x: i64) !i64 {
    if (x < 0) return error.Negative;
    return x;
}

fn dbl(x: i64) !i64 {
    const v = try parse(x);
    return v + v;
}
