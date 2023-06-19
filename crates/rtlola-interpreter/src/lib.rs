//! # The RTLola Interpreter
//! The RTLola interpreter is a library used to evaluate RTLola specifications.
//! It is designed such that it easily integrates into your setup through highly configurable APIs.
//!
//! ## Features
//! - `queued-api` (Default): By default the library features a queued API that uses threads. If your target architecture doesn't support threads, consider disabling this feature through "default-features = false"
//! - `serde`: Enables Serde Serialization and Deserialization support for API interface structs.
//!
//! ## Usage
//! The main entrypoint of the application is the [ConfigBuilder].
//! It features multiple methods to configure the interpreter for your needs.
//! From there you can create a [Monitor] or [QueuedMonitor].
//! The main interaction points of the library.

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

use std::error::Error;
use std::fmt::{Display, Formatter};
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
pub use crate::storage::{Value, ValueConvertError};

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

/// Represents an error type that never occurs. This can be replaced by the `Never` type once it is stabilized.
#[derive(Debug, Copy, Clone)]
pub struct NoError {}
impl Display for NoError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "This error will never be thrown.")
    }
}
impl Error for NoError {}

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
