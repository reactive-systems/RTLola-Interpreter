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

// Public exports
use std::time::Duration;

// Reexport Frontend
pub use rtlola_frontend::mir as rtlola_mir;

// Serialize and Deserialize traits for serde support
pub use crate::api::monitor;
pub use crate::api::monitor::Monitor;
#[cfg(feature = "queued-api")]
pub use crate::api::queued;
#[cfg(feature = "queued-api")]
pub use crate::api::queued::QueuedMonitor;
pub use crate::configuration::config_builder::ConfigBuilder;
pub use crate::configuration::{config, time};
pub use crate::storage::Value;

mod api;
mod closuregen;
mod configuration;
mod evaluator;
mod schedule;
mod storage;
#[cfg(test)]
mod tests;

/// The internal time representation.
pub type Time = Duration;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// A helper trait to conditionally require a `serde::Serialize` as a trait bound when the `serde` feature is activated.
#[cfg(feature = "serde")]
pub trait CondSerialize: Serialize {}

#[cfg(not(feature = "serde"))]
/// A helper trait to conditionally require a `serde::Serialize` as a trait bound when the `serde` feature is activated.
pub trait CondSerialize {}

#[cfg(feature = "serde")]
impl<T: Serialize> CondSerialize for T {}
#[cfg(not(feature = "serde"))]
impl<T> CondSerialize for T {}

#[cfg(feature = "serde")]
/// A helper trait to conditionally require a `serde::Deserialize` as a trait bound when the `serde` feature is activated.
pub trait CondDeserialize: for<'a> Deserialize<'a> {}

#[cfg(not(feature = "serde"))]
/// A helper trait to conditionally require a `serde::Deserialize` as a trait bound when the `serde` feature is activated.
pub trait CondDeserialize {}

#[cfg(feature = "serde")]
impl<T: for<'a> Deserialize<'a>> CondDeserialize for T {}
#[cfg(not(feature = "serde"))]
impl<T> CondDeserialize for T {}
