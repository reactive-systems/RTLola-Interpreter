#![allow(clippy::mutex_atomic)]

use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::stdin;
use std::marker::PhantomData;
use std::path::PathBuf;

use csv::{ByteRecord, Reader as CSVReader, Result as ReaderResult, StringRecord};
use rtlola_interpreter::monitor::Record;
use rtlola_interpreter::rtlola_mir::{RtLolaMir, Type};
use rtlola_interpreter::time::TimeRepresentation;
use rtlola_interpreter::Value;

use crate::io::EventSource;

/// Configures the input source for the [CsvEventSource].
#[derive(Debug, Clone)]
pub struct CsvInputSource {
    /// The index of column in which the time information is given
    /// If none the column named 'time' is chosen.
    pub time_col: Option<usize>,
    /// Specifies the input channel of the source.
    pub kind: CsvInputSourceKind,
}

/// Sets the input channel of the [CsvEventSource]
#[derive(Debug, Clone)]
pub enum CsvInputSourceKind {
    /// Use the std-in as an input channel
    StdIn,
    /// Use the specified file as an input channel
    File(PathBuf),
}

#[derive(Debug, Clone)]
pub(crate) struct CsvColumnMapping {
    /// Maps input streams to csv columns
    name2col: HashMap<String, usize>,

    /// Maps input streams to their type
    name2type: HashMap<String, Type>,

    /// Column index of time (if existent)
    time_ix: Option<usize>,
}

pub(crate) struct CsvRecord(ByteRecord);

impl From<ByteRecord> for CsvRecord {
    fn from(rec: ByteRecord) -> Self {
        CsvRecord(rec)
    }
}

impl Record for CsvRecord {
    type CreationData = CsvColumnMapping;

    fn func_for_input(name: &str, data: Self::CreationData) -> Box<dyn Fn(&Self) -> Value> {
        let col_idx = data.name2col[name];
        let ty = data.name2type[name].clone();
        let name = name.to_string();

        Box::new(move |rec| {
            let bytes = rec.0.get(col_idx).expect("column mapping to be correct");
            match Value::try_from(bytes, &ty) {
                Some(v) => v,
                None => {
                    panic!(
                        "Could not parse csv item into value. Tried to parse: {:?} for input stream {}",
                        bytes, name
                    )
                },
            }
        })
    }
}

impl CsvColumnMapping {
    fn from_header(names: &[&str], types: &[Type], header: &StringRecord, time_col: Option<usize>) -> CsvColumnMapping {
        let name2col: HashMap<String, usize> = names
            .iter()
            .map(|name| {
                let pos = header.iter().position(|entry| &entry == name).unwrap_or_else(|| {
                    eprintln!("error: CSV header does not contain an entry for stream `{}`.", name);
                    std::process::exit(1)
                });
                (name.to_string(), pos)
            })
            .collect();

        let name2type: HashMap<String, Type> = names.iter().map(|s| s.to_string()).zip(types.iter().cloned()).collect();

        let time_ix = time_col.map(|col| col - 1).or_else(|| {
            header.iter().position(|name| {
                let name = name.to_lowercase();
                name == "time" || name == "ts" || name == "timestamp"
            })
        });
        CsvColumnMapping {
            name2col,
            name2type,
            time_ix,
        }
    }
}

#[derive(Debug)]
enum ReaderWrapper {
    Std(CSVReader<std::io::Stdin>),
    File(CSVReader<File>),
}

impl ReaderWrapper {
    fn read_record(&mut self, rec: &mut ByteRecord) -> ReaderResult<bool> {
        match self {
            ReaderWrapper::Std(r) => r.read_byte_record(rec),
            ReaderWrapper::File(r) => r.read_byte_record(rec),
        }
    }

    fn header(&mut self) -> ReaderResult<&StringRecord> {
        match self {
            ReaderWrapper::Std(r) => r.headers(),
            ReaderWrapper::File(r) => r.headers(),
        }
    }
}

///Parses events in CSV format.
pub struct CsvEventSource<InputTime: TimeRepresentation> {
    reader: ReaderWrapper,
    csv_column_mapping: CsvColumnMapping,
    get_time: Box<dyn Fn(&CsvRecord) -> InputTime::InnerTime>,
    timer: PhantomData<InputTime>,
}

impl<InputTime: TimeRepresentation> CsvEventSource<InputTime> {
    pub(crate) fn setup(
        time_col: Option<usize>,
        kind: CsvInputSourceKind,
        ir: &RtLolaMir,
    ) -> Result<CsvEventSource<InputTime>, Box<dyn Error>> {
        let mut wrapper = match kind {
            CsvInputSourceKind::StdIn => ReaderWrapper::Std(CSVReader::from_reader(stdin())),
            CsvInputSourceKind::File(path) => ReaderWrapper::File(CSVReader::from_path(path)?),
        };

        let stream_names: Vec<&str> = ir.inputs.iter().map(|i| i.name.as_str()).collect();
        let in_types: Vec<Type> = ir.inputs.iter().map(|i| i.ty.clone()).collect();
        let csv_column_mapping = CsvColumnMapping::from_header(
            stream_names.as_slice(),
            in_types.as_slice(),
            wrapper.header()?,
            time_col,
        );

        if InputTime::requires_timestamp() && csv_column_mapping.time_ix.is_none() {
            return Err(Box::from("Missing 'time' column in CSV input file."));
        }

        if let Some(time_ix) = csv_column_mapping.time_ix {
            let get_time = Box::new(move |rec: &CsvRecord| {
                let ts = rec.0.get(time_ix).expect("time index to exist.");
                let ts_str = match std::str::from_utf8(ts) {
                    Ok(s) => s,
                    Err(e) => panic!("Could not parse timestamp: {:?}. Utf8 error: {}", ts, e),
                };
                match InputTime::parse(ts_str) {
                    Ok(t) => t,
                    Err(e) => panic!("Could not parse timestamp {} into input format: {}", ts_str, e),
                }
            });
            Ok(CsvEventSource {
                reader: wrapper,
                csv_column_mapping,
                get_time,
                timer: PhantomData::default(),
            })
        } else {
            let get_time = Box::new(move |_: &CsvRecord| InputTime::parse("").expect("timestamp to not matter"));
            Ok(CsvEventSource {
                reader: wrapper,
                csv_column_mapping,
                get_time,
                timer: PhantomData::default(),
            })
        }
    }
}

impl<InputTime: TimeRepresentation> EventSource<CsvRecord, InputTime> for CsvEventSource<InputTime> {
    fn init_data(&self) -> <CsvRecord as Record>::CreationData {
        self.csv_column_mapping.clone()
    }

    fn next_event(&mut self) -> Option<(CsvRecord, InputTime::InnerTime)> {
        let mut res = ByteRecord::new();
        match self.reader.read_record(&mut res) {
            Ok(true) => {
                let record = CsvRecord::from(res);
                let ts = (*self.get_time)(&record);
                Some((record, ts))
            },
            Ok(false) => None,
            Err(e) => panic!("Error reading csv file: {}", e),
        }
    }
}
