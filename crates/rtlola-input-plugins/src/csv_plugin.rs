//! An input plugin that parses data in csv format

use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::stdin;
use std::marker::PhantomData;
use std::path::PathBuf;

use csv::{ByteRecord, Reader as CSVReader, ReaderBuilder, Result as ReaderResult, StringRecord, Trim};
use rtlola_frontend::mir::InputStream;
use rtlola_interpreter::input::{EventFactoryError, InputMap, ValueGetter};
use rtlola_interpreter::rtlola_mir::{RtLolaMir, Type};
use rtlola_interpreter::time::TimeRepresentation;
use rtlola_interpreter::Value;

use crate::EventSource;

const TIME_COLUMN_NAMES: [&str; 3] = ["time", "ts", "timestamp"];

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
    /// Use a string as an input channel
    Buffer(String),
}

#[derive(Debug, Clone)]
/// Used to map input streams to csv columns
pub struct CsvColumnMapping {
    /// Maps input streams to csv columns
    name2col: HashMap<String, usize>,

    /// Maps input streams to their type
    name2type: HashMap<String, Type>,

    /// Column index of time (if existent)
    time_ix: Option<usize>,
}

#[derive(Debug)]
/// Describes different kinds of CsvParsing Errors
pub enum CsvError {
    Io(std::io::Error),
    Validation(String),
    Value(String),
}

impl Display for CsvError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CsvError::Io(e) => write!(f, "Io error occured: {}", e),
            CsvError::Validation(reason) => write!(f, "Csv validation failed: {}", reason),
            CsvError::Value(reason) => write!(f, "Failed to parse value: {}", reason),
        }
    }
}

impl Error for CsvError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            CsvError::Io(e) => Some(e),
            CsvError::Validation(_) => None,
            CsvError::Value(_) => None,
        }
    }
}

impl From<CsvError> for EventFactoryError {
    fn from(value: CsvError) -> Self {
        EventFactoryError::Other(Box::new(value))
    }
}

/// The record as read from the csv data
pub struct CsvRecord(ByteRecord);

impl From<ByteRecord> for CsvRecord {
    fn from(rec: ByteRecord) -> Self {
        CsvRecord(rec)
    }
}

impl InputMap for CsvRecord {
    type CreationData = CsvColumnMapping;
    type Error = CsvError;

    fn func_for_input(name: &str, data: Self::CreationData) -> Result<ValueGetter<Self, Self::Error>, Self::Error> {
        let col_idx = data.name2col[name];
        let ty = data.name2type[name].clone();
        let name = name.to_string();

        Ok(Box::new(move |rec| {
            let bytes = rec.0.get(col_idx).expect("column mapping to be correct");
            Value::try_from_bytes(bytes, &ty).map_err(|e| {
                CsvError::Value(format!(
                    "Could not parse csv item into value. {e} for input stream {name}"
                ))
            })
        }))
    }
}

impl CsvColumnMapping {
    fn from_header(
        inputs: &[InputStream],
        header: &StringRecord,
        time_col: Option<usize>,
    ) -> Result<CsvColumnMapping, Box<dyn Error>> {
        let name2col = inputs
            .iter()
            .map(|i| {
                if let Some(pos) = header.iter().position(|entry| entry == i.name) {
                    Ok((i.name.clone(), pos))
                } else {
                    Err(format!(
                        "error: CSV header does not contain an entry for stream `{}`.",
                        &i.name
                    ))
                }
            })
            .collect::<Result<HashMap<String, usize>, String>>()?;

        let name2type: HashMap<String, Type> = inputs.iter().map(|i| (i.name.clone(), i.ty.clone())).collect();

        let time_ix = time_col.map(|col| col - 1).or_else(|| {
            header
                .iter()
                .position(|name| TIME_COLUMN_NAMES.contains(&name.to_lowercase().as_str()))
        });
        Ok(CsvColumnMapping {
            name2col,
            name2type,
            time_ix,
        })
    }
}

#[derive(Debug)]
enum ReaderWrapper {
    Std(CSVReader<std::io::Stdin>),
    File(CSVReader<File>),
    Buffer(CSVReader<VecDeque<u8>>),
}

