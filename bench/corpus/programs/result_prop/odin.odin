package bench

parse :: proc(x: i64) -> (i64, bool) {
	if x < 0 {
		return 0, false
	}
	return x, true
}

dbl :: proc(x: i64) -> (i64, bool) {
	v := parse(x) or_return
	return v + v, true
}
