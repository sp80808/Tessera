fn fact(n: i64) i64 {
    return if (n < 2) 1 else n * fact(n - 1);
}
