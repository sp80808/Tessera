package bench

peek :: proc(x: ^i64) -> i64 {
	return x^ + 1
}

look :: proc(v: i64) -> i64 {
	v := v
	return peek(&v)
}
