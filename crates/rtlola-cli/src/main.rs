use std::error::Error;
use std::fs::File;
use std::io::{stderr, stdout, BufWriter};
use std::marker::PhantomData;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{ArgGroup, Args, CommandFactory, Parser, ValueEnum};
use clap_complete::generate;
use clap_complete::shells::*;
#[cfg(feature = "public")]
use human_panic::setup_panic;
use rtlola_interpreter::config::{ExecutionMode, OfflineMode, OnlineMode};
use rtlola_interpreter::rtlola_frontend;
use rtlola_interpreter::time::{
    parse_float_time, AbsoluteFloat, AbsoluteRfc, DelayTime, OffsetFloat, OffsetNanos, RealTime, RelativeFloat,
    RelativeNanos,
};
use rtlola_io_plugins::inputs::csv_plugin::{CsvEventSource, CsvInputSourceKind};
#[cfg(feature = "pcap_interface")]
use rtlola_io_plugins::inputs::pcap_plugin::{PcapEventSource, PcapInputSource};
use rtlola_io_plugins::outputs::csv_plugin::CsvVerdictSink;
use rtlola_io_plugins::outputs::json_plugin::JsonFactory;
use rtlola_io_plugins::outputs::log_printer::LogPrinter;
use rtlola_io_plugins::outputs::VerbosityAnnotations;
use termcolor::{Ansi, NoColor};

use crate::config::{Config, EventSourceConfig, Statistics, Verbosity};
use crate::output::{OutputChannel, StatisticsVerdictSink};

mod config;
mod output;

#[derive(Parser, Debug, Clone)]
#[command(
    author,
    version,
    about,
    long_about = "RTLola is a tool to analyze and monitor Lola specifications."
)]
#[command(propagate_version = true)]
enum Cli {
    /// Parses the input file and runs semantic analysis.
    Analyze {
        /// Path to the specification
        spec: PathBuf,
    },

    #[cfg(feature = "pcap_interface")]
    /// Run the monitor for network intrusion detection
    Ids {
        /// Path to the specification
        spec: PathBuf,

        /// The local ip range given in CIDR notation
        local_network: String,

        #[command(flatten)]
        output: CliOutputChannel,

        #[command(flatten)]
        input: IdsInput,

        #[command(flatten)]
        start_time: CliStartTime,

        /// Sets the statistic verbosity
        #[arg(short, long, value_enum, default_value_t)]
        statistics: Statistics,

        /// Sets the output verbosity
        #[arg(short, long, value_enum, default_value_t)]
        verbosity: Verbosity,

        /// Set the format in which time should be represented in the output
        #[arg(short='f', long, value_enum,
        default_value_t=CliOutputTimeRepresentation::RelativeFloatSecs
        )]
        output_time_format: CliOutputTimeRepresentation,
        /// Set the formatting of the monitor output
        #[arg(long, value_enum, default_value_t)]
        output_format: CliOutputFormat,
    },

    /// Start the monitor using the given specification
    Monitor {
        /// Path to the specification
        spec: PathBuf,

        #[command(flatten)]
        input: MonitorInput,

        #[command(flatten)]
        output: CliOutputChannel,

        #[command(flatten)]
        mode: CliExecutionMode,

        #[command(flatten)]
        start_time: CliStartTime,

        /// Sets the statistic verbosity
        #[arg(short, long, value_enum, default_value_t)]
        statistics: Statistics,

        /// Sets the output verbosity
        #[arg(short, long, value_enum, default_value_t)]
        verbosity: Verbosity,

        /// Set the format in which time should be represented in the output
        #[arg(short='f', long, value_enum,
        default_value_t=CliOutputTimeRepresentation::RelativeFloatSecs
        )]
        output_time_format: CliOutputTimeRepresentation,
        /// Set the formatting of the monitor output
        #[arg(long, value_enum, default_value_t)]
        output_format: CliOutputFormat,
        /// Print debugging information for the given streams
        #[arg(long)]
        debug_streams: Vec<String>,
    },

    /// Generate a SHELL completion script and print it to stdout
    Completions {
        #[arg(value_enum, value_name = "SHELL")]
        shell: Shell,
    },
}

