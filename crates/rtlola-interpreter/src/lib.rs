//! # The RTLola Interpreter
//! The RTLola interpreter is designed to easily test your setup, including your specification.
//! It can either read events from trace files in CSV or PCAP format or read events in an online fashion from std-in or a network device.
//!
//! Note: The network functionality of the interpreter is only available when compiled with the `pcap_interface` feature flag.
//!
//! ## Usage
//! Besides the command line interface, the main entrypoint of the application is the [ConfigBuilder].
//! It features multiple methods to configure the interpreter for your needs.
//! From there you can either run the interpreter directly with a specified input source or create a [Monitor].
//! The main API interaction point of the application.

#![forbid(unused_must_use)] // disallow discarding errors
#![warn(
    missing_docs,
    missing_debug_implementations,
    missing_copy_implementations,
    trivial_casts,
    trivial_numeric_casts,
    unsafe_code,
    unstable_features,
    unused_import_braces,
    unused_qualifications
)]
mod api;
mod closuregen;
mod configuration;
mod evaluator;
mod schedule;
mod storage;
#[cfg(test)]
mod tests;

// Public exports
use std::time::Duration;

pub use crate::api::monitor;
pub use crate::api::monitor::Monitor;
#[cfg(feature = "queued-api")]
pub use crate::api::queued;
#[cfg(feature = "queued-api")]
pub use crate::api::queued::QueuedMonitor;
pub use crate::configuration::config_builder::ConfigBuilder;
pub use crate::configuration::{config, time};
pub use crate::storage::Value;

/// The internal time representation.
pub type Time = Duration;

// Reexport Frontend
pub use rtlola_frontend::mir as rtlola_mir;
