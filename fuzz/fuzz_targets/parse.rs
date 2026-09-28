//! The parser and HTML renderer must accept any UTF-8 input without panicking.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &str| {
    for options in [mdparse::Options::default(), mdparse::Options::commonmark()] {
        let doc = mdparse::parse_with(input, options);
        let _ = mdparse::html::render(&doc);
    }
});
