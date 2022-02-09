use clap::{AppSettings, ArgEnum, ArgGroup, Args, IntoApp, Parser};
use lazy_static::lazy_static;
use rtlola_interpreter::basics::{CsvInputSource, OutputChannel};
use rtlola_interpreter::config::{
    AbsoluteTimeFormat, Config, EventSourceConfig, ExecutionMode, RelativeTimeFormat, TimeRepresentation, Verbosity,
};

#[cfg(feature = "pcap_interface")]
use rtlola_interpreter::basics::PCAPInputSource;

use std::error::Error;
use std::fmt::Write;
use std::path::PathBuf;

use clap_complete::generate;
use clap_complete::shells::*;
#[cfg(feature = "public")]
use human_panic::setup_panic;
use std::convert::TryFrom;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

macro_rules! enum_doc {
    ($enum: ty, $heading: expr) => {{
        let pv_iter = <$enum>::value_variants().iter().filter_map(|v| v.to_possible_value());

        let max_width = pv_iter.clone().filter_map(|pv| pv.get_visible_name()).map(str::len).max().unwrap_or(0);

        let mut text: String = String::from($heading) + "\n";
        for pv in pv_iter {
            // Note: There's a final newline so that clap's default value text is put on a new line.
            writeln!(text, "• {:max_width$} — {}", pv.get_name(), pv.get_help().unwrap()).unwrap();
        }
        text
    }};
}

lazy_static! {
    static ref VERBOSITY_HELP: String = enum_doc!(Verbosity, "Output Verbosity; one of the following keywords:");
    static ref OUTPUT_FORMAT_HELP: String =
        enum_doc!(CliTimeRepresentation, "Output Time Format; one of the following keywords:");
}

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = "RTLola is a tool to analyze and monitor Lola specifications.")]
#[clap(global_setting(AppSettings::PropagateVersion))]
#[clap(global_setting(AppSettings::UseLongFormatForHelpSubcommand))]
#[clap(global_setting(AppSettings::DeriveDisplayOrder))]
enum Cli {
    /// Parses the input file and runs semantic analysis.
    Analyze {
        /// Path to the specification
        #[clap(parse(from_os_str))]
        spec: PathBuf,
    },

    #[cfg(feature = "pcap_interface")]
    /// Run the monitor for network intrusion detection
    Ids {
        /// Path to the specification
        #[clap(parse(from_os_str))]
        spec: PathBuf,

        /// The local ip range given in CIDR notation
        local_network: String,

        #[clap(flatten)]
        output: CliOutputChannel,

        #[clap(flatten)]
        input: IdsInput,

        #[clap(flatten)]
        start_time: CliStartTime,

        /// Sets the output verbosity
        #[clap(short, long, arg_enum,
            long_help=Some(VERBOSITY_HELP.as_str()),
            default_value_t
        )]
        verbosity: Verbosity,

        /// Set the format in which time should be represented in the output
        #[clap(short='f', long, arg_enum,
            long_help=Some(OUTPUT_FORMAT_HELP.as_str()),
            default_value_t=CliTimeRepresentation::RelativeFloatSecs
        )]
        output_time_format: CliTimeRepresentation,
    },

    /// Start the monitor using the given specification
    Monitor {
        /// Path to the specification
        #[clap(parse(from_os_str))]
        spec: PathBuf,

        #[clap(flatten)]
        input: MonitorInput,

        #[clap(flatten)]
        output: CliOutputChannel,

        #[clap(flatten)]
        mode: CliExecutionMode,

        #[clap(flatten)]
        start_time: CliStartTime,

        /// Sets the output verbosity
        #[clap(short, long, arg_enum,
            long_help=Some(VERBOSITY_HELP.as_str()),
            default_value_t
        )]
        verbosity: Verbosity,

        /// Set the format in which time should be represented in the output
        #[clap(short='f', long, arg_enum,
            long_help=Some(OUTPUT_FORMAT_HELP.as_str()),
            default_value_t=CliTimeRepresentation::RelativeFloatSecs
        )]
        output_time_format: CliTimeRepresentation,
    },

    /// Generate a SHELL completion script and print it to stdout
    Completions {
        #[clap(arg_enum, value_name = "SHELL")]
        shell: Shell,
    },
}

