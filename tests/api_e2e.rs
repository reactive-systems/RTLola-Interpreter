use clap::{ArgEnum, Parser};
use crossterm::style::Stylize;
use itertools::Itertools;
use itertools::Position;
use junit_report::{Duration as JunitDuration, OffsetDateTime, ReportBuilder, TestCase, TestSuiteBuilder};
use ordered_float::NotNan;
use rtlola_frontend::mir::Type;
use rtlola_interpreter::config::RelativeTimeFormat;
use rtlola_interpreter::monitor::{EventInput, Monitor, TriggerMessages};
use rtlola_interpreter::{ConfigBuilder, Value};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::env;
use std::error::Error;
use std::fs::{self, File};
use std::io::BufReader;
use std::iter::FromIterator;
use std::num::ParseFloatError;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::str::FromStr;
use std::time::{Duration, SystemTime};

// inspired by: https://github.com/johnterickson/cargo2junit/blob/master/src/main.rs
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
struct SuiteResults {
    passed: usize,
    failed: usize,
    ignored: usize,
}

// inspired by: https://github.com/johnterickson/cargo2junit/blob/master/src/main.rs
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(tag = "event")]
enum SuiteEvent {
    #[serde(rename = "started")]
    Started { test_count: usize },
    #[serde(rename = "ok")]
    Ok {
        #[serde(flatten)]
        results: SuiteResults,
    },
    #[serde(rename = "failed")]
    Failed {
        #[serde(flatten)]
        results: SuiteResults,
    },
}

// inspired by: https://github.com/johnterickson/cargo2junit/blob/master/src/main.rs
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(tag = "event")]
enum TestEvent {
    #[serde(rename = "started")]
    Started { name: String },
    #[serde(rename = "ok")]
    Ok { name: String },
    #[serde(rename = "failed")]
    Failed { name: String, stdout: Option<String>, stderr: Option<String> },
    #[serde(rename = "ignored")]
    Ignored { name: String },
}

// inspired by: https://github.com/johnterickson/cargo2junit/blob/master/src/main.rs
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "type")]
enum Event {
    #[serde(rename = "suite")]
    Suite {
        #[serde(flatten)]
        event: SuiteEvent,
    },
    #[serde(rename = "test")]
    Test {
        #[serde(flatten)]
        event: TestEvent,
    },
}

#[derive(Parser)]
#[clap(author, version, about)]
struct Cli {
    /// Sets a test directoy path
    #[clap(short, long, parse(from_os_str), value_name = "DIR")]
    test_directory: Option<PathBuf>,

    /// Set the mode the tests are run in
    #[clap(short, long, arg_enum, default_value_t = Mode::Offline)]
    mode: Mode,

    /// Set the output format
    #[clap(short, long, arg_enum, default_value_t = Format::Human)]
    format: Format,

    /// Z flags --- Currently ignored
    #[clap(short = 'Z')]
    z_flags: Option<String>,

    /// Currently ignored
    #[clap(short, long)]
    show_output: bool,
}

#[derive(Clone, Debug, Deserialize, ArgEnum, Eq, PartialEq)]
enum Format {
    Human,
    Json,
    Xml,
}

#[derive(Clone, Debug, Deserialize, ArgEnum, Eq, PartialEq)]
enum Mode {
    #[serde(alias = "online")]
    Online,
    #[serde(alias = "offline")]
    Offline,
    #[serde(alias = "pcap")]
    Pcap,
}

#[derive(Clone, Debug, Deserialize)]
struct JsonTrigger {
    expected_count: usize,
    time_info: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct JsonTest {
    name: Option<String>,
    spec_file: String,
    input_file: String,
    rationale: String,
    modes: Vec<Mode>,
    triggers: HashMap<String, JsonTrigger>,
}

#[derive(Debug)]
enum Outcome {
    Passed,
    Failed(String),
    Panic(Box<dyn Error>),
}

#[derive(Clone, Debug)]
struct Test {
    name: String,
    spec_file: PathBuf,
    input_file: PathBuf,
    rationale: String,
    modes: Vec<Mode>,
    triggers: HashSet<(String, Duration)>,
}

impl Test {
    fn from_json_test(test: JsonTest, repo_base: &Path) -> Result<Self, Box<dyn Error>> {
        let JsonTest { name, spec_file, input_file, rationale, modes, triggers } = test;
        debug_assert!(name.is_some());
        let name = name.unwrap();
        let trigger = triggers
            .into_iter()
            .flat_map(|(t_name, trigger)| {
                assert_eq!(
                    trigger.expected_count,
                    trigger.time_info.len(),
                    "Expected trigger count does not match provided timestamps for trigger {} in test {}",
                    t_name,
                    name
                );
                trigger
                    .time_info
                    .into_iter()
                    .map(move |ts| f64::from_str(&ts).map(|f| (t_name.clone(), Duration::from_secs_f64(f))))
            })
            .collect::<Result<HashSet<(String, Duration)>, ParseFloatError>>()?;

        let spec_file = PathBuf::from(spec_file);
        let spec_file_relative: PathBuf = spec_file.iter().skip(1).collect();
        let mut spec_file = PathBuf::from(repo_base);
        spec_file.push(spec_file_relative);
        assert!(spec_file.is_file(), "Spec file path of Test '{}' is invalid", name);

        let input_file = PathBuf::from(input_file);
        let input_file_relative: PathBuf = input_file.iter().skip(1).collect();
        let mut input_file = PathBuf::from(repo_base);
        input_file.push(input_file_relative);
        assert!(input_file.is_file(), "Input file path of Test '{}' is invalid", name);

        Ok(Test { name, spec_file, input_file, rationale, modes, triggers: trigger })
    }

