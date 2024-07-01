![RTLola logo](https://pages.cispa.de/rtlola/assets/img/logos/rtlola-logo-ultrawide-blue.png)

# RTLola Interpreter IO Plugins

[![Crate](https://img.shields.io/crates/v/rtlola-io-plugins.svg)](https://crates.io/crates/rtlola-io-plugins)
[![API](https://docs.rs/rtlola-io-plugins/badge.svg)](https://docs.rs/rtlola-io-plugins)
[![License](https://img.shields.io/crates/l/rtlola-io-plugins)](https://crates.io/crates/rtlola-io-plugins)

RTLola is a runtime monitoring framework.  It consists of a parser, analyzer, and interpreter for the RTLola specification language.
This crate contains an Api to the interpreter capable of reading csv and pcap files.
It also provides an interface to parse and serialize bytes given to and coming from the `rtlola-interpreter`.

For more information on the RTLola framework make sure to visit our Website:
[rtlola.org](https://rtlola.org "RTLola")

# IO Plugins

This crate contains multiple input and output plugins to be used with the `rtlola-interpreter`.
Right now, it supports CSV, PCAP files, and a Byte Plugin to parse and serialize byte streams.
Each plugin (or input/output method) is marked with a feature flag, so only the needed input/output variants can be included.

# Copyright

Copyright (C) CISPA - Helmholtz Center for Information Security 2024.  Authors: Jan Baumeister, Florian Kohn, Stefan Oswald, Maximilian Schwenger.
Based on original work at Universität des Saarlandes (C) 2020.  Authors: Jan Baumeister, Florian Kohn, Malte Schledjewski, Maximilian Schwenger, Marvin Stenger, and Leander Tentrup.
