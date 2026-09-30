package bench

clip :: proc(x: i64) -> i64 {
	if x < 0 {
		return 0
	}
	if x > 100 {
		return 100
	}
	return x
}