impl ReaderWrapper {
    fn read_record(&mut self, rec: &mut ByteRecord) -> ReaderResult<bool> {
        match self {
            ReaderWrapper::Std(r) => r.read_byte_record(rec),
            ReaderWrapper::File(r) => r.read_byte_record(rec),
            ReaderWrapper::Buffer(r) => r.read_byte_record(rec),
        }
    }

    fn header(&mut self) -> ReaderResult<&StringRecord> {
        match self {
            ReaderWrapper::Std(r) => r.headers(),
            ReaderWrapper::File(r) => r.headers(),
            ReaderWrapper::Buffer(r) => r.headers(),
        }
    }
}

type TimeProjection<Time, E> = Box<dyn Fn(&CsvRecord) -> Result<Time, E>>;

///Parses events in CSV format.
pub struct CsvEventSource<InputTime: TimeRepresentation> {
    reader: ReaderWrapper,
    csv_column_mapping: CsvColumnMapping,
    get_time: TimeProjection<InputTime::InnerTime, CsvError>,
    timer: PhantomData<InputTime>,
}

impl<InputTime: TimeRepresentation> CsvEventSource<InputTime> {
    pub fn setup(
        time_col: Option<usize>,
        kind: CsvInputSourceKind,
        ir: &RtLolaMir,
    ) -> Result<CsvEventSource<InputTime>, Box<dyn Error>> {
        let mut reader_builder = ReaderBuilder::new();
        reader_builder.trim(Trim::All);

        let mut wrapper = match kind {
            CsvInputSourceKind::StdIn => ReaderWrapper::Std(reader_builder.from_reader(stdin())),
            CsvInputSourceKind::File(path) => ReaderWrapper::File(reader_builder.from_path(path)?),
            CsvInputSourceKind::Buffer(data) => {
                ReaderWrapper::Buffer(reader_builder.from_reader(VecDeque::from(data.into_bytes())))
            },
        };
        let csv_column_mapping = CsvColumnMapping::from_header(ir.inputs.as_slice(), wrapper.header()?, time_col)?;

        if InputTime::requires_timestamp() && csv_column_mapping.time_ix.is_none() {
            return Err(Box::from("Missing 'time' column in CSV input file."));
        }

        if let Some(time_ix) = csv_column_mapping.time_ix {
            let get_time = Box::new(move |rec: &CsvRecord| {
                let ts = rec.0.get(time_ix).expect("time index to exist.");
                let ts_str = std::str::from_utf8(ts)
                    .map_err(|e| CsvError::Value(format!("Could not parse timestamp: {:?}. Utf8 error: {}", ts, e)))?;
                InputTime::parse(ts_str)
                    .map_err(|e| CsvError::Value(format!("Could not parse timestamp to time format: {}", e)))
            });
            Ok(CsvEventSource {
                reader: wrapper,
                csv_column_mapping,
                get_time,
                timer: PhantomData,
            })
        } else {
            let get_time = Box::new(move |_: &CsvRecord| Ok(InputTime::parse("").unwrap()));
            Ok(CsvEventSource {
                reader: wrapper,
                csv_column_mapping,
                get_time,
                timer: PhantomData,
            })
        }
    }
}

impl<InputTime: TimeRepresentation> EventSource<InputTime> for CsvEventSource<InputTime> {
    type Error = CsvError;
    type Rec = CsvRecord;

    fn init_data(&self) -> Result<<CsvRecord as InputMap>::CreationData, CsvError> {
        Ok(self.csv_column_mapping.clone())
    }

    fn next_event(&mut self) -> Result<Option<(CsvRecord, InputTime::InnerTime)>, CsvError> {
        let mut res = ByteRecord::new();
        self.reader
            .read_record(&mut res)
            .map_err(|e| CsvError::Validation(format!("Error reading csv file: {}", e)))
            .and_then(|success| {
                if success {
                    let record = CsvRecord::from(res);
                    let ts = (*self.get_time)(&record)?;
                    Ok(Some((record, ts)))
                } else {
                    Ok(None)
                }
            })
    }
}