#[derive(ValueEnum, Copy, Clone, Debug)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    PowerShell,
    Elvish,
}
impl Shell {
    fn generate(&self) {
        let mut app = Cli::command();
        let mut fd = std::io::stdout();
        match self {
            Shell::Bash => generate(Bash, &mut app, "rtlola-interpreter", &mut fd),
            Shell::Zsh => generate(Zsh, &mut app, "rtlola-interpreter", &mut fd),
            Shell::Fish => generate(Fish, &mut app, "rtlola-interpreter", &mut fd),
            Shell::PowerShell => generate(PowerShell, &mut app, "rtlola-interpreter", &mut fd),
            Shell::Elvish => generate(Elvish, &mut app, "rtlola-interpreter", &mut fd),
        }
    }
}

#[derive(Clone, Debug, Args)]
#[command(next_help_heading = "Start Time")]
struct CliStartTime {
    /// Sets the starting time of the monitor using a unix timestamp in 'seconds.subseconds' format.
    #[arg(long="start-time-unix", value_parser = parse_float_time, group = "start-time")]
    unix: Option<Duration>,
    /// Sets the starting time of the monitor using a timestamp in RFC3339 format.
    #[arg(long = "start-time-rfc3339", value_parser = humantime::parse_rfc3339,  group = "start-time")]
    rfc: Option<SystemTime>,
}

#[cfg(feature = "pcap_interface")]
#[derive(Clone, Debug, Args)]
#[command(next_help_heading = "Input Source")]
#[command(group(
ArgGroup::new("ids_input")
.required(true)
.args(&["pcap_in", "interface"])
))]
struct IdsInput {
    /// Use the specified pcap file as input source
    #[clap(short, long)]
    pcap_in: Option<PathBuf>,
    /// Use the specified network interface as input source
    #[clap(short, long = "iface")]
    interface: Option<String>,
    /// Specifies a delay in ms to apply between two packets in the input file.
    #[clap(long, requires = "pcap_in")]
    input_delay: Option<u64>,
}

#[derive(Clone, Debug, Args)]
#[command(next_help_heading = "Input Source")]
#[command(group(
ArgGroup::new("monitor_input")
.required(true)
.args(&["csv_in", "stdin"])
))]
struct MonitorInput {
    /// Use the specified CSV file as input source
    #[arg(long)]
    csv_in: Option<PathBuf>,
    /// Use the StdIn as input source
    #[arg(long)]
    stdin: bool,
    /// Specifies a delay in ms to apply between two events in the input file.
    #[arg(long, requires = "csv_in")]
    input_delay: Option<u64>,
    /// The column in the CSV that contains time information.
    #[arg(long, requires = "csv_in")]
    csv_time_column: Option<usize>,
}

#[derive(Clone, Copy, Debug, Args)]
#[command(next_help_heading = "Execution Mode")]
#[command(group(
ArgGroup::new("mode")
.required(true)
.args(&["online", "offline"])
))]
struct CliExecutionMode {
    /// The time of input events is taken by the monitor
    #[arg(long, requires = "stdin")]
    online: bool,

    /// The time of input events is taken from the source in the given format.
    #[arg(long, value_enum, value_name = "TIME FORMAT")]
    offline: Option<CliInputTimeRepresentation>,
}

