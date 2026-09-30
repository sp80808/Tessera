package bench

calc :: proc(a, b: i64) -> i64 {
	s := a + b
	t := s * a
	return t - b
}
