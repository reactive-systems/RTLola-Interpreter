#![allow(clippy::mutex_atomic)]

use crate::basics::EventSource;
use crate::configuration::time::{init_start_time, TimeRepresentation};
use crate::storage::Value;
use crate::Time;
use csv::{ByteRecord, Reader as CSVReader, Result as ReaderResult, StringRecord};
use rtlola_frontend::mir::{RtLolaMir, Type};
use std::error::Error;
use std::fs::File;
use std::io::stdin;
use std::path::PathBuf;
use std::time::SystemTime;

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
    /// Mapping from column index to input stream index/reference
    pub(crate) col2str: Vec<Option<usize>>,

    /// Column index of time (if existent)
    time_ix: Option<usize>,
}

impl CsvColumnMapping {
    fn from_header(names: &[&str], header: &StringRecord, time_col: Option<usize>) -> CsvColumnMapping {
        let str2col: Vec<usize> = names
            .iter()
            .map(|name| {
                header.iter().position(|entry| &entry == name).unwrap_or_else(|| {
                    eprintln!("error: CSV header does not contain an entry for stream `{}`.", name);
                    std::process::exit(1)
                })
            })
            .collect();

        let mut col2str: Vec<Option<usize>> = vec![None; header.len()];
        for (str_ix, header_ix) in str2col.iter().enumerate() {
            col2str[*header_ix] = Some(str_ix);
        }

        let time_ix = time_col.map(|col| col - 1).or_else(|| {
            header.iter().position(|name| {
                let name = name.to_lowercase();
                name == "time" || name == "ts" || name == "timestamp"
            })
        });
        CsvColumnMapping { col2str, time_ix }
    }

    fn input_to_stream(&self, input_ix: usize) -> Option<usize> {
        self.col2str[input_ix]
    }

    fn num_inputs(&self) -> usize {
        self.col2str.len()
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

    fn get_header(&mut self) -> ReaderResult<&StringRecord> {
        match self {
            ReaderWrapper::Std(r) => r.headers(),
            ReaderWrapper::File(r) => r.headers(),
        }
    }
}

///Parses events in CSV format.
#[derive(Debug)]
pub struct CsvEventSource<InputTime: TimeRepresentation> {
    reader: ReaderWrapper,
    record: ByteRecord,
    mapping: CsvColumnMapping,
    in_types: Vec<Type>,
    timer: InputTime,
}

impl<InputTime: TimeRepresentation> CsvEventSource<InputTime> {
    pub(crate) fn setup(
        src: &CsvInputSource,
        timer: InputTime,
        ir: &RtLolaMir,
        start_time: Option<SystemTime>,
    ) -> Result<Box<dyn EventSource<InputTime>>, Box<dyn Error>> {
        let CsvInputSource { time_col, kind } = src;
        let mut wrapper = match kind {
            CsvInputSourceKind::StdIn => ReaderWrapper::Std(CSVReader::from_reader(stdin())),
            CsvInputSourceKind::File(path) => ReaderWrapper::File(CSVReader::from_path(path)?),
        };

        let stream_names: Vec<&str> = ir.inputs.iter().map(|i| i.name.as_str()).collect();
        let mapping = CsvColumnMapping::from_header(stream_names.as_slice(), wrapper.get_header()?, *time_col);
        let in_types: Vec<Type> = ir.inputs.iter().map(|i| i.ty.clone()).collect();

        init_start_time::<InputTime>(start_time);

        Ok(Box::new(CsvEventSource { reader: wrapper, record: ByteRecord::new(), mapping, in_types, timer }))
    }

    fn read_blocking(&mut self) -> Result<bool, Box<dyn Error>> {
        if cfg!(debug_assertion) {
            // Reset record.
            self.record.clear();
        }
        let read_res = match self.reader.read_record(&mut self.record) {
            Ok(v) => v,
            Err(e) => {
                return Err(e.into());
            }
        };
        if !read_res {
            return Ok(false);
        }
        assert_eq!(self.record.len(), self.mapping.num_inputs());

        //TODO(marvin): this assertion seems wrong, empty strings could be valid values
        if cfg!(debug_assertion) {
            assert!(self
                .record
                .iter()
                .enumerate()
                .filter(|(ix, _)| self.mapping.input_to_stream(*ix).is_some())
                .all(|(_, str)| !str.is_empty()));
        }

        Ok(true)
    }

    pub(crate) fn str_for_time(&self) -> Option<&str> {
        self.time_index().map(|ix| &self.record[ix]).and_then(|bytes| std::str::from_utf8(bytes).ok())
    }

    fn time_index(&self) -> Option<usize> {
        self.mapping.time_ix
    }

    fn get_time(&mut self) -> Time {
        let str = self.str_for_time().unwrap().to_string();
        self.timer.parse(&str).unwrap()
    }

    fn read_event(&self) -> Vec<Value> {
        let mut buffer = vec![Value::None; self.in_types.len()];
        for (col_ix, s) in self.record.iter().enumerate() {
            if let Some(str_ix) = self.mapping.col2str[col_ix] {
                // utf8-encoding (as [u8]) of string "#"
                if s != [35] {
                    let t = &self.in_types[str_ix];
                    buffer[str_ix] = Value::try_from(s, t).unwrap_or_else(|| {
                        if let Ok(s) = std::str::from_utf8(s) {
                            eprintln!(
                                "error: problem with data source; failed to parse {} as value of type {:?}.",
                                s, t
                            );
                        } else {
                            eprintln!(
                                "error: problem with data source; failed to parse non-utf8 {:?} as value of type {:?}.",
                                s, t
                            );
                        }
                        std::process::exit(1)
                    })
                }
            }
        }
        buffer
    }
}

impl<InputTime: TimeRepresentation> EventSource<InputTime> for CsvEventSource<InputTime> {
    fn has_event(&mut self) -> bool {
        self.read_blocking().unwrap_or_else(|e| {
            eprintln!("error: failed to read data. {}", e);
            std::process::exit(1)
        })
    }

    fn get_event(&mut self) -> (Vec<Value>, Time) {
        let event = self.read_event();
        let time = self.get_time();
        (event, time)
    }
}
