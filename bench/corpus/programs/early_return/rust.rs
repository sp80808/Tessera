fn clip(x: i64) -> i64 {
    if x < 0 {
        return 0;
    }
    if x > 100 {
        return 100;
    }
    x
}
