use criterion::{
    BatchSize, BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main,
};
use tessera_syntax::{expand, fmt, parse};

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");

    let small_fn = "f add(a:i64,b:i64)>i64=a+b";
    let medium_fn = "f sum(a:i64,b:i64,c:i64,d:i64,e:i64,f:i64)>i64=a+b+c+d+e+f";
    let large_fn = "f big(a:i64,b:i64,c:i64,d:i64,e:i64,f:i64,g:i64,h:i64,i:i64,j:i64)>i64=a+b+c+d+e+f+g+h+i+j";
    let very_large_fn = (0..50)
        .map(|i| format!("p{i}:i64"))
        .collect::<Vec<_>>()
        .join(",");
    let very_large_fn = format!(
        "f huge({very_large_fn})>i64={}",
        (0..50)
            .map(|i| format!("p{i}"))
            .collect::<Vec<_>>()
            .join("+")
    );

    let inputs = [
        ("small", small_fn as &str),
        ("medium", medium_fn),
        ("large", large_fn),
        ("very_large", &very_large_fn[..]),
    ];

    for (name, src) in inputs {
        let bytes = src.len();
        group.throughput(Throughput::Bytes(bytes as u64));

        group.bench_with_input(BenchmarkId::new("parse", name), src, |b, src| {
            b.iter_batched(
                || src.to_string(),
                |s| parse(black_box(&s)).expect("parse ok"),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_fmt(c: &mut Criterion) {
    let mut group = c.benchmark_group("fmt");

    let small_fn = "f add(a:i64,b:i64)>i64=a+b";
    let medium_fn = "f sum(a:i64,b:i64,c:i64,d:i64,e:i64,f:i64)>i64=a+b+c+d+e+f";
    let large_fn = "f big(a:i64,b:i64,c:i64,d:i64,e:i64,f:i64,g:i64,h:i64,i:i64,j:i64)>i64=a+b+c+d+e+f+g+h+i+j";
    let very_large_fn = (0..50)
        .map(|i| format!("p{i}:i64"))
        .collect::<Vec<_>>()
        .join(",");
    let very_large_fn = format!(
        "f huge({very_large_fn})>i64={}",
        (0..50)
            .map(|i| format!("p{i}"))
            .collect::<Vec<_>>()
            .join("+")
    );

    let inputs = [
        ("small", small_fn as &str),
        ("medium", medium_fn),
        ("large", large_fn),
        ("very_large", &very_large_fn[..]),
    ];

    for (name, src) in inputs {
        let bytes = src.len();
        group.throughput(Throughput::Bytes(bytes as u64));

        group.bench_with_input(BenchmarkId::new("fmt", name), src, |b, src| {
            b.iter_batched(
                || src.to_string(),
                |s| fmt(black_box(&s)).expect("fmt ok"),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_expand(c: &mut Criterion) {
    let mut group = c.benchmark_group("expand");

    let small_fn = "f add(a:i64,b:i64)>i64=a+b";
    let medium_fn = "f sum(a:i64,b:i64,c:i64,d:i64,e:i64,f:i64)>i64=a+b+c+d+e+f";
    let large_fn = "f big(a:i64,b:i64,c:i64,d:i64,e:i64,f:i64,g:i64,h:i64,i:i64,j:i64)>i64=a+b+c+d+e+f+g+h+i+j";
    let very_large_fn = (0..50)
        .map(|i| format!("p{i}:i64"))
        .collect::<Vec<_>>()
        .join(",");
    let very_large_fn = format!(
        "f huge({very_large_fn})>i64={}",
        (0..50)
            .map(|i| format!("p{i}"))
            .collect::<Vec<_>>()
            .join("+")
    );

    let inputs = [
        ("small", small_fn as &str),
        ("medium", medium_fn),
        ("large", large_fn),
        ("very_large", &very_large_fn[..]),
    ];

    for (name, src) in inputs {
        let bytes = src.len();
        group.throughput(Throughput::Bytes(bytes as u64));

        group.bench_with_input(BenchmarkId::new("expand", name), src, |b, src| {
            b.iter_batched(
                || src.to_string(),
                |s| expand(black_box(&s)).expect("expand ok"),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("roundtrip");

    let small_fn = "f add(a:i64,b:i64)>i64=a+b";
    let medium_fn = "f sum(a:i64,b:i64,c:i64,d:i64,e:i64,f:i64)>i64=a+b+c+d+e+f";
    let large_fn = "f big(a:i64,b:i64,c:i64,d:i64,e:i64,f:i64,g:i64,h:i64,i:i64,j:i64)>i64=a+b+c+d+e+f+g+h+i+j";
    let very_large_fn = (0..50)
        .map(|i| format!("p{i}:i64"))
        .collect::<Vec<_>>()
        .join(",");
    let very_large_fn = format!(
        "f huge({very_large_fn})>i64={}",
        (0..50)
            .map(|i| format!("p{i}"))
            .collect::<Vec<_>>()
            .join("+")
    );

    let inputs = [
        ("small", small_fn as &str),
        ("medium", medium_fn),
        ("large", large_fn),
        ("very_large", &very_large_fn[..]),
    ];

    for (name, src) in inputs {
        let bytes = src.len();
        group.throughput(Throughput::Bytes(bytes as u64));

        group.bench_with_input(BenchmarkId::new("parse_fmt_expand", name), src, |b, src| {
            b.iter_batched(
                || src.to_string(),
                |s| {
                    let ast = parse(black_box(&s)).expect("parse ok");
                    let formatted = fmt(black_box(&s)).expect("fmt ok");
                    let expanded = expand(black_box(&s)).expect("expand ok");
                    black_box((ast, formatted, expanded))
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_fmt_idempotent(c: &mut Criterion) {
    let mut group = c.benchmark_group("fmt_idempotent");

    let small_fn = "f add(a:i64,b:i64)>i64=a+b";
    let medium_fn = "f sum(a:i64,b:i64,c:i64,d:i64,e:i64,f:i64)>i64=a+b+c+d+e+f";
    let large_fn = "f big(a:i64,b:i64,c:i64,d:i64,e:i64,f:i64,g:i64,h:i64,i:i64,j:i64)>i64=a+b+c+d+e+f+g+h+i+j";

    let inputs = [
        ("small", small_fn as &str),
        ("medium", medium_fn),
        ("large", large_fn),
    ];

    for (name, src) in inputs {
        let canonical = fmt(src).expect("fmt ok");
        let bytes = canonical.len();
        group.throughput(Throughput::Bytes(bytes as u64));

        group.bench_with_input(
            BenchmarkId::new("fmt_idempotent", name),
            &canonical,
            |b, src| {
                b.iter(|| fmt(black_box(src)).expect("fmt ok"));
            },
        );
    }
    group.finish();
}

fn bench_token_count(c: &mut Criterion) {
    use std::hint::black_box;

    let mut group = c.benchmark_group("token_count");

    let small_fn = "f add(a:i64,b:i64)>i64=a+b";
    let medium_fn = "f sum(a:i64,b:i64,c:i64,d:i64,e:i64,f:i64)>i64=a+b+c+d+e+f";
    let large_fn = "f big(a:i64,b:i64,c:i64,d:i64,e:i64,f:i64,g:i64,h:i64,i:i64,j:i64)>i64=a+b+c+d+e+f+g+h+i+j";

    let inputs = [
        ("small", small_fn as &str),
        ("medium", medium_fn),
        ("large", large_fn),
    ];

    for (name, src) in inputs {
        let ast = parse(src).expect("parse ok");
        let token_count = count_tokens(&ast.body);
        group.throughput(Throughput::Elements(token_count as u64));

        group.bench_with_input(BenchmarkId::new("token_count", name), src, |b, src| {
            b.iter(|| {
                let ast = parse(black_box(src)).expect("parse ok");
                black_box(count_tokens(&ast.body))
            });
        });
    }
    group.finish();
}

fn count_tokens(expr: &tessera_syntax::AstExpr) -> usize {
    match expr {
        tessera_syntax::AstExpr::Int(_) => 1,
        tessera_syntax::AstExpr::Var(_) => 1,
        tessera_syntax::AstExpr::Add(lhs, rhs) => 1 + count_tokens(lhs) + count_tokens(rhs),
    }
}

criterion_group!(
    benches,
    bench_parse,
    bench_fmt,
    bench_expand,
    bench_roundtrip,
    bench_fmt_idempotent,
    bench_token_count,
);
criterion_main!(benches);