    fn run(&self) -> Outcome {
        match self.run_inner() {
            Ok(o) => o,
            Err(e) => Outcome::Panic(e),
        }
    }

    fn run_inner(&self) -> Result<Outcome, Box<dyn Error>> {
        // Open CSV File
        let file = File::open(self.input_file.as_path())?;
        let reader = BufReader::new(file);
        let mut csv = csv::Reader::from_reader(reader);
        //find time column
        let time_idx =
            csv.headers().unwrap().iter().position(|header| header == "time").expect("No time column in csv file");

        // Init Monitor API
        let config = ConfigBuilder::api()
            .relative_input_time(RelativeTimeFormat::FloatSecs)
            .spec_file(self.spec_file.clone())
            .build();

        //Get Input names, Types and column
        let inputs: Vec<_> = config
            .ir
            .inputs
            .iter()
            .map(|i| {
                let name = i.name.clone();
                let idx = csv.headers().unwrap().iter().position(|h| h == &name).expect("missing input in csv");
                (i.ty.clone(), idx)
            })
            .collect();

        // Todo: Consider using the RecordParser input
        let mut monitor: Monitor<EventInput<Vec<Value>>, TriggerMessages> = config.monitor();

        let mut actual = Vec::new();
        for line in csv.records().with_position() {
            let is_last = matches!(line, Position::Last(_));
            let line = line.into_inner()?;
            let time = Duration::from_secs_f64(f64::from_str(&line[time_idx])?);
            let event =
                inputs.iter().map(|(ty, idx)| value_from_string(&line[*idx], ty)).collect::<Result<Vec<Value>, _>>()?;

            let verdict = monitor.accept_event(event, time);
            let triggers = verdict.event.into_iter().map(move |(_, name)| (name, time)).chain(
                verdict.timed.into_iter().flat_map(|(time, trig)| trig.into_iter().map(move |(_, name)| (name, time))),
            );
            actual.extend(triggers);

            if is_last {
                let triggers = monitor
                    .accept_time(time)
                    .into_iter()
                    .flat_map(|(time, trig)| trig.into_iter().map(move |(_, name)| (name, time)));
                actual.extend(triggers);
            }
        }

        // Check results
        let mut fail: bool = false;
        let mut messages: Vec<String> = Vec::new();

        let actual = HashSet::from_iter(actual.into_iter());
        let expected = &self.triggers;
        for (trigger, when) in actual.difference(&expected) {
            fail = true;
            messages.push(format!("Unexpected trigger occurred: '{}' at {:?}", trigger, when));
        }
        for (trigger, when) in expected.difference(&actual) {
            fail = true;
            messages.push(format!("Missing trigger '{}' at {:?}", trigger, when));
        }

        if fail {
            if !self.rationale.is_empty() {
                messages.push(self.rationale.clone());
            }
            let message: String = messages.join("\n");
            Ok(Outcome::Failed(message))
        } else {
            Ok(Outcome::Passed)
        }
    }
}

fn parse_tests(directory: &Path, repo_base: &Path) -> Result<Vec<Test>, Box<dyn Error>> {
    let files = fs::read_dir(directory)?.map(|test| test.map(|t| t.path())).collect::<Result<Vec<_>, _>>()?;
    let tests = files
        .into_iter()
        .filter(|test| {
            test.extension().and_then(|s| s.to_str()).map(|ext| ext == "rtlola_interpreter_test").unwrap_or(false)
        })
        .map::<Result<JsonTest, std::io::Error>, _>(|file_path| {
            let file = File::open(file_path.as_path())?;
            let reader = BufReader::new(file);
            let mut test: JsonTest = serde_json::from_reader(reader).map_err(|e| std::io::Error::from(e))?;
            test.name = file_path.file_stem().and_then(|s| s.to_str().map(|s| s.to_string()));
            Ok(test)
        })
        .collect::<Result<Vec<JsonTest>, _>>()?;
    tests.into_iter().map(|t| Test::from_json_test(t, repo_base)).collect::<Result<Vec<Test>, _>>()
}

fn find_base_dir() -> PathBuf {
    let cwd = env::current_dir().unwrap();
    let parent = PathBuf::from(cwd.parent().unwrap());

    let mut current_ci = cwd.clone();
    current_ci.push(".gitlab-ci.yml");

    let mut parent_ci = parent.clone();
    parent_ci.push(".gitlab-ci.yml");

    if current_ci.is_file() {
        cwd
    } else if parent_ci.is_file() {
        parent
    } else {
        panic!("Test not run from repo base dir or tests directory");
    }
}

fn value_from_string(str: &str, ty: &Type) -> Result<Value, Box<dyn Error>> {
    if str == "#" {
        return Ok(Value::None);
    }
    Ok(match ty {
        Type::Bool => Value::Bool(bool::from_str(str)?),
        Type::Int(_) => Value::Signed(i64::from_str(str)?),
        Type::UInt(_) => Value::Unsigned(u64::from_str(str)?),
        Type::Float(_) => Value::Float(NotNan::new(f64::from_str(str)?)?),
        Type::String => Value::Str(str.into()),
        Type::Bytes | Type::Tuple(_) | Type::Option(_) | Type::Function { .. } => unimplemented!(),
    })
}

trait OutputFormat {
    fn pre(&mut self, tests: &[Test]);
    fn test_start(&mut self, test: &Test);
    fn ignored(&mut self, test: &Test);
    fn passed(&mut self, test: &Test);
    fn failed(&mut self, test: &Test, msg: String);
    fn panic(&mut self, test: &Test, error: Box<dyn Error>);
    fn post(&mut self);
}

// ################# Handle Human Output Format #################

#[derive(Default, Debug)]
struct HumanOutput {
    passed: usize,
    failed: usize,
    ignored: usize,
    total: usize,
    failed_names: Vec<String>,
}

impl OutputFormat for HumanOutput {
    fn pre(&mut self, tests: &[Test]) {
        self.total = tests.len();
        println!("==================== {} =====================", "API e2e tests".bold());
        println!("Running {} tests", tests.len());
    }

