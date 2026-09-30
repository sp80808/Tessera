package bench

tri :: proc(n: i64) -> i64 {
	acc: i64 = 0
	for i in 0..<n {
		acc += i
	}
	return acc
}
