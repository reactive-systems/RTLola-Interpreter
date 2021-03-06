# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

