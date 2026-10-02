#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| tessera_syntax_fuzz::cst_inv::check_bytes(data));
