#![feature(test)]

extern crate test;

use rtlola_interpreter::config::{AbsoluteTimeFormat, Config, Verbosity};
use rtlola_interpreter::ConfigBuilder;
use std::path::PathBuf;
use test::Bencher;

fn setup(spec: &str, trace: &str) -> Config {
    ConfigBuilder::runnable()
        .spec_file(PathBuf::from(spec))
        .offline_absolute(AbsoluteTimeFormat::UnixTimeFloat)
        .csv_file_input(PathBuf::from(trace), None, None)
        .verbosity(Verbosity::Silent)
        .absolute_output_time(AbsoluteTimeFormat::Rfc3339)
        .build()
}
#[bench]
fn endtoend_semi_complex_spec(b: &mut Bencher) {
    let config = setup("traces/spec_offline.lola", "traces/timed/trace_0.csv");
    b.iter(|| {
        config.clone().run().unwrap_or_else(|e| panic!("E2E test failed: {}", e));
    });
}
