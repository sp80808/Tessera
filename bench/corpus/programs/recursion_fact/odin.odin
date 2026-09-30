package bench

fact :: proc(n: i64) -> i64 {
	return 1 if n < 2 else n * fact(n - 1)
}
