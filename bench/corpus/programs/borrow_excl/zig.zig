fn bump(x: *i64) void {
    x.* += 1;
}

fn run(v: i64) i64 {
    var c = v;
    bump(&c);
    return c;
}
