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
//!
//! ## Time Representations
//! The RTLola interpreter supports multiple representations of time in its input and output.
//! If run in offline mode, meaning the time for an event is parsed from the input source,
//! the format in which the time is present in the input has to be set. Consider the following example CSV file:
//!
//! <pre>
//! a,b,time
//! 0,1,0.1
//! 2,3,0.2
//! 4,5,0.3
//! </pre>
//!
//! The supported time representations are:
//!
//! #### Relative
//! Time is considered relative to a fixed point in time. Call this point in time `x` then in the example above
//! the first event gets the timestamp `x + 0.1`, the second one `x + 0.2` and so forth.
//!
//! #### Incremental
//! Time is considered relative to the preceding event. This induces the following timestamps for the above example:
//!
//! <pre>
//! a,b, time
//! 0,1, x + 0.1
//! 2,3, x + 0.3
//! 4,5, x + 0.6
//! </pre>
//!
//! #### Absolute
//! Time is parsed as absolute timestamps.
//!
//! **Note**: The evaluation of periodic streams depends on the time passed between events.
//! Depending on the representation, determining the time that passed before the first event is not obvious.
//! While the relative and incremental representations do not strictly need a point of reference to determine
//! the time passed, the absolute representation requires such a point of reference.
//! This point of time can either be directly supplied during configuration using the [start_time](ConfigBuilder::start_time) method
//! or inferred as the time of the first event.
//! The latter consequently assumes that no time has passed before the first event in the input.

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
