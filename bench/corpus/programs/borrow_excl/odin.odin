package bench

bump :: proc(x: ^i64) {
	x^ += 1
}

run :: proc(v: i64) -> i64 {
	c := v
	bump(&c)
	return c
}
