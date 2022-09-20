#![feature(test)]

extern crate test;

use std::path::PathBuf;

use rtlola_interpreter::config::{Config, Verbosity};
use rtlola_interpreter::time::{AbsoluteFloat, AbsoluteRfc};
use rtlola_interpreter::ConfigBuilder;
use test::Bencher;

fn setup(spec: &str, trace: &str) -> Config<AbsoluteFloat, AbsoluteRfc> {
    ConfigBuilder::runnable()
        .spec_file(PathBuf::from(spec))
        .input_time::<AbsoluteFloat>()
        .offline()
        .csv_file_input(PathBuf::from(trace), None)
        .verbosity(Verbosity::Silent)
        .output_time::<AbsoluteRfc>()
        .build()
}

#[bench]
fn endtoend_semi_complex_spec(b: &mut Bencher) {
    let config = setup("traces/spec_offline.lola", "traces/timed/trace_0.csv");
    b.iter(|| {
        config
            .clone()
            .run()
            .unwrap_or_else(|e| panic!("E2E test failed: {}", e));
    });
}
