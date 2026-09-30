fn bump(x: &mut i64) {
    *x += 1;
}

fn run(v: i64) -> i64 {
    let mut c = v;
    bump(&mut c);
    c
}
