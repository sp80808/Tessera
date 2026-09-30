package bench

larger :: proc(a, b: i64) -> i64 {
	return a if a > b else b
}
