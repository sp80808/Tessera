enum St {
    Idle,
    Run,
    Done,
}

fn next(s: St) -> St {
    match s {
        St::Idle => St::Run,
        St::Run => St::Done,
        St::Done => St::Idle,
    }
}