#[derive(Clone, Debug, Args)]
#[command(next_help_heading = "Output Channel")]
struct CliOutputChannel {
    /// Print output to StdOut (default)
    #[arg(long, group = "output")]
    stdout: bool,
    /// Print output to StdErr
    #[arg(long, group = "output")]
    stderr: bool,
    /// Print output to file
    #[arg(long, group = "output")]
    output_file: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CliInputTimeRepresentation {
    /// Short for relative-float-secs.
    Relative,
    /// Short for relative-uint-nanos.
    RelativeNanos,
    /// Time represented as the unsigned number of nanoseconds relative to a fixed start time.
    RelativeUintNanos,
    /// Short for relative-float-secs.
    RelativeSecs,
    /// Time represented as a positive real number representing seconds and sub-seconds relative to a fixed start time.
    /// ie. 5.2
    RelativeFloatSecs,
    /// Short for offset-float-secs.
    Offset,
    /// Short for offset-uint-nanos.
    OffsetNanos,
    /// Time represented as the unsigned number in nanoseconds as the offset to the preceding event.
    OffsetUintNanos,
    /// Short for offset-float-secs.
    OffsetSecs,
    /// Time represented as a positive real number representing seconds and sub-seconds as the offset to the preceding event.
    OffsetFloatSecs,
    /// Short for absolute-unix.
    Absolute,
    /// Time represented as wall clock time given as a positive real number representing seconds and sub-seconds since the start of the Unix Epoch.
    AbsoluteUnix,
    /// Short for absolute-rfc3339.
    AbsoluteRfc,
    /// Time represented as wall clock time in RFC3339 format.
    AbsoluteRfc3339,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CliOutputTimeRepresentation {
    /// Short for relative-float-secs.
    Relative,
    /// Short for relative-uint-nanos.
    RelativeNanos,
    /// Time represented as the unsigned number of nanoseconds relative to a fixed start time.
    RelativeUintNanos,
    /// Short for relative-float-secs.
    RelativeSecs,
    /// Time represented as a positive real number representing seconds and sub-seconds relative to a fixed start time.
    /// ie. 5.2
    RelativeFloatSecs,
    /// Short for absolute-unix.
    Absolute,
    /// Time represented as wall clock time given as a positive real number representing seconds and sub-seconds since the start of the Unix Epoch.
    AbsoluteUnix,
    /// Short for absolute-rfc3339.
    AbsoluteRfc,
    /// Time represented as wall clock time in RFC3339 format.
    AbsoluteRfc3339,
}

#[derive(Clone, Copy, Debug, ValueEnum, Default)]
enum CliOutputFormat {
    /// Print the output in a line based logging format.
    #[default]
    Logger,
    /// Print each verdict as a JSON object.
    Json,
    /// Print each verdict as a row in CSV format.
    Csv,
}

impl From<MonitorInput> for EventSourceConfig {
    fn from(input: MonitorInput) -> Self {
        if input.stdin {
            EventSourceConfig::Csv {
                time_col: input.csv_time_column,
                kind: CsvInputSourceKind::StdIn,
            }
        } else {
            EventSourceConfig::Csv {
                time_col: input.csv_time_column,
                kind: CsvInputSourceKind::File(input.csv_in.unwrap()),
            }
        }
    }
}

#[cfg(feature = "pcap_interface")]
impl IdsInput {
    fn into_event_source(self, local_net: String) -> EventSourceConfig {
        if let Some(pcap) = self.pcap_in {
            EventSourceConfig::Pcap(PcapInputSource::File {
                path: pcap,
                local_network: local_net,
            })
        } else {
            EventSourceConfig::Pcap(PcapInputSource::Device {
                name: self.interface.unwrap(),
                local_network: local_net,
            })
        }
    }
}

impl From<CliOutputChannel> for OutputChannel {
    fn from(output: CliOutputChannel) -> Self {
        if output.stdout {
            OutputChannel::StdOut
        } else if output.stderr {
            OutputChannel::StdErr
        } else if let Some(file) = output.output_file {
            OutputChannel::File(file)
        } else {
            OutputChannel::StdOut
        }
    }
}

impl From<CliStartTime> for Option<SystemTime> {
    fn from(st: CliStartTime) -> Self {
        st.rfc.or_else(|| st.unix.map(|d| UNIX_EPOCH + d))
    }
}

macro_rules! run_config {
    ($it:expr, $ot: expr, $ir: expr, $source: expr, $statistics: expr, $verbosity: expr, $output: expr, $mode: ty, $start_time: expr, $of: expr, $annotations:expr) => {
        match $it {
            CliInputTimeRepresentation::RelativeNanos | CliInputTimeRepresentation::RelativeUintNanos => {
                run_config_it!(
                    RelativeNanos::default(),
                    $ot,
                    $ir,
                    $source,
                    $statistics,
                    $verbosity,
                    $output,
                    $mode,
                    $start_time,
                    $of,
                    $annotations
                )
            },
            CliInputTimeRepresentation::Relative
            | CliInputTimeRepresentation::RelativeSecs
            | CliInputTimeRepresentation::RelativeFloatSecs => {
                run_config_it!(
                    RelativeFloat::default(),
                    $ot,
                    $ir,
                    $source,
                    $statistics,
                    $verbosity,
                    $output,
                    $mode,
                    $start_time,
                    $of,
                    $annotations
                )
            },
            CliInputTimeRepresentation::OffsetNanos | CliInputTimeRepresentation::OffsetUintNanos => {
                run_config_it!(
                    OffsetNanos::default(),
                    $ot,
                    $ir,
                    $source,
                    $statistics,
                    $verbosity,
                    $output,
                    $mode,
                    $start_time,
                    $of,
                    $annotations
                )
            },
            CliInputTimeRepresentation::Offset
            | CliInputTimeRepresentation::OffsetSecs
            | CliInputTimeRepresentation::OffsetFloatSecs => {
                run_config_it!(
                    OffsetFloat::default(),
                    $ot,
                    $ir,
                    $source,
                    $statistics,
                    $verbosity,
                    $output,
                    $mode,
                    $start_time,
                    $of,
                    $annotations
                )
            },
            CliInputTimeRepresentation::Absolute | CliInputTimeRepresentation::AbsoluteUnix => {
                run_config_it!(
                    AbsoluteFloat::default(),
                    $ot,
                    $ir,
                    $source,
                    $statistics,
                    $verbosity,
                    $output,
                    $mode,
                    $start_time,
                    $of,
                    $annotations
                )
            },
            CliInputTimeRepresentation::AbsoluteRfc | CliInputTimeRepresentation::AbsoluteRfc3339 => {
                run_config_it!(
                    AbsoluteRfc::default(),
                    $ot,
                    $ir,
                    $source,
                    $statistics,
                    $verbosity,
                    $output,
                    $mode,
                    $start_time,
                    $of,
                    $annotations
                )
            },
        }
    };
}

macro_rules! run_config_it {
    ($it:expr, $ot: expr, $ir: expr, $source: expr, $statistics: expr, $verbosity: expr, $output: expr, $mode: ty, $start_time: expr, $of: expr, $annotations:expr) => {
        match $ot {
            CliOutputTimeRepresentation::RelativeNanos | CliOutputTimeRepresentation::RelativeUintNanos => {
                run_config_it_ot!(
                    $it,
                    RelativeNanos,
                    $ir,
                    $source,
                    $statistics,
                    $verbosity,
                    $output,
                    $mode,
                    $start_time,
                    $of,
                    $annotations
                )
            },
            CliOutputTimeRepresentation::Relative
            | CliOutputTimeRepresentation::RelativeSecs
            | CliOutputTimeRepresentation::RelativeFloatSecs => {
                run_config_it_ot!(
                    $it,
                    RelativeFloat,
                    $ir,
                    $source,
                    $statistics,
                    $verbosity,
                    $output,
                    $mode,
                    $start_time,
                    $of,
                    $annotations
                )
            },
            CliOutputTimeRepresentation::Absolute | CliOutputTimeRepresentation::AbsoluteUnix => {
                run_config_it_ot!(
                    $it,
                    AbsoluteFloat,
                    $ir,
                    $source,
                    $statistics,
                    $verbosity,
                    $output,
                    $mode,
                    $start_time,
                    $of,
                    $annotations
                )
            },
            CliOutputTimeRepresentation::AbsoluteRfc | CliOutputTimeRepresentation::AbsoluteRfc3339 => {
                run_config_it_ot!(
                    $it,
                    AbsoluteRfc,
                    $ir,
                    $source,
                    $statistics,
                    $verbosity,
                    $output,
                    $mode,
                    $start_time,
                    $of,
                    $annotations
                )
            },
        }
    };
}

macro_rules! run_config_it_ot {
    ($it:expr, $ot:ty, $ir: expr, $source: expr, $statistics: expr, $verbosity: expr, $output: expr, $mode: ty, $start_time: expr, $of: expr, $annotations:expr) => {
        match $source {
            EventSourceConfig::Csv { time_col, kind } => {
                let src: CsvEventSource<_> = CsvEventSource::setup(time_col, kind, &$ir)?;
                run_config_it_ot_src!(
                    $it,
                    $ot,
                    $ir,
                    src,
                    $statistics,
                    $verbosity,
                    $output,
                    $mode,
                    $start_time,
                    $of,
                    $annotations
                )
            },
            #[cfg(feature = "pcap_interface")]
            EventSourceConfig::Pcap(cfg) => {
                let src: PcapEventSource<_> = PcapEventSource::setup(&cfg)?;
                run_config_it_ot_src!(
                    $it,
                    $ot,
                    $ir,
                    src,
                    $statistics,
                    $verbosity,
                    $output,
                    $mode,
                    $start_time,
                    $of,
                    $annotations
                )
            },
        }
    };
}

macro_rules! run_config_it_ot_src {
    ($it:expr, $ot:ty, $ir: expr, $source: expr, $statistics: expr, $verbosity: expr, $output: expr, $mode: ty, $start_time: expr, $of: expr, $annotations:expr) => {
        match $output {
            OutputChannel::StdOut => {
                run_config_it_ot_src2!(
                    $it,
                    $ot,
                    $ir,
                    $source,
                    $statistics,
                    $verbosity,
                    stdout(),
                    $mode,
                    $start_time,
                    $of,
                    atty::is(atty::Stream::Stdout),
                    $annotations
                )
            },
            OutputChannel::StdErr => {
                run_config_it_ot_src2!(
                    $it,
                    $ot,
                    $ir,
                    $source,
                    $statistics,
                    $verbosity,
                    stderr(),
                    $mode,
                    $start_time,
                    $of,
                    atty::is(atty::Stream::Stderr),
                    $annotations
                )
            },
            OutputChannel::File(f) => {
                let file = File::create(f.as_path()).expect("Could not open output file");
                let writer = BufWriter::new(file);
                run_config_it_ot_src2!(
                    $it,
                    $ot,
                    $ir,
                    $source,
                    $statistics,
                    $verbosity,
                    writer,
                    $mode,
                    $start_time,
                    $of,
                    false,
                    $annotations
                )
            },
        }
    };
}

macro_rules! run_config_it_ot_src2 {
    ($it:expr, $ot:ty, $ir: expr, $source: expr, $statistics: expr, $verbosity: expr, $output: expr, $mode: ty, $start_time: expr, $of: expr, $colored: expr, $annotations:expr) => {{
        match $of {
            CliOutputFormat::Logger if $colored => {
                let sink = LogPrinter::<_, Ansi<_>>::new_with_annotations($verbosity.try_into()?, &$ir, $annotations)?
                    .sink($output);
                run_config_it_ot_src_of!($it, $ot, $ir, $source, $statistics, $mode, $start_time, sink)
            },
            CliOutputFormat::Logger => {
                let sink =
                    LogPrinter::<_, NoColor<_>>::new_with_annotations($verbosity.try_into()?, &$ir, $annotations)?
                        .sink($output);
                run_config_it_ot_src_of!($it, $ot, $ir, $source, $statistics, $mode, $start_time, sink)
            },
            CliOutputFormat::Json => {
                let sink = JsonFactory::new_with_annotations(&$ir, $verbosity.try_into()?, $annotations)?.sink($output);
                run_config_it_ot_src_of!($it, $ot, $ir, $source, $statistics, $mode, $start_time, sink)
            },
            CliOutputFormat::Csv => {
                let sink = CsvVerdictSink::for_verbosity(&$ir, $output, $verbosity.try_into()?, $annotations)?;
                run_config_it_ot_src_of!($it, $ot, $ir, $source, $statistics, $mode, $start_time, sink)
            },
        }
    }};
}

macro_rules! run_config_it_ot_src_of {
    ($it:expr, $ot:ty, $ir: expr, $source: expr, $statistics: expr, $mode: ty, $start_time: expr, $of: expr) => {{
        let stats_sink = match $statistics {
            Statistics::All => Some(StatisticsVerdictSink::new($ir.triggers.len(), stderr())),
            Statistics::None => None,
        };
        Config {
            ir: $ir,
            source: $source,
            mode: <$mode as ExecutionMode>::new($it),
            output_time_representation: PhantomData::<$ot>::default(),
            start_time: $start_time,
            verdict_sink: $of,
            stats_sink,
        }
        .run()
        .map(|_| ())
    }};
}

fn monitor(
    spec: PathBuf,
    input: MonitorInput,
    output: CliOutputChannel,
    mode: CliExecutionMode,
    start_time: CliStartTime,
    statistics: Statistics,
    verbosity: Verbosity,
    output_time_format: CliOutputTimeRepresentation,
    output_format: CliOutputFormat,
    debug_streams: Vec<String>,
) -> Result<(), Box<dyn Error>> {
    let config = rtlola_frontend::ParserConfig::from_path(spec).unwrap_or_else(|e| {
        eprintln!("{}", e);
        std::process::exit(1)
    });
    let handler = rtlola_frontend::Handler::from(&config);
    let ir = rtlola_frontend::parse(&config).unwrap_or_else(|e| {
        handler.emit_error(&e);
        std::process::exit(1);
    });

    let annotations = match ir
        .validate_tags(VerbosityAnnotations::parsers())
        .and_then(|_| VerbosityAnnotations::new_with_debug(&ir, &debug_streams))
    {
        Ok(annotations) => annotations,
        Err(e) => {
            handler.emit_error(&e);
            std::process::exit(1);
        },
    };

    let source = EventSourceConfig::from(input.clone());

    match mode {
        CliExecutionMode { online: true, .. } => {
            run_config_it!(
                RealTime::default(),
                output_time_format,
                ir,
                source,
                statistics,
                verbosity,
                output.into(),
                OnlineMode,
                start_time.into(),
                output_format,
                annotations
            )?;
        },
        CliExecutionMode { offline: Some(it), .. } => {
            if let Some(d) = input.input_delay {
                run_config_it!(
                    DelayTime::new(Duration::from_millis(d)),
                    output_time_format,
                    ir,
                    source,
                    statistics,
                    verbosity,
                    output.into(),
                    OfflineMode<_>,
                    start_time.into(),
                    output_format,
                    annotations
                )?;
            } else {
                run_config!(
                    it,
                    output_time_format,
                    ir,
                    source,
                    statistics,
                    verbosity,
                    output.into(),
                    OfflineMode<_>,
                    start_time.into(),
                    output_format,
                    annotations
                )?;
            }
        },
        _ => unreachable!("Ensured by Clap"),
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    #[cfg(feature = "public")]
    {
        setup_panic!(Metadata {
            name: env!("CARGO_PKG_NAME").into(),
            version: env!("CARGO_PKG_VERSION").into(),
            authors: "RTLola Team <contact@rtlola.org>".into(),
            homepage: "www.rtlola.org".into(),
        });
    }

    let cli = Cli::parse();

    match cli {
        Cli::Analyze { spec } => {
            let config = rtlola_frontend::ParserConfig::from_path(spec).unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1)
            });
            let handler = rtlola_frontend::Handler::from(&config);
            match rtlola_frontend::parse(&config) {
                Ok(_) => std::process::exit(0),
                Err(e) => {
                    handler.emit_error(&e);
                    std::process::exit(1)
                },
            }
        },
        Cli::Monitor {
            spec,
            input,
            output,
            mode,
            start_time,
            statistics,
            verbosity,
            output_time_format,
            output_format,
            debug_streams,
        } => {
            if let Err(e) = monitor(
                spec,
                input,
                output,
                mode,
                start_time,
                statistics,
                verbosity,
                output_time_format,
                output_format,
                debug_streams,
            ) {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },

        #[cfg(feature = "pcap_interface")]
        Cli::Ids {
            spec,
            local_network,
            output,
            input,
            start_time,
            statistics,
            verbosity,
            output_time_format,
            output_format,
        } => {
            let config = rtlola_frontend::ParserConfig::from_path(spec).unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1)
            });
            let handler = rtlola_frontend::Handler::from(&config);
            let ir = rtlola_frontend::parse(&config).unwrap_or_else(|e| {
                handler.emit_error(&e);
                std::process::exit(1);
            });

