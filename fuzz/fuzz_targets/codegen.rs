//! Typst code generation must accept any UTF-8 input without panicking.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &str| {
    let _ = md2typst::convert(input, &md2typst::Options::default());
});
