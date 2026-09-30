const Buf = struct {
    n: i64,
};

fn eat(b: Buf) i64 {
    return b.n;
}

fn feed(b: Buf) i64 {
    return eat(b);
}
