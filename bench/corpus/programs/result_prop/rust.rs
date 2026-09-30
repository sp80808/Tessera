fn parse(x: i64) -> Result<i64, ()> {
    if x < 0 { Err(()) } else { Ok(x) }
}

fn dbl(x: i64) -> Result<i64, ()> {
    let v = parse(x)?;
    Ok(v + v)
}
