![RTLola logo](https://pages.cispa.de/rtlola/assets/img/logos/rtlola-logo-ultrawide-blue.png)
# RTLola Interpreter
[![Crate](https://img.shields.io/crates/v/rtlola-interpreter.svg)](https://crates.io/crates/rtlola-interpreter)
[![API](https://docs.rs/rtlola-interpreter/badge.svg)](https://docs.rs/rtlola-interpreter)
[![License](https://img.shields.io/crates/l/rtlola-interpreter)](https://crates.io/crates/rtlola-interpreter)

RTLola is a runtime monitoring framework. It consists of a parser, analyzer, and interpreter for the RTLola specification language.

This library crate provides two APIs to evaluate RTLola specifications through interpretation.
If you are looking for a ready to use tool try out the `rtlola-cli` crate, which provides a command line interface to the interpreter capable of parsing csv and pcap files.

For more information on the RTLola framework make sure to visit our Website:
[rtlola.org](https://rtlola.org "RTLola")

## The RTLola language

An example for a RTLola specification is given below:

```
input a: Int64
input b: Int64

output x := a + b
trigger x > 2
```

Evaluated on a trace given in CSV format:

```
a,b,time
0,1,0.1
2,3,0.2
4,5,0.3
```

the interpreter will produce an output similar to this:

```
rtlola-cli monitor example.spec --offline relative --csv-in example.csv 
Trigger: x > 2
Trigger: x > 2
```

# Copyright

Copyright (C) CISPA - Helmholtz Center for Information Security 2021.  Authors: Jan Baumeister, Florian Kohn, Stefan Oswald, Maximilian Schwenger.
Based on original work at Universität des Saarlandes (C) 2020.  Authors: Jan Baumeister, Florian Kohn, Malte Schledjewski, Maximilian Schwenger, Marvin Stenger, and Leander Tentrup.
