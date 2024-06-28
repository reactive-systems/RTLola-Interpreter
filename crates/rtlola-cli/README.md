![RTLola logo](https://pages.cispa.de/rtlola/assets/img/logos/rtlola-logo-ultrawide-blue.png)
# RTLola Interpreter CLI
[![Crate](https://img.shields.io/crates/v/rtlola-cli.svg)](https://crates.io/crates/rtlola-cli)
[![API](https://docs.rs/rtlola-cli/badge.svg)](https://docs.rs/rtlola-cli)
[![License](https://img.shields.io/crates/l/rtlola-cli)](https://crates.io/crates/rtlola-cli)

RTLola is a runtime monitoring framework.  It consists of a parser, analyzer, and interpreter for the RTLola specification language.
This crate contains a CLI interface to the interpreter capable of reading csv and pcap files.

For detailed usage instructions try:

`rtlola-cli help`

For more information on the RTLola framework make sure to visit our Website:
[rtlola.org](https://rtlola.org "RTLola")

## Installation Notes

If you want to use the network interface make sure to compile with the `pcap_interface` feature enable. In that case the  PCAP library is required. If it is not already installed on your system you can do so as follows:

### Windows

You can download and install the library from here:
[NPcap](https://nmap.org/npcap/)

### Linux

Use the packet manager of your choice to install the `libpcap-dev` package. For example using `apt`:

`apt install libpcap-dev`

### Mac OS

The PCAP library is already be included in Mac OS X.

## Command Line Usage

### Specification Analysis

```
rtlola-cli analyze [SPEC]
```

checks whether the given specification is valid

### Monitoring

```
rtlola-cli monitor [SPEC] --offline relative --csv-in [TRACE] --verbosity trigger
```

For example, given the specification

```
input a: Int64
input b: Int64

output x := a + b
trigger x > 2
```

in file `example.spec` and the CSV

```
a,b,time
0,1,0.1
2,3,0.2
4,5,0.3
```

in file `example.csv` we get

```
rtlola-interpreter monitor example.spec --offline relative --csv-in example.csv 
Trigger: x > 2
Trigger: x > 2
```


See all available options with `rtlola-cli help monitor`

### Time Representations
The RTLola interpreter supports multiple representations of time in its input and output.
If run in offline mode, meaning the time for an event is parsed from the input source, 
the format in which the time is present in the input has to be set. The following options are supported:

#### Relative Timestamps
Time is considered as timestamps relative to a fixed point in time. Call this point in time `x` then in the example above
the first event gets the timestamp `x + 0.1`, the second one `x + 0.2` and so forth.

#### Absolute Timestamps
Time is parsed as absolute wall clock timestamps.

**Note**: The evaluation of periodic streams depends on the time passed between events.
Depending on the representation, determining the time that passed before the first event is not obvious.
While the relative and offset representations do not strictly need a point of reference to determine
the time passed, the absolute representation requires such a point of reference.
This point of time can either be directly supplied by the command line arguments: `--start-time-unix` and `--start-time-rfc3339`
or inferred as the time of the first event.
The latter consequently assumes that no time has passed before the first event in the input.

#### Offset
Time is considered as an offset to the preceding event. This induces the following *timestamps* for the above example:
```
a,b, time
0,1, x + 0.1
2,3, x + 0.3
4,5, x + 0.6
```

# Copyright

Copyright (C) CISPA - Helmholtz Center for Information Security 2024.  Authors: Jan Baumeister, Florian Kohn, Stefan Oswald, Frederik Scheerer, Maximilian Schwenger.
Based on original work at Universität des Saarlandes (C) 2020.  Authors: Jan Baumeister, Florian Kohn, Malte Schledjewski, Maximilian Schwenger, Marvin Stenger, and Leander Tentrup.
