//! The API of the RTLola interpreter. It is used to evaluate input events on a given specification and output the results.
//! There are two ways to interact with the monitor targeted at different kind of domains.
//!
//! The [monitor::Monitor] is the single threaded version of the API.
//! Consequently deadlines of timed streams are only evaluated with a new event.
//! Hence this API is more suitable for offline monitoring or embedded scenarios.
//!
//! The [queued::QueuedMonitor] is the multi-threaded version of the API.
//! Deadlines are evaluated immediately and the resulting verdicts are returned through a queue.
//! This API should be used in an online monitoring setting.
//!
//! The both structure are parameterized over their input and output method.
//! The preferred method to create an API is using the [ConfigBuilder](crate::ConfigBuilder) and the [monitor](crate::ConfigBuilder::monitor), [queued_monitor](crate::ConfigBuilder::queued_monitor) method respectively.
//!
//! # Input Method
//! An input method has to implement the [Input] trait. Out of the box two different methods are provided:
//! * [EventInput]: Provides a basic input method for anything that already is an [Event] or that can be transformed into one using `Into<Event>`.
//! * [RecordInput]: Is a more elaborate input method. It allows to provide a custom data structure to the monitor as an input, as long as it implements the [Record] trait.
//!     If implemented this traits provides functionality to generate a new value for any input stream from the data structure.
//!
//! # Output Method
//! The [Monitor] can provide output with a varying level of detail captured by the [VerdictRepresentation] trait. The different output formats are:
//! * [Incremental]: For each processed event a condensed list of monitor state changes is provided.
//! * [Total]: For each event a complete snapshot of the current monitor state is returned
//! * [TriggerMessages]: For each event a list of violated triggers with their description is produced.
//! * [TriggersWithInfoValues]: For each event a list of violated triggers with their specified corresponding values is returned.

pub mod monitor;

#[cfg(feature = "queued-api")]
pub mod queued;
