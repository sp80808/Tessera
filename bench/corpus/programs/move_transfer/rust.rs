struct Buf {
    n: i64,
}

fn eat(b: Buf) -> i64 {
    b.n
}

fn feed(b: Buf) -> i64 {
    eat(b)
}
