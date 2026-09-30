package bench

Buf :: struct {
	n: i64,
}

eat :: proc(b: Buf) -> i64 {
	return b.n
}

feed :: proc(b: Buf) -> i64 {
	return eat(b)
}
