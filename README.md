![RTLola logo](https://pages.cispa.de/rtlola/assets/img/logos/rtlola-logo-ultrawide-blue.png)
# RTLola Interpreter Repository

RTLola is a runtime monitoring framework.  It consists of a parser, analyzer, and interpreter for the RTLola specification language.

The project is split into two crates. The interpreter crate provides a library for interpreting RTLola specifications.
The CLI crate provides a command line interface to the interpreter.

## RTLola Interpreter
[![Crate](https://img.shields.io/crates/v/rtlola-interpreter.svg)](https://crates.io/crates/rtlola-interpreter)
[![API](https://docs.rs/rtlola-interpreter/badge.svg)](https://docs.rs/rtlola-interpreter)
[![License](https://img.shields.io/crates/l/rtlola-interpreter)](https://crates.io/crates/rtlola-interpreter)

This library crate provides two APIs to evaluate RTLola specifications through interpretation.

## RTLola Interpreter CLI
[![Crate](https://img.shields.io/crates/v/rtlola-cli.svg)](https://crates.io/crates/rtlola-cli)
[![API](https://docs.rs/rtlola-cli/badge.svg)](https://docs.rs/rtlola-cli)
[![License](https://img.shields.io/crates/l/rtlola-cli)](https://crates.io/crates/rtlola-cli)

This crate contains a CLI interface to the interpreter capable of reading csv and pcap files.

# Copyright

Copyright (C) CISPA - Helmholtz Center for Information Security 2021.  Authors: Jan Baumeister, Florian Kohn, Stefan Oswald, Maximilian Schwenger.
Based on original work at Universität des Saarlandes (C) 2020.  Authors: Jan Baumeister, Florian Kohn, Malte Schledjewski, Maximilian Schwenger, Marvin Stenger, and Leander Tentrup.