    fn test_start(&mut self, test: &Test) {
        println!("\n==================== {} =====================\n", test.name.as_str().bold());
    }

    fn ignored(&mut self, _test: &Test) {
        self.ignored += 1;
        println!("{}", "IGNORED".yellow());
    }

    fn passed(&mut self, _test: &Test) {
        self.passed += 1;
        println!("{}", "PASSED".green());
    }

    fn failed(&mut self, test: &Test, msg: String) {
        self.failed += 1;
        self.failed_names.push(test.name.clone());
        println!("{}", msg);
        println!("\n{}", "FAILED".red());
    }

    fn panic(&mut self, _test: &Test, error: Box<dyn Error>) {
        self.failed += 1;
        println!("{:?}", error);
        println!("\n{}", "PANIC".red());
    }

    fn post(&mut self) {
        println!("\n==================== {} =====================\n", "Summary".bold());
        println!("{}:\n{}\n", "Failed tests".bold(), self.failed_names.join("\n"));
        println!(
            "{}: {} | {}: {} | {}: {} | Total: {}",
            "Failed".red(),
            self.failed,
            "Ignored".yellow(),
            self.ignored,
            "Passed".green(),
            self.passed,
            self.total
        );
    }
}

// ################# Handle Json Output Format #################

#[derive(Default, Debug)]
struct JsonOutput {
    passed: usize,
    failed: usize,
    ignored: usize,
}

impl OutputFormat for JsonOutput {
    fn pre(&mut self, tests: &[Test]) {
        println!(
            "{}",
            serde_json::to_string(&Event::Suite { event: SuiteEvent::Started { test_count: tests.len() } }).unwrap()
        );
    }

    fn test_start(&mut self, test: &Test) {
        println!(
            "{}",
            serde_json::to_string(&Event::Test { event: TestEvent::Started { name: test.name.clone() } }).unwrap()
        )
    }

