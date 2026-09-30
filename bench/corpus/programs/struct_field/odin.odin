package bench

P :: struct {
	x, y: i64,
}

norm1 :: proc(p: P) -> i64 {
	return p.x + p.y
}
