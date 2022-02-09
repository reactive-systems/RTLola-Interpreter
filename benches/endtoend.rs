#![feature(test)]

extern crate test;

use rtlola_interpreter::{
    AbsoluteTimeFormat, Config, Config, CsvInputSource, EventSourceConfig, ExecutionMode, OutputChannel,
    RelativeTimeFormat, Statistics, TimeRepresentation, Verbosity,
};
use std::path::PathBuf;
use test::Bencher;

fn setup(spec: &str, trace: &str) -> Config {
    let eval_conf = Config::new(
        EventSourceConfig::Csv {
            src: CsvInputSource::file(
                PathBuf::from(trace),
                None,
                None,
                ExecutionMode::Offline(TimeRepresentation::Absolute(AbsoluteTimeFormat::UnixTimeFloat)),
            ),
        },
        Statistics::None,
        Verbosity::Silent,
        OutputChannel::None,
        ExecutionMode::Offline(TimeRepresentation::Absolute(AbsoluteTimeFormat::UnixTimeFloat)),
        TimeRepresentation::Absolute(AbsoluteTimeFormat::Rfc3339),
        None,
    );
    let config = rtlola_frontend::ParserConfig::from_path(PathBuf::from(spec)).unwrap_or_else(|e| {
        eprintln!("{}", e);
        std::process::exit(1)
    });
    let handler = rtlola_frontend::Handler::from(config.clone());
    let ir = rtlola_frontend::parse(config).unwrap_or_else(|e| {
        handler.emit_error(&e);
        std::process::exit(1);
    });
    Config::new(eval_conf, ir)
}
#[bench]
fn endtoend_semi_complex_spec(b: &mut Bencher) {
    let config = setup("traces/spec_offline.lola", "traces/timed/trace_0.csv");
    b.iter(|| {
        config.clone().run().unwrap_or_else(|e| panic!("E2E test failed: {}", e));
    });
}
