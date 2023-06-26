![RTLola logo](https://pages.cispa.de/rtlola/assets/img/logos/rtlola-logo-ultrawide-blue.png)
# RTLola Interpreter Macros
[![Crate](https://img.shields.io/crates/v/rtlola-interpreter-macros.svg)](https://crates.io/crates/rtlola-interpreter-macros)
[![API](https://docs.rs/rtlola-interpreter-macros/badge.svg)](https://docs.rs/rtlola-interpreter-macros)
[![License](https://img.shields.io/crates/l/rtlola-interpreter-macros)](https://crates.io/crates/rtlola-interpreter-macros)

RTLola is a runtime monitoring framework.  It consists of a parser, analyzer, and interpreter for the RTLola specification language.
This crate contains macros relevant for using your own type as an input to the interpreter.

For more information on the RTLola framework make sure to visit our Website:
[rtlola.org](https://rtlola.org "RTLola")

# Interpreter Macros
This crate contains macros relevant for using your own type as an input to the interpreter. I.e. it contains a derive macro that implements `Record` for a given type such that the monitor can parse an event from it.

# Copyright

Copyright (C) CISPA - Helmholtz Center for Information Security 2021.  Authors: Jan Baumeister, Florian Kohn, Stefan Oswald, Maximilian Schwenger.
Based on original work at Universität des Saarlandes (C) 2020.  Authors: Jan Baumeister, Florian Kohn, Malte Schledjewski, Maximilian Schwenger, Marvin Stenger, and Leander Tentrup.
