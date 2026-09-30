package bench

St :: enum {
	Idle,
	Run,
	Done,
}

next :: proc(s: St) -> St {
	switch s {
	case .Idle:
		return .Run
	case .Run:
		return .Done
	case:
		return .Idle
	}
}
