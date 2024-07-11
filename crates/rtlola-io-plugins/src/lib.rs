//! This module exposes functionality to handle input and output methods of the monitor.
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

/// This module exposes functionality to handle the input methods of the monitor.
pub mod inputs;
/// This module exposes functionality to handle the output methods of the monitor.
pub mod outputs;
