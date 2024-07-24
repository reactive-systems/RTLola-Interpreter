# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.11.0] - ?

### Added
- `FromValues` trait and `StructVerdictFactory` to easily construct custom datatypes from monitor verdicts.

### Changed
- The trigger field in `IncrementalVerdict` now contains `TriggerReferences`, not `OutputReferences`.

### Fixed
- Fix bug where `Incremental` verdict representation included trigger twice.
- Inform tracer about event parsing times also in the `QueuedMonitor`.
- Format `Value::String` with surrounding quotation marks.
- Bug in sliding windows preventing them from accepting values in the same iteration in which they are spawned.

## [0.10.1] - 02.07.2024

### Fixed
- Bug when spawning streams without spawn condition and expression.

## [0.10.0] - 28.06.2024

### Fixed
- critical Bug in handling deadlines in online mode.
- Bug when executing two monitor instances in parallel.

### Changed
- Refactored input handling types into their own file.
- Renamed most input types to be clearer.
  - `Input` -> `EventFactory`
  - `Record` -> `InputMap`
  - `DerivedInput` -> `AssociatedFactory`
  - `ValueProjection` -> `ValueGetter`
  - `RecordInput` -> `MappedFactory`
- Type Parameters of the Api to include the monitoring mode. 
- Update to trigger representation in frontend.
- Exclude trigger messages from `TotalIncrement` verdict.

### Removed
- `EventInput` in favor of the new `ArrayFactory`

### Added
- `ArrayFactory` to pass arrays of values to the monitor API.
- `VectorFactory` to pass vectors of values to the monitor API.
- `EmptyFactory` a dummy factory that always produces the empty event.
- Instance aggregations to aggregate the values of all instances of a stream.
- Support for the floating rounding function.
- Add support for multiple eval clauses.

## [0.9.0] - 19.12.2022

### Changed
- Updated to Frontend version 0.6.0
- This includes many syntax changes.

### Removed
- CLI has now its own crate `rtlola-cli`

### Added
- Support for `get()` and `is_fresh()` stream accesses.
- New Queued Api designed for online monitoring using threads and FIFO queues to send verdicts of timed streams immediately.

## [0.8.0] - 12.04.2022

### General
- This crate is now fully documented.

### Changed
- Refactored the CLI. Run `rtlola-interpreter help` for more info.
- Running the monitor in offline mode now requires the specification of the time format in the input source
- Complete rework of the API to make it easier to use.


## [0.7.0] - 23.11.2021

### Added
- `From` implementation for values
- Support for parameterization
- New window operations: last, median, nth-percentile, variance, standard deviation, covariance

## [0.6.0] - 2021-08-03

### General
- Trigger messages can now include stream names to display their value.
- Periodic streams are no longer evaluated at time 0.
- Update Api allowing different verbosities

### Added
- Trigonometric functions: tan, arcsin, arccos

## [0.5.0] - 2021-05-21

### General
- Moved to RTLola Frontend version 0.4.1

## [0.4.2] - 2021-01-08

### General
- rtlola-frontend dependency update

### Fix
- Periodic streams now work properly with different time initialization options.

## [0.4.1] - 2020-09-18

### Add
- Discrete windows

### Fix
- When using the API, StateSlice lacked information on triggers.

## [0.4.0] - 2020-08-26

### General
- Hide pcap_on_demand behind feature flag.

### Fix
- Update is public now.

## [0.3.3] - 2020-08-26

### Fixed
- EvalConfig, TimeRepresentation, TimeFormat are public now.

## [0.3.2] - 2020-04-27

### General
- Libpcap now only loaded, if the network monitoring interface is used.

### Added
- Frontend: `hold(or: default)` syntax now supported
- Evaluator: Added `forall` (`conjunction`) and `exists` (`disjunction`) window aggregations over boolean streams
- Evaluator: Implemented `sum` window aggregation over boolean streams

## [0.3.1] - 2020-03-05

### Fixed
- Evaluator: Fixed Evaluation of Min/Max

## [0.3.0] - 2020-02-27
### Added
- Evaluator: Add pcap interface (see `ids` subcommand) (requires libpcap dependency)
- Frontend: Add pcap interface (see `ids` subcommand) (requires libpcap dependency)
- Frontend: Add Float16 Type
- Language: Add bitwise operators and `&`, or `|`, xor `^`, left shift `<<`, and right shift `>>`
- Language: Add `Bytes` data type
- Language: Add method `Bytes.at(index:)`, e.g., `bytes.at(index: 0)`

### Fixed
- Evaluator: Fixed Min/Max aggregation function implementation

## [0.2.0] - 2019-08-23
### Added
- Language: it is now possible to annotate optional types using the syntax `T?` to denote optional of type `T`, e.g., `Int64?`
- Language: support `min` and `max` as aggregations for sliding windows, e.g., `x.aggregate(over: 1s, using: min)`

### Fixed
- Frontend: Fix parsing problem related to keywords, e.g., `output outputxyz` is now be parsed (was refused before)
- Frontend: Ignore [BOM](https://de.wikipedia.org/wiki/Byte_Order_Mark) at the start of specification file
- Interpreter: Fix sliding window aggregation bug


## [0.1.0] - 2019-08-12
### Added
- Initial public release: parsing, type checking, memory analysis, lowering, and evaluation of StreamLAB specifications

