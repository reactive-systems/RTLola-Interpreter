# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - ?

### Changed
- Split plugins into `inputs` and `outputs` plugins.
- Rename `ByteVerdinkSink` into `BincodeSink`.
- Move `VerdictFactory` and `EventFactory` trait to interpreter.

### Added
- Add new output plugins:
  - `ByteSink` to write bytes to a writer; `DiscardSink` to discard all verdicts.
  - `JsonFactory` and `JsonSink` to write the verdicts in JSONL format.
  - `CsvFactory` and `CsvSink` to write the verdicts in CSV format.
  - `LogPrinter` factory to construct logging output.
  - `StatisticsFactory` to construct statistical information.

## [0.2.0] - 28.06.2024

### Added
- Byte Input Plugin
- VerdictSink

## [0.1.0] - 19.12.2022

### Changed
- The input plugins were split from the interpreter library now available as `rtlola-interpreter`.