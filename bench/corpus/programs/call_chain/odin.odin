package bench

inc :: proc(x: i64) -> i64 {
	return x + 1
}

dbl :: proc(x: i64) -> i64 {
	return x + x
}

step :: proc(x: i64) -> i64 {
	return dbl(inc(x))
}
