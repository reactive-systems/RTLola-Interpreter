//! This module contains necessary trait to interface the output of the interpreter with your datastructures.
//! The [VerdictFactory] trait represents a factory for verdicts given a [VerdictRepresentation] of the monitor.

use std::error::Error;

use crate::monitor::VerdictRepresentation;
use crate::time::OutputTimeRepresentation;

/// This trait provides the functionally to convert the monitor output.
/// You can either implement this trait for your own datatype or use one of the predefined output methods.
/// See [VerdictRepresentationFactory]
pub trait VerdictFactory<MonitorOutput: VerdictRepresentation, OutputTime: OutputTimeRepresentation> {
    /// Type of the expected Output representation
    type Verdict;
    /// Error when converting the monitor output to the verdict
    type Error: Error + 'static;

    /// This function converts a monitor to a verdict.
    fn get_verdict(&mut self, rec: MonitorOutput, ts: OutputTime::InnerTime) -> Result<Self::Verdict, Self::Error>;
}