#[derive(ArgEnum, Copy, Clone, Debug)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    PowerShell,
    Elvish,
}
impl Shell {
    fn generate(&self) {
        let mut app = Cli::into_app();
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

fn parse_start_time(time: &str) -> Result<Duration, String> {
    RelativeTimeFormat::FloatSecs.parse_str(time)
}
#[derive(Clone, Debug, Args)]
#[clap(help_heading = "Start Time")]
struct CliStartTime {
    /// Sets the starting time of the monitor using a unix timestamp in 'seconds.subseconds' format.
    #[clap(long="start-time-unix", parse(try_from_str = parse_start_time), group = "start-time")]
    unix: Option<Duration>,
    /// Sets the starting time of the monitor using a timestamp in RFC3339 format.
    #[clap(long = "start-time-rfc3339", parse(try_from_str = humantime::parse_rfc3339),  group = "start-time")]
    rfc: Option<SystemTime>,
}

#[cfg(feature = "pcap_interface")]
#[derive(Clone, Debug, Args)]
#[clap(help_heading = "Input Source")]
#[clap(group(
    ArgGroup::new("ids_input")
    .required(true)
    .args(&["pcap-in", "interface"])
))]
struct IdsInput {
    /// Use the specified pcap file as input source
    #[clap(short, long, parse(from_os_str))]
    pcap_in: Option<PathBuf>,
    /// Use the specified network interface as input source
    #[clap(short, long = "iface")]
    interface: Option<String>,
    /// Specifies a delay in ms to apply between two packets in the input file.
    #[clap(long, requires = "pcap-in")]
    input_delay: Option<u64>,
}

#[derive(Clone, Debug, Args)]
#[clap(help_heading = "Input Source")]
#[clap(group(
    ArgGroup::new("monitor_input")
    .required(true)
    .args(&["csv-in", "stdin"])
))]
struct MonitorInput {
    /// Use the specified CSV file as input source
    #[clap(long, parse(from_os_str))]
    csv_in: Option<PathBuf>,
    /// Use the StdIn as input source
    #[clap(long)]
    stdin: bool,
    /// Specifies a delay in ms to apply between two events in the input file.
    #[clap(long, requires = "csv-in")]
    input_delay: Option<u64>,
    /// The column in the CSV that contains time information.
    #[clap(long, requires = "csv-in")]
    csv_time_column: Option<usize>,
}

#[derive(Clone, Copy, Debug, Args)]
#[clap(help_heading = "Execution Mode")]
#[clap(group(
    ArgGroup::new("mode")
    .required(true)
    .args(&["online", "offline"])
))]
struct CliExecutionMode {
    /// The time of input events is taken by the monitor
    #[clap(long, requires = "stdin")]
    online: bool,

    /// The time of input events is taken from the source in the given format.
    /// For more details, refer to the help of 'output_time_format'
    #[clap(long, arg_enum, value_name = "TIME FORMAT")]
    offline: Option<CliTimeRepresentation>,
}

#[derive(Clone, Debug, Args)]
#[clap(help_heading = "Output Channel")]
struct CliOutputChannel {
    /// Print output to StdOut (default)
    #[clap(long, group = "output")]
    stdout: bool,
    /// Print output to StdErr
    #[clap(long, group = "output")]
    stderr: bool,
    /// Print output to file
    #[clap(long, group = "output", parse(from_os_str))]
    output_file: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, ArgEnum)]
enum CliTimeRepresentation {
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
    /// Short for incremental-float-secs.
    Incremental,
    /// Short for incremental-uint-nanos.
    IncrementalNanos,
    /// Time represented as the unsigned number of nanoseconds relative to the preceding event.
    IncrementalUintNanos,
    /// Short for incremental-float-secs.
    IncrementalSecs,
    /// Time represented as a positive real number representing seconds and sub-seconds relative to the preceding event.
    IncrementalFloatSecs,
    /// Short for absolute-unix.
    Absolute,
    /// Time represented as a positive real number representing seconds and sub-seconds relative to the start of the Unix Epoch.
    AbsoluteUnix,
    /// Short for absolute-rfc3339.
    AbsoluteRfc,
    /// Time represented as a string in RFC3339 format.
    AbsoluteRfc3339,
}

impl MonitorInput {
    fn into_event_source(self, mode: ExecutionMode) -> EventSourceConfig {
        if self.stdin {
            EventSourceConfig::Csv { src: CsvInputSource::stdin(self.csv_time_column, mode) }
        } else {
            EventSourceConfig::Csv {
                src: CsvInputSource::file(
                    self.csv_in.unwrap(),
                    self.input_delay.map(Duration::from_millis),
                    self.csv_time_column,
                    mode,
                ),
            }
        }
    }
}

#[cfg(feature = "pcap_interface")]
impl IdsInput {
    fn into_event_source(self, local_net: String) -> EventSourceConfig {
        if let Some(pcap) = self.pcap_in {
            EventSourceConfig::PCAP {
                src: PCAPInputSource::File {
                    path: pcap,
                    delay: self.input_delay.map(Duration::from_millis),
                    local_network: local_net,
                },
            }
        } else {
            EventSourceConfig::PCAP {
                src: PCAPInputSource::Device { name: self.interface.unwrap(), local_network: local_net },
            }
        }
    }
}

