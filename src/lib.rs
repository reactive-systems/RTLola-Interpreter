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

pub mod basics;
mod closuregen;
mod coordination;
mod evaluator;
mod storage;
#[cfg(test)]
mod tests;

use crate::basics::config::{EvalConfig, ExecutionMode};
use crate::basics::OutputHandler;
use crate::config::Config;
use crate::coordination::Controller;
use crate::monitor::{Input, VerdictRepresentation};
use rtlola_frontend::mir::RtLolaMir;
use std::sync::Arc;

// Public exports
pub use crate::basics::config;
pub use crate::coordination::monitor;
pub use crate::coordination::monitor::Monitor;
pub use crate::storage::Value;

// TODO add example to doc
impl Config {
    /**
    Creates a new `Config` which can then be run directly or turned into a `Monitor` by `as_api`.
    */
    pub fn new(cfg: EvalConfig, ir: RtLolaMir) -> Config {
        Config { cfg, ir }
    }

    /**
    Turns a `Config` that was created through a call to `new_api` into a `Monitor`.
    */
    pub fn as_api<S: Input, V: VerdictRepresentation>(self) -> Monitor<S, V> {
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
