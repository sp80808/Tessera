fn tri(n: i64) i64 {
    var acc: i64 = 0;
    var i: i64 = 0;
    while (i < n) : (i += 1) {
        acc += i;
    }
    return acc;
}
