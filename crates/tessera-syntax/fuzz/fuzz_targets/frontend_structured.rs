#![no_main]

use libfuzzer_sys::fuzz_target;
use tessera_syntax_fuzz::structured::Program;

fuzz_target!(|program: Program| tessera_syntax_fuzz::structured::check(&program));
