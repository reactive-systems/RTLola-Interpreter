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
mod configuration;
mod coordination;
mod evaluator;
mod storage;
#[cfg(test)]
mod tests;

// Public exports
pub use crate::configuration::config;
pub use crate::configuration::config_builder::ConfigBuilder;
pub use crate::coordination::monitor;
pub use crate::coordination::monitor::Monitor;

pub use crate::storage::Value;

use std::time::Duration;

/// The internal time representation.
pub type Time = Duration;
