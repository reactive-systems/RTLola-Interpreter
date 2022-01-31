#![forbid(unused_must_use)] // disallow discarding errors
#![warn(
    // missing_docs,
    missing_debug_implementations,
    missing_copy_implementations,
    trivial_casts,
    trivial_numeric_casts,
    unsafe_code,
    unstable_features,
    unused_import_braces,
    unused_qualifications
)]

mod basics;
mod closuregen;
mod coordination;
mod evaluator;
mod storage;
#[cfg(test)]
mod tests;

use crate::basics::OutputHandler;
use crate::coordination::Controller;
#[cfg(feature = "pcap_interface")]
pub use basics::PCAPInputSource;
use rtlola_frontend::mir::RtLolaMir;
use std::sync::Arc;

pub use crate::basics::{
    AbsoluteTimeFormat, CsvInputSource, EvalConfig, EventSourceConfig, ExecutionMode, OutputChannel,
    RelativeTimeFormat, Statistics, Time, TimeRepresentation, Verbosity,
};
pub use crate::coordination::{
    monitor::{Incremental, Total, TriggerMessages, TriggersWithInfoValues, VerdictRepresentation, Verdicts},
    Event, Monitor,
};
pub use crate::storage::Value;

// TODO add example to doc

/**
`Config` combines an RTLola specification in `LolaIR` form with an `EvalConfig`.

The evaluation configuration describes how the specification should be executed.
The `Config` can then be turned into a monitor for use via the API or simply executed.
*/
#[derive(Debug, Clone)]
pub struct Config {
    cfg: EvalConfig,
    ir: RtLolaMir,
}

impl Config {
    /**
    Creates a new `Config`
    */
    pub fn new(cfg: EvalConfig, ir: RtLolaMir) -> Config {
        Config { cfg, ir }
    }

    /**
    Turns a `Config` that was created through a call to `new_api` into a `Monitor`.
    */
    pub fn as_api<V: VerdictRepresentation>(self) -> Monitor<V> {
        assert!(matches!(self.cfg.mode, ExecutionMode::Offline(_)));
        Monitor::setup(self.ir, self.cfg)
    }

    /**
    Runs a `Config` that was created through a call to `new`.
    */
    pub fn run(self) -> Result<Arc<OutputHandler>, Box<dyn std::error::Error>> {
        // TODO: Rather than returning OutputHandler publicly --- let alone an Arc ---, transform into more suitable format or make OutputHandler more accessible.
        Controller::new(self.ir, self.cfg).start()
    }
}
