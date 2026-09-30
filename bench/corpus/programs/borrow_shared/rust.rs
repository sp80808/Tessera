fn peek(x: &i64) -> i64 {
    *x + 1
}

fn look(v: i64) -> i64 {
    peek(&v)
}
