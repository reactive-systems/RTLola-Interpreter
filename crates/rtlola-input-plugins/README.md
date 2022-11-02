![RTLola logo](https://pages.cispa.de/rtlola/assets/img/logos/rtlola-logo-ultrawide-blue.png)
# RTLola Interpreter Input Plugins
[![Crate](https://img.shields.io/crates/v/rtlola-input-plugins.svg)](https://crates.io/crates/rtlola-input-plugins)
[![API](https://docs.rs/rtlola-input-plugins/badge.svg)](https://docs.rs/rtlola-input-plugins)
[![License](https://img.shields.io/crates/l/rtlola-input-plugins)](https://crates.io/crates/rtlola-input-plugins)

RTLola is a runtime monitoring framework.  It consists of a parser, analyzer, and interpreter for the RTLola specification language.
This crate contains a CLI interface to the interpreter capable of reading csv and pcap files.

For more information on the RTLola framework make sure to visit our Website:
[rtlola.org](https://rtlola.org "RTLola")

# Input Plugins
This crate contains multiple input plugins to be used with the `rtlola-interpreter`.
Right now, it supports CSV and PCAP files.
Each plugin (or input method) is marked with a feature flag, so only the needed input variants can be included.
By default, all input plugins are included.

# Copyright

Copyright (C) CISPA - Helmholtz Center for Information Security 2021.  Authors: Jan Baumeister, Florian Kohn, Stefan Oswald, Maximilian Schwenger.
Based on original work at Universität des Saarlandes (C) 2020.  Authors: Jan Baumeister, Florian Kohn, Malte Schledjewski, Maximilian Schwenger, Marvin Stenger, and Leander Tentrup.
