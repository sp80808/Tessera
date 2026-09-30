use tessera_hir::{dump, lower, print_tc};
use tessera_phases::FileId;
use tessera_syntax::cst::parse_file;

#[test]
fn show() {
    for src in [
        "f add(a:i64,b:i64)>i64=a+b\n",
        "f x()>i64=",
        "f x()>i64= \n",
        "f x(a:i64,)>i64=a",
        "f 1()>i64=1",
        "f x(a:1)>i64=a",
        "f x(a)>i64=a",
        "f x(a:i64)>i64=(a",
        "f x(a:i64)>i64=a+",
        "f x()>i64=99999999999999999999",
        "f s(a:i64,b:i64,c:i64)>i64=a+(b+c)",
        "f s(a:i64,b:i64)>i64=(a)+(b)",
        "f x()>i64=1 2 3",
        "add(a:i64)>i64=a",
        "",
    ] {
        let p = parse_file(FileId(0), src);
        println!("=== {src:?}\n{}", p.value.dump(src));
        println!(
            "parse diags: {:?}",
            p.diagnostics
                .iter()
                .map(|d| (d.code, d.message.clone()))
                .collect::<Vec<_>>()
        );
        let out = lower(&p.value, src);
        println!("{}", dump(&out.value.module, &out.value.provenance));
        println!(
            "hir diags: {:?}",
            out.diagnostics
                .iter()
                .map(|d| (d.code, d.message.clone(), d.at))
                .collect::<Vec<_>>()
        );
        println!("print: {:?}", print_tc(&out.value.module));
    }
}