impl From<CliTimeRepresentation> for TimeRepresentation {
    fn from(time_repr: CliTimeRepresentation) -> Self {
        match time_repr {
            CliTimeRepresentation::RelativeNanos | CliTimeRepresentation::RelativeUintNanos => {
                TimeRepresentation::Relative(RelativeTimeFormat::UIntNanos)
            }
            CliTimeRepresentation::Relative
            | CliTimeRepresentation::RelativeSecs
            | CliTimeRepresentation::RelativeFloatSecs => TimeRepresentation::Relative(RelativeTimeFormat::FloatSecs),
            CliTimeRepresentation::IncrementalNanos | CliTimeRepresentation::IncrementalUintNanos => {
                TimeRepresentation::Incremental(RelativeTimeFormat::UIntNanos)
            }
            CliTimeRepresentation::Incremental
            | CliTimeRepresentation::IncrementalSecs
            | CliTimeRepresentation::IncrementalFloatSecs => {
                TimeRepresentation::Incremental(RelativeTimeFormat::FloatSecs)
            }
            CliTimeRepresentation::Absolute | CliTimeRepresentation::AbsoluteUnix => {
                TimeRepresentation::Absolute(AbsoluteTimeFormat::UnixTimeFloat)
            }
            CliTimeRepresentation::AbsoluteRfc | CliTimeRepresentation::AbsoluteRfc3339 => {
                TimeRepresentation::Absolute(AbsoluteTimeFormat::Rfc3339)
            }
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

impl From<CliExecutionMode> for ExecutionMode {
    fn from(mode: CliExecutionMode) -> Self {
        if let Some(time_repr) = mode.offline {
            ExecutionMode::Offline(time_repr.into())
        } else {
            ExecutionMode::Online
        }
    }
}

#[cfg(feature = "pcap_interface")]
impl From<IdsInput> for ExecutionMode {
    fn from(input: IdsInput) -> Self {
        if input.interface.is_some() {
            ExecutionMode::Online
        } else {
            ExecutionMode::Offline(TimeRepresentation::Absolute(AbsoluteTimeFormat::UnixTimeFloat))
        }
    }
}

impl From<CliStartTime> for Option<SystemTime> {
    fn from(st: CliStartTime) -> Self {
        st.rfc.or_else(|| st.unix.map(|d| UNIX_EPOCH + d))
    }
}

impl TryFrom<Cli> for Config {
    type Error = ();

    fn try_from(cli: Cli) -> Result<Self, Self::Error> {
        match cli {
            Cli::Analyze { .. } => Err(()),
            Cli::Completions { .. } => Err(()),
            #[cfg(feature = "pcap_interface")]
            Cli::Ids { spec, local_network, output, input, start_time, verbosity, output_time_format } => {
                let config = rtlola_frontend::ParserConfig::from_path(spec).unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1)
                });
                let handler = rtlola_frontend::Handler::from(config.clone());
                let ir = rtlola_frontend::parse(config).unwrap_or_else(|e| {
                    handler.emit_error(&e);
                    std::process::exit(1);
                });

                let source = input.clone().into_event_source(local_network);

                Ok(Config {
                    ir,
                    source,
                    statistics: verbosity.into(),
                    verbosity,
                    output_channel: output.into(),
                    mode: input.into(),
                    output_time_representation: output_time_format.into(),
                    start_time: start_time.into(),
                })
            }
            Cli::Monitor { spec, input, output, mode, start_time, verbosity, output_time_format } => {
                let config = rtlola_frontend::ParserConfig::from_path(spec).unwrap_or_else(|e| {
                    eprintln!("{}", e);
                    std::process::exit(1)
                });
                let handler = rtlola_frontend::Handler::from(config.clone());
                let ir = rtlola_frontend::parse(config).unwrap_or_else(|e| {
                    handler.emit_error(&e);
                    std::process::exit(1);
                });

                let source = input.into_event_source(mode.into());

                Ok(Config {
                    ir,
                    source,
                    statistics: verbosity.into(),
                    verbosity,
                    output_channel: output.into(),
                    mode: mode.into(),
                    output_time_representation: output_time_format.into(),
                    start_time: start_time.into(),
                })
            }
        }
    }
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
            let handler = rtlola_frontend::Handler::from(config.clone());
            match rtlola_frontend::parse(config) {
                Ok(_) => std::process::exit(0),
                Err(e) => {
                    handler.emit_error(&e);
                    std::process::exit(1)
                }
            }
        }
        Cli::Monitor { .. } => {
            let cfg = Config::try_from(cli).expect("Case handled above");
            cfg.run()?;
        }

        #[cfg(feature = "pcap_interface")]
        Cli::Ids { .. } => {
            let cfg = Config::try_from(cli).expect("Case handled above");
            cfg.run()?;
        }

        Cli::Completions { shell } => shell.generate(),
    }
    Ok(())
}