            let annotations = match ir
                .validate_tags(VerbosityAnnotations::parsers())
                .and_then(|_| VerbosityAnnotations::new(&ir))
            {
                Ok(annotations) => annotations,
                Err(e) => {
                    handler.emit_error(&e);
                    std::process::exit(1);
                },
            };

            let source = input.clone().into_event_source(local_network);

            if input.interface.is_some() {
                run_config_it!(
                    RealTime::default(),
                    output_time_format,
                    ir,
                    source,
                    statistics,
                    verbosity,
                    output.into(),
                    OnlineMode,
                    start_time.into(),
                    output_format,
                    annotations
                )?;
            } else if let Some(d) = input.input_delay {
                run_config_it!(
                    DelayTime::new(Duration::from_millis(d)),
                    output_time_format,
                    ir,
                    source,
                    statistics,
                    verbosity,
                    output.into(),
                    OfflineMode<_>,
                    start_time.into(),
                    output_format,
                    annotations
                )?;
            } else {
                run_config_it!(
                    AbsoluteFloat::default(),
                    output_time_format,
                    ir,
                    source,
                    statistics,
                    verbosity,
                    output.into(),
                    OfflineMode<_>,
                    start_time.into(),
                    output_format,
                    annotations
                )?;
            }
        },

        Cli::Completions { shell } => shell.generate(),
    }
    Ok(())
}