    fn ignored(&mut self, test: &Test) {
        self.ignored += 1;
        println!(
            "{}",
            serde_json::to_string(&Event::Test { event: TestEvent::Ignored { name: test.name.clone() } }).unwrap()
        )
    }

    fn passed(&mut self, test: &Test) {
        self.passed += 1;
        println!(
            "{}",
            serde_json::to_string(&Event::Test { event: TestEvent::Ok { name: test.name.clone() } }).unwrap()
        )
    }

    fn failed(&mut self, test: &Test, msg: String) {
        self.failed += 1;
        println!(
            "{}",
            serde_json::to_string(&Event::Test {
                event: TestEvent::Failed { name: test.name.clone(), stdout: Some(msg), stderr: None },
            })
            .unwrap()
        )
    }

    fn panic(&mut self, test: &Test, error: Box<dyn Error>) {
        self.failed += 1;
        println!(
            "{}",
            serde_json::to_string(&Event::Test {
                event: TestEvent::Failed {
                    name: test.name.clone(),
                    stdout: None,
                    stderr: Some(format!("{:?}", error))
                },
            })
            .unwrap()
        )
    }

    fn post(&mut self) {
        let ev = if self.failed > 0 {
            Event::Suite {
                event: SuiteEvent::Failed {
                    results: SuiteResults { passed: self.passed, failed: self.failed, ignored: self.ignored },
                },
            }
        } else {
            Event::Suite {
                event: SuiteEvent::Ok {
                    results: SuiteResults { passed: self.passed, failed: self.failed, ignored: self.ignored },
                },
            }
        };
        println!("{}", serde_json::to_string(&ev).unwrap());
    }
}

// ################# Handle Xml Output Format #################

#[derive(Debug)]
struct XmlOutput {
    suite: TestSuiteBuilder,
    start_time: SystemTime,
}

impl Default for XmlOutput {
    fn default() -> Self {
        XmlOutput { suite: TestSuiteBuilder::new("Api e2e Tests"), start_time: SystemTime::now() }
    }
}

impl OutputFormat for XmlOutput {
    fn pre(&mut self, _tests: &[Test]) {
        self.suite.set_timestamp(OffsetDateTime::now_utc());
    }

    fn test_start(&mut self, _test: &Test) {
        self.start_time = SystemTime::now();
    }

    fn ignored(&mut self, test: &Test) {
        self.suite.add_testcase(TestCase::skipped(test.name.as_str()));
    }

    fn passed(&mut self, test: &Test) {
        let dur = JunitDuration::try_from(self.start_time.elapsed().unwrap()).unwrap();
        self.suite.add_testcase(TestCase::success(test.name.as_str(), dur));
    }

    fn failed(&mut self, test: &Test, msg: String) {
        let dur = JunitDuration::try_from(self.start_time.elapsed().unwrap()).unwrap();
        self.suite.add_testcase(TestCase::failure(test.name.as_str(), dur, "Trigger mismatch", msg.as_str()));
    }

    fn panic(&mut self, test: &Test, error: Box<dyn Error>) {
        let dur = JunitDuration::try_from(self.start_time.elapsed().unwrap()).unwrap();
        self.suite.add_testcase(TestCase::error(test.name.as_str(), dur, "Test Panic", &format!("{:?}", error)));
    }

    fn post(&mut self) {
        let suite = self.suite.build();
        let report = ReportBuilder::new().add_testsuite(suite).build();
        let file = File::create("api-e2e-results.xml").unwrap();
        report.write_xml(file).unwrap();
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    // Parse CLI args
    let args: Cli = Cli::parse();

    let mut output: Box<dyn OutputFormat> = match args.format {
        Format::Human => Box::new(HumanOutput::default()),
        Format::Json => Box::new(JsonOutput::default()),
        Format::Xml => Box::new(XmlOutput::default()),
    };

    let test_path = args.test_directory.or_else(|| PathBuf::from_str("./tests/definitions").ok()).unwrap();
    assert!(test_path.is_dir());
    let base_dir = find_base_dir();
    let tests: Vec<Test> = parse_tests(&test_path, base_dir.as_path())?;

    let mut failed = false;

    output.pre(&tests);

    for test in tests {
        output.test_start(&test);
        if test.modes.contains(&Mode::Pcap) {
            output.ignored(&test);
            continue;
        }
        match test.run() {
            Outcome::Passed => {
                output.passed(&test);
            }
            Outcome::Failed(msg) => {
                output.failed(&test, msg);
                failed = true;
            }
            Outcome::Panic(e) => {
                output.panic(&test, e);
                failed = true;
            }
        }
    }

    output.post();

    if failed {
        exit(101);
    }
    Ok(())
}
