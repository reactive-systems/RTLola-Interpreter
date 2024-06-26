#![feature(prelude_import)]
//! This module exposes functionality to handle the input and output methods of the CLI.
#[prelude_import]
use std::prelude::rust_2021::*;
#[macro_use]
extern crate std;
#[cfg(feature = "csv_plugin")]
pub mod csv_plugin {
    //! An input plugin that parses data in csv format
    use std::collections::{HashMap, VecDeque};
    use std::error::Error;
    use std::fmt::{Display, Formatter};
    use std::fs::File;
    use std::io::stdin;
    use std::marker::PhantomData;
    use std::path::PathBuf;
    use csv::{
        ByteRecord, Reader as CSVReader, ReaderBuilder, Result as ReaderResult,
        StringRecord, Trim,
    };
    use rtlola_interpreter::input::{EventFactoryError, InputMap, ValueGetter};
    use rtlola_interpreter::rtlola_frontend::mir::InputStream;
    use rtlola_interpreter::rtlola_mir::{RtLolaMir, Type};
    use rtlola_interpreter::time::TimeRepresentation;
    use rtlola_interpreter::Value;
    use crate::EventSource;
    const TIME_COLUMN_NAMES: [&str; 3] = ["time", "ts", "timestamp"];
    /// Configures the input source for the [CsvEventSource].
    pub struct CsvInputSource {
        /// The index of column in which the time information is given
        /// If none the column named 'time' is chosen.
        pub time_col: Option<usize>,
        /// Specifies the input channel of the source.
        pub kind: CsvInputSourceKind,
    }
    #[automatically_derived]
    impl ::core::fmt::Debug for CsvInputSource {
        #[inline]
        fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
            ::core::fmt::Formatter::debug_struct_field2_finish(
                f,
                "CsvInputSource",
                "time_col",
                &self.time_col,
                "kind",
                &&self.kind,
            )
        }
    }
    #[automatically_derived]
    impl ::core::clone::Clone for CsvInputSource {
        #[inline]
        fn clone(&self) -> CsvInputSource {
            CsvInputSource {
                time_col: ::core::clone::Clone::clone(&self.time_col),
                kind: ::core::clone::Clone::clone(&self.kind),
            }
        }
    }
    /// Sets the input channel of the [CsvEventSource]
    pub enum CsvInputSourceKind {
        /// Use the std-in as an input channel
        StdIn,
        /// Use the specified file as an input channel
        File(PathBuf),
        /// Use a string as an input channel
        Buffer(String),
    }
    #[automatically_derived]
    impl ::core::fmt::Debug for CsvInputSourceKind {
        #[inline]
        fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
            match self {
                CsvInputSourceKind::StdIn => {
                    ::core::fmt::Formatter::write_str(f, "StdIn")
                }
                CsvInputSourceKind::File(__self_0) => {
                    ::core::fmt::Formatter::debug_tuple_field1_finish(
                        f,
                        "File",
                        &__self_0,
                    )
                }
                CsvInputSourceKind::Buffer(__self_0) => {
                    ::core::fmt::Formatter::debug_tuple_field1_finish(
                        f,
                        "Buffer",
                        &__self_0,
                    )
                }
            }
        }
    }
    #[automatically_derived]
    impl ::core::clone::Clone for CsvInputSourceKind {
        #[inline]
        fn clone(&self) -> CsvInputSourceKind {
            match self {
                CsvInputSourceKind::StdIn => CsvInputSourceKind::StdIn,
                CsvInputSourceKind::File(__self_0) => {
                    CsvInputSourceKind::File(::core::clone::Clone::clone(__self_0))
                }
                CsvInputSourceKind::Buffer(__self_0) => {
                    CsvInputSourceKind::Buffer(::core::clone::Clone::clone(__self_0))
                }
            }
        }
    }
    /// Used to map input streams to csv columns
    pub struct CsvColumnMapping {
        /// Maps input streams to csv columns
        name2col: HashMap<String, usize>,
        /// Maps input streams to their type
        name2type: HashMap<String, Type>,
        /// Column index of time (if existent)
        time_ix: Option<usize>,
    }
    #[automatically_derived]
    impl ::core::fmt::Debug for CsvColumnMapping {
        #[inline]
        fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
            ::core::fmt::Formatter::debug_struct_field3_finish(
                f,
                "CsvColumnMapping",
                "name2col",
                &self.name2col,
                "name2type",
                &self.name2type,
                "time_ix",
                &&self.time_ix,
            )
        }
    }
    #[automatically_derived]
    impl ::core::clone::Clone for CsvColumnMapping {
        #[inline]
        fn clone(&self) -> CsvColumnMapping {
            CsvColumnMapping {
                name2col: ::core::clone::Clone::clone(&self.name2col),
                name2type: ::core::clone::Clone::clone(&self.name2type),
                time_ix: ::core::clone::Clone::clone(&self.time_ix),
            }
        }
    }
    /// Describes different kinds of CsvParsing Errors
    pub enum CsvError {
        Io(std::io::Error),
        Validation(String),
        Value(String),
    }
    #[automatically_derived]
    impl ::core::fmt::Debug for CsvError {
        #[inline]
        fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
            match self {
                CsvError::Io(__self_0) => {
                    ::core::fmt::Formatter::debug_tuple_field1_finish(f, "Io", &__self_0)
                }
                CsvError::Validation(__self_0) => {
                    ::core::fmt::Formatter::debug_tuple_field1_finish(
                        f,
                        "Validation",
                        &__self_0,
                    )
                }
                CsvError::Value(__self_0) => {
                    ::core::fmt::Formatter::debug_tuple_field1_finish(
                        f,
                        "Value",
                        &__self_0,
                    )
                }
            }
        }
    }
    impl Display for CsvError {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            match self {
                CsvError::Io(e) => f.write_fmt(format_args!("Io error occured: {0}", e)),
                CsvError::Validation(reason) => {
                    f.write_fmt(format_args!("Csv validation failed: {0}", reason))
                }
                CsvError::Value(reason) => {
                    f.write_fmt(format_args!("Failed to parse value: {0}", reason))
                }
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
        fn func_for_input(
            name: &str,
            data: Self::CreationData,
        ) -> Result<ValueGetter<Self, Self::Error>, Self::Error> {
            let col_idx = data.name2col[name];
            let ty = data.name2type[name].clone();
            let name = name.to_string();
            Ok(
                Box::new(move |rec| {
                    let bytes = rec
                        .0
                        .get(col_idx)
                        .expect("column mapping to be correct");
                    Value::try_from_bytes(bytes, &ty)
                        .map_err(|e| {
                            CsvError::Value({
                                let res = ::alloc::fmt::format(
                                    format_args!(
                                        "Could not parse csv item into value. {0} for input stream {1}",
                                        e,
                                        name,
                                    ),
                                );
                                res
                            })
                        })
                }),
            )
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
                        Err({
                            let res = ::alloc::fmt::format(
                                format_args!(
                                    "error: CSV header does not contain an entry for stream `{0}`.",
                                    &i.name,
                                ),
                            );
                            res
                        })
                    }
                })
                .collect::<Result<HashMap<String, usize>, String>>()?;
            let name2type: HashMap<String, Type> = inputs
                .iter()
                .map(|i| (i.name.clone(), i.ty.clone()))
                .collect();
            let time_ix = time_col
                .map(|col| col - 1)
                .or_else(|| {
                    header
                        .iter()
                        .position(|name| {
                            TIME_COLUMN_NAMES.contains(&name.to_lowercase().as_str())
                        })
                });
            Ok(CsvColumnMapping {
                name2col,
                name2type,
                time_ix,
            })
        }
    }
    enum ReaderWrapper {
        Std(CSVReader<std::io::Stdin>),
        File(CSVReader<File>),
        Buffer(CSVReader<VecDeque<u8>>),
    }
    #[automatically_derived]
    impl ::core::fmt::Debug for ReaderWrapper {
        #[inline]
        fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
            match self {
                ReaderWrapper::Std(__self_0) => {
                    ::core::fmt::Formatter::debug_tuple_field1_finish(
                        f,
                        "Std",
                        &__self_0,
                    )
                }
                ReaderWrapper::File(__self_0) => {
                    ::core::fmt::Formatter::debug_tuple_field1_finish(
                        f,
                        "File",
                        &__self_0,
                    )
                }
                ReaderWrapper::Buffer(__self_0) => {
                    ::core::fmt::Formatter::debug_tuple_field1_finish(
                        f,
                        "Buffer",
                        &__self_0,
                    )
                }
            }
        }
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
                CsvInputSourceKind::StdIn => {
                    ReaderWrapper::Std(reader_builder.from_reader(stdin()))
                }
                CsvInputSourceKind::File(path) => {
                    ReaderWrapper::File(reader_builder.from_path(path)?)
                }
                CsvInputSourceKind::Buffer(data) => {
                    ReaderWrapper::Buffer(
                        reader_builder.from_reader(VecDeque::from(data.into_bytes())),
                    )
                }
            };
            let csv_column_mapping = CsvColumnMapping::from_header(
                ir.inputs.as_slice(),
                wrapper.header()?,
                time_col,
            )?;
            if InputTime::requires_timestamp() && csv_column_mapping.time_ix.is_none() {
                return Err(Box::from("Missing 'time' column in CSV input file."));
            }
            if let Some(time_ix) = csv_column_mapping.time_ix {
                let get_time = Box::new(move |rec: &CsvRecord| {
                    let ts = rec.0.get(time_ix).expect("time index to exist.");
                    let ts_str = std::str::from_utf8(ts)
                        .map_err(|e| CsvError::Value({
                            let res = ::alloc::fmt::format(
                                format_args!(
                                    "Could not parse timestamp: {0:?}. Utf8 error: {1}",
                                    ts,
                                    e,
                                ),
                            );
                            res
                        }))?;
                    InputTime::parse(ts_str)
                        .map_err(|e| CsvError::Value({
                            let res = ::alloc::fmt::format(
                                format_args!(
                                    "Could not parse timestamp to time format: {0}",
                                    e,
                                ),
                            );
                            res
                        }))
                });
                Ok(CsvEventSource {
                    reader: wrapper,
                    csv_column_mapping,
                    get_time,
                    timer: PhantomData,
                })
            } else {
                let get_time = Box::new(move |_: &CsvRecord| Ok(
                    InputTime::parse("").unwrap(),
                ));
                Ok(CsvEventSource {
                    reader: wrapper,
                    csv_column_mapping,
                    get_time,
                    timer: PhantomData,
                })
            }
        }
    }
    impl<InputTime: TimeRepresentation> EventSource<InputTime>
    for CsvEventSource<InputTime> {
        type Error = CsvError;
        type MappedEvent = CsvRecord;
        fn init_data(&self) -> Result<<CsvRecord as InputMap>::CreationData, CsvError> {
            Ok(self.csv_column_mapping.clone())
        }
        fn next_event(
            &mut self,
        ) -> Result<Option<(CsvRecord, InputTime::InnerTime)>, CsvError> {
            let mut res = ByteRecord::new();
            self.reader
                .read_record(&mut res)
                .map_err(|e| CsvError::Validation({
                    let res = ::alloc::fmt::format(
                        format_args!("Error reading csv file: {0}", e),
                    );
                    res
                }))
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
}
#[cfg(feature = "pcap_plugin")]
pub mod pcap_plugin {
    //! This plugins enables the parsing of network packets either from a pcap file or from a network device.
    use std::error::Error;
    use std::fmt::{Display, Formatter};
    use std::marker::PhantomData;
    use std::net::IpAddr;
    use std::num::TryFromIntError;
    use std::path::PathBuf;
    use std::str::FromStr;
    use etherparse::{
        Ethernet2Header, InternetSlice, Ipv4Header, Ipv6Header, LinkSlice, SlicedPacket,
        TcpHeader, TransportSlice, UdpHeader,
    };
    use ip_network::IpNetwork;
    use pcap::{Activated, Capture, Device, Error as PCAPError, Packet};
    use rtlola_interpreter::input::{EventFactoryError, InputMap, ValueGetter};
    use rtlola_interpreter::time::TimeRepresentation;
    use rtlola_interpreter::Value;
    use crate::EventSource;
    fn get_ethernet_header(packet: &SlicedPacket) -> Option<Ethernet2Header> {
        match &packet.link {
            Some(s) => {
                match s {
                    LinkSlice::Ethernet2(hdr) => Some(hdr.to_header()),
                }
            }
            None => None,
        }
    }
    fn ethernet_source(packet: &SlicedPacket) -> Value {
        if let Some(ethernet_header) = get_ethernet_header(packet) {
            let values: Vec<Value> = ethernet_header
                .source
                .iter()
                .map(|v: &u8| Value::Unsigned((*v).into()))
                .collect();
            Value::Tuple(values.into_boxed_slice())
        } else {
            Value::None
        }
    }
    fn ethernet_destination(packet: &SlicedPacket) -> Value {
        if let Some(ethernet_header) = get_ethernet_header(packet) {
            let values: Vec<Value> = ethernet_header
                .destination
                .iter()
                .map(|v: &u8| Value::Unsigned((*v).into()))
                .collect();
            Value::Tuple(values.into_boxed_slice())
        } else {
            Value::None
        }
    }
    fn ethernet_type(packet: &SlicedPacket) -> Value {
        if let Some(ethernet_header) = get_ethernet_header(packet) {
            Value::Unsigned(ethernet_header.ether_type.into())
        } else {
            Value::None
        }
    }
    fn get_ipv4_header(packet: &SlicedPacket) -> Option<Ipv4Header> {
        match &packet.ip {
            Some(int_slice) => {
                match int_slice {
                    InternetSlice::Ipv4(h, _) => Some(h.to_header()),
                    InternetSlice::Ipv6(_, _) => None,
                }
            }
            None => None,
        }
    }
    fn ipv4_source(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_ipv4_header(packet) {
            let values: Vec<Value> = header
                .source
                .iter()
                .map(|v: &u8| Value::Unsigned((*v).into()))
                .collect();
            Value::Tuple(values.into_boxed_slice())
        } else {
            Value::None
        }
    }
    fn ipv4_destination(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_ipv4_header(packet) {
            let values: Vec<Value> = header
                .destination
                .iter()
                .map(|v: &u8| Value::Unsigned((*v).into()))
                .collect();
            Value::Tuple(values.into_boxed_slice())
        } else {
            Value::None
        }
    }
    fn ipv4_ihl(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_ipv4_header(packet) {
            Value::Unsigned(header.ihl().into())
        } else {
            Value::None
        }
    }
    fn ipv4_dscp(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_ipv4_header(packet) {
            Value::Unsigned(header.differentiated_services_code_point.into())
        } else {
            Value::None
        }
    }
    fn ipv4_ecn(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_ipv4_header(packet) {
            Value::Unsigned(header.explicit_congestion_notification.into())
        } else {
            Value::None
        }
    }
    fn ipv4_length(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_ipv4_header(packet) {
            Value::Unsigned(header.total_len().into())
        } else {
            Value::None
        }
    }
    fn ipv4_id(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_ipv4_header(packet) {
            Value::Unsigned(header.identification.into())
        } else {
            Value::None
        }
    }
    fn ipv4_fragment_offset(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_ipv4_header(packet) {
            Value::Unsigned(header.fragments_offset.into())
        } else {
            Value::None
        }
    }
    fn ipv4_ttl(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_ipv4_header(packet) {
            Value::Unsigned(header.time_to_live.into())
        } else {
            Value::None
        }
    }
    fn ipv4_protocol(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_ipv4_header(packet) {
            Value::Unsigned(header.protocol.into())
        } else {
            Value::None
        }
    }
    fn ipv4_checksum(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_ipv4_header(packet) {
            Value::Unsigned(header.header_checksum.into())
        } else {
            Value::None
        }
    }
    fn ipv4_flags_df(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_ipv4_header(packet) {
            Value::Bool(header.dont_fragment)
        } else {
            Value::None
        }
    }
    fn ipv4_flags_mf(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_ipv4_header(packet) {
            Value::Bool(header.more_fragments)
        } else {
            Value::None
        }
    }
    fn get_ipv6_header(packet: &SlicedPacket) -> Option<Ipv6Header> {
        match &packet.ip {
            Some(int_slice) => {
                match int_slice {
                    InternetSlice::Ipv4(_, _) => None,
                    InternetSlice::Ipv6(h, _) => Some(h.to_header()),
                }
            }
            None => None,
        }
    }
    fn ipv6_source(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_ipv6_header(packet) {
            let values: Vec<Value> = header
                .source
                .iter()
                .map(|v: &u8| Value::Unsigned((*v).into()))
                .collect();
            Value::Tuple(values.into_boxed_slice())
        } else {
            Value::None
        }
    }
    fn ipv6_destination(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_ipv6_header(packet) {
            let values: Vec<Value> = header
                .destination
                .iter()
                .map(|v: &u8| Value::Unsigned((*v).into()))
                .collect();
            Value::Tuple(values.into_boxed_slice())
        } else {
            Value::None
        }
    }
    fn ipv6_traffic_class(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_ipv6_header(packet) {
            Value::Unsigned(header.traffic_class.into())
        } else {
            Value::None
        }
    }
    fn ipv6_flow_label(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_ipv6_header(packet) {
            Value::Unsigned(header.flow_label.into())
        } else {
            Value::None
        }
    }
    fn ipv6_length(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_ipv6_header(packet) {
            Value::Unsigned(header.payload_length.into())
        } else {
            Value::None
        }
    }
    fn ipv6_hop_limit(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_ipv6_header(packet) {
            Value::Unsigned(header.hop_limit.into())
        } else {
            Value::None
        }
    }
    fn get_tcp_header(packet: &SlicedPacket) -> Option<TcpHeader> {
        use TransportSlice::*;
        match &packet.transport {
            Some(t) => {
                match t {
                    Tcp(h) => Some(h.to_header()),
                    Udp(_) | Icmpv4(_) | Icmpv6(_) | Unknown(_) => None,
                }
            }
            None => None,
        }
    }
    fn tcp_source_port(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_tcp_header(packet) {
            Value::Unsigned(header.source_port.into())
        } else {
            Value::None
        }
    }
    fn tcp_destination_port(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_tcp_header(packet) {
            Value::Unsigned(header.destination_port.into())
        } else {
            Value::None
        }
    }
    fn tcp_seq_number(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_tcp_header(packet) {
            Value::Unsigned(header.sequence_number.into())
        } else {
            Value::None
        }
    }
    fn tcp_ack_number(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_tcp_header(packet) {
            Value::Unsigned(header.acknowledgment_number.into())
        } else {
            Value::None
        }
    }
    fn tcp_data_offset(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_tcp_header(packet) {
            Value::Unsigned(header.data_offset().into())
        } else {
            Value::None
        }
    }
    fn tcp_flags_ns(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_tcp_header(packet) {
            Value::Bool(header.ns)
        } else {
            Value::None
        }
    }
    fn tcp_flags_fin(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_tcp_header(packet) {
            Value::Bool(header.fin)
        } else {
            Value::None
        }
    }
    fn tcp_flags_syn(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_tcp_header(packet) {
            Value::Bool(header.syn)
        } else {
            Value::None
        }
    }
    fn tcp_flags_rst(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_tcp_header(packet) {
            Value::Bool(header.rst)
        } else {
            Value::None
        }
    }
    fn tcp_flags_psh(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_tcp_header(packet) {
            Value::Bool(header.psh)
        } else {
            Value::None
        }
    }
    fn tcp_flags_ack(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_tcp_header(packet) {
            Value::Bool(header.ack)
        } else {
            Value::None
        }
    }
    fn tcp_flags_urg(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_tcp_header(packet) {
            Value::Bool(header.urg)
        } else {
            Value::None
        }
    }
    fn tcp_flags_ece(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_tcp_header(packet) {
            Value::Bool(header.ece)
        } else {
            Value::None
        }
    }
    fn tcp_flags_cwr(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_tcp_header(packet) {
            Value::Bool(header.cwr)
        } else {
            Value::None
        }
    }
    fn tcp_window_size(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_tcp_header(packet) {
            Value::Unsigned(header.window_size.into())
        } else {
            Value::None
        }
    }
    fn tcp_checksum(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_tcp_header(packet) {
            Value::Unsigned(header.checksum.into())
        } else {
            Value::None
        }
    }
    fn tcp_urgent_pointer(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_tcp_header(packet) {
            Value::Unsigned(header.urgent_pointer.into())
        } else {
            Value::None
        }
    }
    fn get_udp_header(packet: &SlicedPacket) -> Option<UdpHeader> {
        use TransportSlice::*;
        match &packet.transport {
            Some(t) => {
                match t {
                    Tcp(_) | Icmpv4(_) | Icmpv6(_) | Unknown(_) => None,
                    Udp(h) => Some(h.to_header()),
                }
            }
            None => None,
        }
    }
    fn udp_source_port(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_udp_header(packet) {
            Value::Unsigned(header.source_port.into())
        } else {
            Value::None
        }
    }
    fn udp_destination_port(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_udp_header(packet) {
            Value::Unsigned(header.destination_port.into())
        } else {
            Value::None
        }
    }
    fn udp_length(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_udp_header(packet) {
            Value::Unsigned(header.length.into())
        } else {
            Value::None
        }
    }
    fn udp_checksum(packet: &SlicedPacket) -> Value {
        if let Some(header) = get_udp_header(packet) {
            Value::Unsigned(header.checksum.into())
        } else {
            Value::None
        }
    }
    fn get_packet_payload(packet: &SlicedPacket) -> Value {
        Value::Str(String::from_utf8_lossy(packet.payload).as_ref().into())
    }
    fn get_packet_protocol(packet: &SlicedPacket) -> Value {
        match &packet.transport {
            Some(transport) => {
                match transport {
                    TransportSlice::Tcp(_) => Value::Str("TCP".into()),
                    TransportSlice::Udp(_) => Value::Str("UDP".into()),
                    TransportSlice::Icmpv4(_) => Value::Str("ICMPv4".into()),
                    TransportSlice::Icmpv6(_) => Value::Str("ICMPv6".into()),
                    TransportSlice::Unknown(_) => Value::Str("Unknown".into()),
                }
            }
            None => {
                match &packet.ip {
                    Some(ip) => {
                        match ip {
                            InternetSlice::Ipv4(_, _) => Value::Str("IPv4".into()),
                            InternetSlice::Ipv6(_, _) => Value::Str("IPv6".into()),
                        }
                    }
                    None => {
                        match &packet.link {
                            Some(link) => {
                                match link {
                                    LinkSlice::Ethernet2(_) => Value::Str("Ethernet2".into()),
                                }
                            }
                            None => Value::Str("Unknown".into()),
                        }
                    }
                }
            }
        }
    }
    /// Represents different kind of errors that might occur.
    pub enum PcapError {
        UnknownInput(String),
        UnknownDevice(String, Vec<String>),
        InvalidLocalNetwork(String, ip_network::IpNetworkParseError),
        TimeParseError(TryFromIntError),
        TimeFormatError(String),
        Pcap(pcap::Error),
    }
    #[automatically_derived]
    impl ::core::fmt::Debug for PcapError {
        #[inline]
        fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
            match self {
                PcapError::UnknownInput(__self_0) => {
                    ::core::fmt::Formatter::debug_tuple_field1_finish(
                        f,
                        "UnknownInput",
                        &__self_0,
                    )
                }
                PcapError::UnknownDevice(__self_0, __self_1) => {
                    ::core::fmt::Formatter::debug_tuple_field2_finish(
                        f,
                        "UnknownDevice",
                        __self_0,
                        &__self_1,
                    )
                }
                PcapError::InvalidLocalNetwork(__self_0, __self_1) => {
                    ::core::fmt::Formatter::debug_tuple_field2_finish(
                        f,
                        "InvalidLocalNetwork",
                        __self_0,
                        &__self_1,
                    )
                }
                PcapError::TimeParseError(__self_0) => {
                    ::core::fmt::Formatter::debug_tuple_field1_finish(
                        f,
                        "TimeParseError",
                        &__self_0,
                    )
                }
                PcapError::TimeFormatError(__self_0) => {
                    ::core::fmt::Formatter::debug_tuple_field1_finish(
                        f,
                        "TimeFormatError",
                        &__self_0,
                    )
                }
                PcapError::Pcap(__self_0) => {
                    ::core::fmt::Formatter::debug_tuple_field1_finish(
                        f,
                        "Pcap",
                        &__self_0,
                    )
                }
            }
        }
    }
    impl Display for PcapError {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            match self {
                PcapError::UnknownInput(i) => {
                    f.write_fmt(format_args!("Malformed input name: {0}", i))
                }
                PcapError::UnknownDevice(d, available) => {
                    f.write_fmt(
                        format_args!(
                            "Interface {0} was not found. Available interfaces are: {1}",
                            d,
                            available.join(", "),
                        ),
                    )
                }
                PcapError::InvalidLocalNetwork(name, e) => {
                    f.write_fmt(
                        format_args!(
                            "Could not parse local network range: {0}. Error: {1}",
                            *name,
                            e,
                        ),
                    )
                }
                PcapError::TimeParseError(e) => {
                    f.write_fmt(
                        format_args!("Could not parse timestamp from packet: {0}", e),
                    )
                }
                PcapError::TimeFormatError(e) => {
                    f.write_fmt(
                        format_args!("Could not parse timestamp to time format: {0}", e),
                    )
                }
                PcapError::Pcap(e) => {
                    f.write_fmt(format_args!("Could not process packet: {0}", e))
                }
            }
        }
    }
    impl Error for PcapError {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            match self {
                PcapError::UnknownInput(_) => None,
                PcapError::UnknownDevice(_, _) => None,
                PcapError::InvalidLocalNetwork(_, e) => Some(e),
                PcapError::TimeParseError(e) => Some(e),
                PcapError::TimeFormatError(_) => None,
                PcapError::Pcap(e) => Some(e),
            }
        }
    }
    impl From<PcapError> for EventFactoryError {
        fn from(value: PcapError) -> Self {
            match value {
                PcapError::UnknownInput(i) => {
                    EventFactoryError::InputStreamUnknown(
                        <[_]>::into_vec(#[rustc_box] ::alloc::boxed::Box::new([i])),
                    )
                }
                e => EventFactoryError::Other(Box::new(e)),
            }
        }
    }
    /// Represents a raw network packet
    pub struct PcapRecord(Vec<u8>);
    impl InputMap for PcapRecord {
        type CreationData = IpNetwork;
        type Error = PcapError;
        fn func_for_input(
            name: &str,
            data: Self::CreationData,
        ) -> Result<ValueGetter<Self, PcapError>, PcapError> {
            let local_net = data;
            let layers: Vec<&str> = name.split("::").collect();
            if layers.len() > 3 || layers.is_empty() {
                return Err(PcapError::UnknownInput(name.to_string()));
            }
            let get_packet_direction = move |packet: &SlicedPacket| -> Value {
                let addr: IpAddr = match &packet.ip {
                    Some(ip) => {
                        match ip {
                            InternetSlice::Ipv4(header, _) => {
                                IpAddr::V4(header.destination_addr())
                            }
                            InternetSlice::Ipv6(header, _) => {
                                IpAddr::V6(header.destination_addr())
                            }
                        }
                    }
                    None => return Value::None,
                };
                if local_net.contains(addr) {
                    Value::Str("Incoming".into())
                } else {
                    Value::Str("Outgoing".into())
                }
            };
            let inner_fn: Box<dyn Fn(&SlicedPacket) -> Value> = match layers[0] {
                "Ethernet" => {
                    if layers.len() != 2 {
                        return Err(PcapError::UnknownInput(name.to_string()));
                    }
                    match layers[1] {
                        "source" => Box::new(ethernet_source),
                        "destination" => Box::new(ethernet_destination),
                        "type" => Box::new(ethernet_type),
                        _ => {
                            return Err(PcapError::UnknownInput(name.to_string()));
                        }
                    }
                }
                "IPv4" => {
                    if layers.len() < 2 {
                        return Err(PcapError::UnknownInput(name.to_string()));
                    }
                    match layers[1] {
                        "source" => Box::new(ipv4_source),
                        "destination" => Box::new(ipv4_destination),
                        "ihl" => Box::new(ipv4_ihl),
                        "dscp" => Box::new(ipv4_dscp),
                        "ecn" => Box::new(ipv4_ecn),
                        "length" => Box::new(ipv4_length),
                        "identification" => Box::new(ipv4_id),
                        "fragment_offset" => Box::new(ipv4_fragment_offset),
                        "ttl" => Box::new(ipv4_ttl),
                        "protocol" => Box::new(ipv4_protocol),
                        "checksum" => Box::new(ipv4_checksum),
                        "flags" => {
                            if layers.len() < 3 {
                                return Err(PcapError::UnknownInput(name.to_string()));
                            }
                            match layers[2] {
                                "df" => Box::new(ipv4_flags_df),
                                "mf" => Box::new(ipv4_flags_mf),
                                _ => {
                                    return Err(PcapError::UnknownInput(name.to_string()));
                                }
                            }
                        }
                        _ => {
                            return Err(PcapError::UnknownInput(name.to_string()));
                        }
                    }
                }
                "IPv6" => {
                    if layers.len() < 2 {
                        return Err(PcapError::UnknownInput(name.to_string()));
                    }
                    match layers[1] {
                        "source" => Box::new(ipv6_source),
                        "destination" => Box::new(ipv6_destination),
                        "traffic_class" => Box::new(ipv6_traffic_class),
                        "flow_label" => Box::new(ipv6_flow_label),
                        "length" => Box::new(ipv6_length),
                        "hop_limit" => Box::new(ipv6_hop_limit),
                        _ => {
                            return Err(PcapError::UnknownInput(name.to_string()));
                        }
                    }
                }
                "TCP" => {
                    if layers.len() < 2 {
                        return Err(PcapError::UnknownInput(name.to_string()));
                    }
                    match layers[1] {
                        "source" => Box::new(tcp_source_port),
                        "destination" => Box::new(tcp_destination_port),
                        "seq_number" => Box::new(tcp_seq_number),
                        "ack_number" => Box::new(tcp_ack_number),
                        "data_offset" => Box::new(tcp_data_offset),
                        "window_size" => Box::new(tcp_window_size),
                        "checksum" => Box::new(tcp_checksum),
                        "urgent_pointer" => Box::new(tcp_urgent_pointer),
                        "flags" => {
                            if layers.len() < 3 {
                                return Err(PcapError::UnknownInput(name.to_string()));
                            }
                            match layers[2] {
                                "ns" => Box::new(tcp_flags_ns),
                                "fin" => Box::new(tcp_flags_fin),
                                "syn" => Box::new(tcp_flags_syn),
                                "rst" => Box::new(tcp_flags_rst),
                                "psh" => Box::new(tcp_flags_psh),
                                "ack" => Box::new(tcp_flags_ack),
                                "urg" => Box::new(tcp_flags_urg),
                                "ece" => Box::new(tcp_flags_ece),
                                "cwr" => Box::new(tcp_flags_cwr),
                                _ => {
                                    return Err(PcapError::UnknownInput(name.to_string()));
                                }
                            }
                        }
                        _ => {
                            return Err(PcapError::UnknownInput(name.to_string()));
                        }
                    }
                }
                "UDP" => {
                    if layers.len() < 2 {
                        return Err(PcapError::UnknownInput(name.to_string()));
                    }
                    match layers[1] {
                        "source" => Box::new(udp_source_port),
                        "destination" => Box::new(udp_destination_port),
                        "length" => Box::new(udp_length),
                        "checksum" => Box::new(udp_checksum),
                        _ => {
                            return Err(PcapError::UnknownInput(name.to_string()));
                        }
                    }
                }
                "payload" => Box::new(get_packet_payload),
                "direction" => Box::new(get_packet_direction),
                "protocol" => Box::new(get_packet_protocol),
                _ => {
                    return Err(PcapError::UnknownInput(name.to_string()));
                }
            };
            Ok(
                Box::new(move |packet: &PcapRecord| {
                    let packet = SlicedPacket::from_ethernet(packet.0.as_slice())
                        .expect("Could not parse packet!");
                    Ok(inner_fn(&packet))
                }),
            )
        }
    }
    /// Determines the input source for network packets.
    pub enum PcapInputSource {
        /// Use the specified device for packet input.
        Device {
            /// The name of the device to use.
            name: String,
            /// The description of your local network IP address range in CIDR-Notation.
            local_network: String,
        },
        /// Use the given file for packet input.
        File {
            /// The path to the PCAP file.
            path: PathBuf,
            /// The description of your local network IP address range in CIDR-Notation.
            local_network: String,
        },
    }
    #[automatically_derived]
    impl ::core::fmt::Debug for PcapInputSource {
        #[inline]
        fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
            match self {
                PcapInputSource::Device { name: __self_0, local_network: __self_1 } => {
                    ::core::fmt::Formatter::debug_struct_field2_finish(
                        f,
                        "Device",
                        "name",
                        __self_0,
                        "local_network",
                        &__self_1,
                    )
                }
                PcapInputSource::File { path: __self_0, local_network: __self_1 } => {
                    ::core::fmt::Formatter::debug_struct_field2_finish(
                        f,
                        "File",
                        "path",
                        __self_0,
                        "local_network",
                        &__self_1,
                    )
                }
            }
        }
    }
    #[automatically_derived]
    impl ::core::clone::Clone for PcapInputSource {
        #[inline]
        fn clone(&self) -> PcapInputSource {
            match self {
                PcapInputSource::Device { name: __self_0, local_network: __self_1 } => {
                    PcapInputSource::Device {
                        name: ::core::clone::Clone::clone(__self_0),
                        local_network: ::core::clone::Clone::clone(__self_1),
                    }
                }
                PcapInputSource::File { path: __self_0, local_network: __self_1 } => {
                    PcapInputSource::File {
                        path: ::core::clone::Clone::clone(__self_0),
                        local_network: ::core::clone::Clone::clone(__self_1),
                    }
                }
            }
        }
    }
    /// Parses events from network packets.
    #[allow(missing_debug_implementations)]
    pub struct PcapEventSource<InputTime: TimeRepresentation> {
        capture_handle: Capture<dyn Activated>,
        timer: PhantomData<InputTime>,
        local_net: IpNetwork,
    }
    impl<InputTime: TimeRepresentation> PcapEventSource<InputTime> {
        pub fn setup(
            src: &PcapInputSource,
        ) -> Result<PcapEventSource<InputTime>, PcapError> {
            let capture_handle = match src {
                PcapInputSource::Device { name, .. } => {
                    let all_devices = Device::list().map_err(PcapError::Pcap)?;
                    let device_names: Vec<_> = all_devices
                        .iter()
                        .map(|d| d.name.clone())
                        .collect();
                    let dev: Device = all_devices
                        .into_iter()
                        .find(|d| &d.name == name)
                        .ok_or_else(|| PcapError::UnknownDevice(
                            name.clone(),
                            device_names,
                        ))?;
                    let capture_handle = Capture::from_device(dev)
                        .map_err(PcapError::Pcap)?
                        .promisc(true)
                        .snaplen(65535)
                        .open()
                        .map_err(PcapError::Pcap)?;
                    capture_handle.into()
                }
                PcapInputSource::File { path, .. } => {
                    let capture_handle = Capture::from_file(path)
                        .map_err(PcapError::Pcap)?;
                    capture_handle.into()
                }
            };
            let local_network_range = match src {
                PcapInputSource::Device { local_network, .. } => local_network,
                PcapInputSource::File { local_network, .. } => local_network,
            };
            let local_net = IpNetwork::from_str(local_network_range.as_ref())
                .map_err(|e| PcapError::InvalidLocalNetwork(
                    local_network_range.clone(),
                    e,
                ))?;
            Ok(Self {
                capture_handle,
                timer: Default::default(),
                local_net,
            })
        }
    }
    impl<InputTime: TimeRepresentation> EventSource<InputTime>
    for PcapEventSource<InputTime> {
        type Error = PcapError;
        type MappedEvent = PcapRecord;
        fn init_data(&self) -> Result<IpNetwork, PcapError> {
            Ok(self.local_net)
        }
        fn next_event(
            &mut self,
        ) -> Result<Option<(PcapRecord, InputTime::InnerTime)>, PcapError> {
            let raw_packet: Packet = match self.capture_handle.next_packet() {
                Ok(pkt) => pkt,
                Err(PCAPError::NoMorePackets) => return Ok(None),
                Err(e) => return Err(PcapError::Pcap(e)),
            };
            let p = (*raw_packet).to_vec();
            let (secs, nanos) = u64::try_from(raw_packet.header.ts.tv_sec)
                .and_then(|secs| {
                    u32::try_from(raw_packet.header.ts.tv_usec * 1000)
                        .map(|sub_secs| (secs, sub_secs))
                })
                .map_err(PcapError::TimeParseError)?;
            let time_str = {
                let res = ::alloc::fmt::format(format_args!("{0}.{1:09}", secs, nanos));
                res
            };
            let time = InputTime::parse(&time_str).map_err(PcapError::TimeFormatError)?;
            Ok(Some((PcapRecord(p), time)))
        }
    }
}
#[cfg(feature = "network_plugin")]
pub mod network_plugin {
    use std::error::Error;
    use std::fmt::Debug;
    use std::marker::PhantomData;
    use byteorder::ByteOrder;
    use rtlola_interpreter::input::InputMap;
    use rtlola_interpreter::time::TimeRepresentation;
    use time_converter::TimeConverter;
    use crate::EventSource;
    pub mod reader {}
    pub mod time_converter {
        use rtlola_interpreter::input::InputMap;
        use rtlola_interpreter::time::{DelayTime, RealTime, TimeRepresentation};
        /// Trait to convert a value interpreted as time to a [TimeRepresentation] used by the [rtlola_interpreter::Monitor].
        pub trait TimeConverter<T: TimeRepresentation>: Sized + InputMap {
            /// Converts a value to a [TimeRepresentation].
            fn convert_time(
                &self,
            ) -> Result<<T as TimeRepresentation>::InnerTime, <Self as InputMap>::Error>;
        }
        impl<Map: InputMap> TimeConverter<DelayTime> for Map {
            fn convert_time(
                &self,
            ) -> Result<
                <DelayTime as TimeRepresentation>::InnerTime,
                <Self as InputMap>::Error,
            > {
                Ok(())
            }
        }
        impl<Map: InputMap> TimeConverter<RealTime> for Map {
            fn convert_time(
                &self,
            ) -> Result<
                <RealTime as TimeRepresentation>::InnerTime,
                <Self as InputMap>::Error,
            > {
                Ok(())
            }
        }
    }
    pub mod upd {
        use std::net::SocketAddr;
        use std::usize;
        use super::NetworkSource;
        pub struct UdpWrapper(std::net::UdpSocket);
        #[automatically_derived]
        impl ::core::fmt::Debug for UdpWrapper {
            #[inline]
            fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                ::core::fmt::Formatter::debug_tuple_field1_finish(
                    f,
                    "UdpWrapper",
                    &&self.0,
                )
            }
        }
        impl From<std::net::UdpSocket> for UdpWrapper {
            fn from(value: std::net::UdpSocket) -> Self {
                UdpWrapper(value)
            }
        }
        impl std::io::Read for UdpWrapper {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.0.recv(buf)
            }
        }
        pub struct CheckedUdpSocket {
            socket: std::net::UdpSocket,
            allowed_sender: Vec<SocketAddr>,
        }
        #[automatically_derived]
        impl ::core::fmt::Debug for CheckedUdpSocket {
            #[inline]
            fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                ::core::fmt::Formatter::debug_struct_field2_finish(
                    f,
                    "CheckedUdpSocket",
                    "socket",
                    &self.socket,
                    "allowed_sender",
                    &&self.allowed_sender,
                )
            }
        }
        impl CheckedUdpSocket {
            pub fn new(
                socket: std::net::UdpSocket,
                allowed_sender: Vec<SocketAddr>,
            ) -> Self {
                Self { socket, allowed_sender }
            }
        }
        impl NetworkSource for CheckedUdpSocket {
            type Error = CheckUdpError;
            fn read(&mut self, buffer: &mut [u8]) -> Result<Option<usize>, Self::Error> {
                match self.socket.recv_from(buffer) {
                    Ok(
                        (package_size, sender),
                    ) if self.allowed_sender.contains(&sender) => Ok(Some(package_size)),
                    Ok((_, sender)) => Err(CheckUdpError::InvalidSender(sender)),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
                    Err(e) => Err(e.into()),
                }
            }
        }
        pub enum CheckUdpError {
            IOError(std::io::Error),
            InvalidSender(SocketAddr),
        }
        #[automatically_derived]
        impl ::core::fmt::Debug for CheckUdpError {
            #[inline]
            fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                match self {
                    CheckUdpError::IOError(__self_0) => {
                        ::core::fmt::Formatter::debug_tuple_field1_finish(
                            f,
                            "IOError",
                            &__self_0,
                        )
                    }
                    CheckUdpError::InvalidSender(__self_0) => {
                        ::core::fmt::Formatter::debug_tuple_field1_finish(
                            f,
                            "InvalidSender",
                            &__self_0,
                        )
                    }
                }
            }
        }
        impl std::fmt::Display for CheckUdpError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    CheckUdpError::IOError(e) => f.write_fmt(format_args!("{0}", e)),
                    CheckUdpError::InvalidSender(sender) => {
                        f.write_fmt(format_args!("Received package from {0}", sender))
                    }
                }
            }
        }
        impl std::error::Error for CheckUdpError {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                match self {
                    CheckUdpError::IOError(e) => Some(e),
                    CheckUdpError::InvalidSender(_) => None,
                }
            }
        }
        impl From<std::io::Error> for CheckUdpError {
            fn from(value: std::io::Error) -> Self {
                CheckUdpError::IOError(value)
            }
        }
    }
    pub struct NetworkEventSource<
        Source: NetworkSource,
        Order: ByteOrder,
        Factory: ByteFactory<Order>,
        InputTime: TimeRepresentation,
        const BUFFERSIZE: usize,
    >
    where
        <Factory::Input as InputMap>::Error: Error,
    {
        factory: Factory,
        source: Source,
        buffer: Vec<u8>,
        timer: PhantomData<(InputTime, Order)>,
    }
    impl<
        Source: NetworkSource,
        Order: ByteOrder,
        Factory: ByteFactory<Order>,
        InputTime: TimeRepresentation,
        const BUFFERSIZE: usize,
    > NetworkEventSource<Source, Order, Factory, InputTime, BUFFERSIZE>
    where
        <Factory::Input as InputMap>::Error: Error,
    {
        pub fn new(source: Source, factory: Factory) -> Self {
            Self {
                factory,
                source,
                buffer: Vec::new(),
                timer: PhantomData::default(),
            }
        }
    }
    /// Enum to collect the errors with for a [NetworkEventSource].
    pub enum NetworkEventSourceError<
        Factory: Error + Debug,
        Source: Error + Debug,
        InputMap: Error + Debug,
    > {
        /// Error while receiving a bytestream.
        Source(Source),
        /// Error while parsing a bytestream.
        Factory(Factory),
        InputMap(InputMap),
    }
    #[automatically_derived]
    impl<
        Factory: ::core::fmt::Debug + Error + Debug,
        Source: ::core::fmt::Debug + Error + Debug,
        InputMap: ::core::fmt::Debug + Error + Debug,
    > ::core::fmt::Debug for NetworkEventSourceError<Factory, Source, InputMap> {
        #[inline]
        fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
            match self {
                NetworkEventSourceError::Source(__self_0) => {
                    ::core::fmt::Formatter::debug_tuple_field1_finish(
                        f,
                        "Source",
                        &__self_0,
                    )
                }
                NetworkEventSourceError::Factory(__self_0) => {
                    ::core::fmt::Formatter::debug_tuple_field1_finish(
                        f,
                        "Factory",
                        &__self_0,
                    )
                }
                NetworkEventSourceError::InputMap(__self_0) => {
                    ::core::fmt::Formatter::debug_tuple_field1_finish(
                        f,
                        "InputMap",
                        &__self_0,
                    )
                }
            }
        }
    }
    impl<
        Factory: Error + Debug,
        Source: Error + Debug,
        InputMap: Error + Debug,
    > std::fmt::Display for NetworkEventSourceError<Factory, Source, InputMap> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                NetworkEventSourceError::Source(e) => f.write_fmt(format_args!("{0}", e)),
                NetworkEventSourceError::Factory(e) => {
                    f.write_fmt(format_args!("{0}", e))
                }
                NetworkEventSourceError::InputMap(e) => {
                    f.write_fmt(format_args!("{0}", e))
                }
            }
        }
    }
    impl<
        Factory: Error + Debug + 'static,
        Source: Error + Debug + 'static,
        InputMap: Error + Debug + 'static,
    > std::error::Error for NetworkEventSourceError<Factory, Source, InputMap> {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            match self {
                NetworkEventSourceError::Source(e) => Some(e),
                NetworkEventSourceError::Factory(e) => Some(e),
                NetworkEventSourceError::InputMap(e) => Some(e),
            }
        }
    }
    impl<
        InputTime: TimeRepresentation,
        Factory: ByteFactory<Order>,
        Source: NetworkSource + Debug,
        Order: ByteOrder,
        const BUFFERSIZE: usize,
    > EventSource<InputTime>
    for NetworkEventSource<Source, Order, Factory, InputTime, BUFFERSIZE>
    where
        <Factory::Input as InputMap>::Error: Error,
        Factory::Input: TimeConverter<InputTime>,
    {
        type Error = NetworkEventSourceError<
            Factory::Error,
            Source::Error,
            <Factory::Input as InputMap>::Error,
        >;
        type MappedEvent = Factory::Input;
        fn init_data(
            &self,
        ) -> Result<<Self::MappedEvent as InputMap>::CreationData, Self::Error> {
            ::core::panicking::panic("not yet implemented")
        }
        fn next_event(
            &mut self,
        ) -> crate::EventResult<
            Self::MappedEvent,
            <InputTime as TimeRepresentation>::InnerTime,
            Self::Error,
        > {
            loop {
                let event = self
                    .factory
                    .from_bytes(&self.buffer)
                    .map(|(event, package_size)| {
                        let ts = <<Factory as ByteFactory<
                            Order,
                        >>::Input as TimeConverter<InputTime>>::convert_time(&event)
                            .map_err(|e| NetworkEventSourceError::InputMap(e))?;
                        let slice = self.buffer.drain(0..package_size);
                        if true {
                            match (&slice.len(), &package_size) {
                                (left_val, right_val) => {
                                    if !(*left_val == *right_val) {
                                        let kind = ::core::panicking::AssertKind::Eq;
                                        ::core::panicking::assert_failed(
                                            kind,
                                            &*left_val,
                                            &*right_val,
                                            ::core::option::Option::None,
                                        );
                                    }
                                }
                            };
                        }
                        Ok((event, ts))
                    });
                match event {
                    Ok(res) => break Ok(Some(res?)),
                    Err(ByteParsingError::Incomplete) => {
                        let mut temp_buffer = [0_u8; BUFFERSIZE];
                        let package_size = self
                            .source
                            .read(&mut temp_buffer)
                            .map_err(NetworkEventSourceError::Source)?;
                        match package_size {
                            None => break Ok(None),
                            Some(package_size) => {
                                self.buffer.extend_from_slice(&temp_buffer[0..package_size])
                            }
                        }
                    }
                    Err(ByteParsingError::Inner(e)) => {
                        break Err(NetworkEventSourceError::Factory(e));
                    }
                }
            }
        }
    }
    pub trait NetworkSource {
        type Error: Error + 'static;
        fn read(&mut self, buffer: &mut [u8]) -> Result<Option<usize>, Self::Error>;
    }
    impl<T: std::io::Read> NetworkSource for T {
        type Error = std::io::Error;
        fn read(&mut self, buffer: &mut [u8]) -> Result<Option<usize>, Self::Error> {
            match std::io::Read::read(self, buffer) {
                Ok(size) => Ok(Some(size)),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
                Err(e) => Err(e),
            }
        }
    }
    pub trait ByteFactory<B: ByteOrder> {
        type Error: Error + 'static;
        type Input: InputMap;
        fn from_bytes(
            &mut self,
            data: &[u8],
        ) -> Result<(Self::Input, usize), ByteParsingError<Self::Error>>
        where
            Self: Sized;
    }
    /// The error returned if anything goes wrong when parsing the bytestream
    pub enum ByteParsingError<R: Error> {
        /// Parsing Error
        Inner(R),
        /// Error to inducate that the number of bytes is insuffienct to parse the event
        Incomplete,
    }
    #[automatically_derived]
    impl<R: ::core::fmt::Debug + Error> ::core::fmt::Debug for ByteParsingError<R> {
        #[inline]
        fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
            match self {
                ByteParsingError::Inner(__self_0) => {
                    ::core::fmt::Formatter::debug_tuple_field1_finish(
                        f,
                        "Inner",
                        &__self_0,
                    )
                }
                ByteParsingError::Incomplete => {
                    ::core::fmt::Formatter::write_str(f, "Incomplete")
                }
            }
        }
    }
    pub trait FromBytes<B: ByteOrder> {
        type Error: Error + 'static;
        fn from_bytes(
            data: &[u8],
        ) -> Result<(Self, usize), ByteParsingError<Self::Error>>
        where
            Self: Sized;
    }
    pub struct EmptyByteFactory<B: FromBytes<Order> + InputMap, Order: ByteOrder> {
        phantom: PhantomData<(B, Order)>,
    }
    #[automatically_derived]
    impl<
        B: ::core::fmt::Debug + FromBytes<Order> + InputMap,
        Order: ::core::fmt::Debug + ByteOrder,
    > ::core::fmt::Debug for EmptyByteFactory<B, Order> {
        #[inline]
        fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
            ::core::fmt::Formatter::debug_struct_field1_finish(
                f,
                "EmptyByteFactory",
                "phantom",
                &&self.phantom,
            )
        }
    }
    #[automatically_derived]
    impl<
        B: ::core::clone::Clone + FromBytes<Order> + InputMap,
        Order: ::core::clone::Clone + ByteOrder,
    > ::core::clone::Clone for EmptyByteFactory<B, Order> {
        #[inline]
        fn clone(&self) -> EmptyByteFactory<B, Order> {
            EmptyByteFactory {
                phantom: ::core::clone::Clone::clone(&self.phantom),
            }
        }
    }
    #[automatically_derived]
    impl<
        B: ::core::marker::Copy + FromBytes<Order> + InputMap,
        Order: ::core::marker::Copy + ByteOrder,
    > ::core::marker::Copy for EmptyByteFactory<B, Order> {}
    impl<B: FromBytes<Order> + InputMap, Order: ByteOrder> std::default::Default
    for EmptyByteFactory<B, Order> {
        fn default() -> Self {
            Self {
                phantom: Default::default(),
            }
        }
    }
    impl<B: FromBytes<Order> + InputMap, Order: ByteOrder> ByteFactory<Order>
    for EmptyByteFactory<B, Order> {
        type Error = <B as FromBytes<Order>>::Error;
        type Input = B;
        fn from_bytes(
            &mut self,
            data: &[u8],
        ) -> Result<(Self::Input, usize), ByteParsingError<Self::Error>>
        where
            Self: Sized,
        {
            B::from_bytes(data)
        }
    }
    impl<
        Source: NetworkSource,
        Order: ByteOrder,
        InputTime: TimeRepresentation,
        B: FromBytes<Order> + InputMap,
        const BUFFERSIZE: usize,
    > NetworkEventSource<
        Source,
        Order,
        EmptyByteFactory<B, Order>,
        InputTime,
        BUFFERSIZE,
    >
    where
        <B as InputMap>::Error: std::error::Error,
    {
        pub fn from_source(source: Source) -> Self {
            Self {
                factory: EmptyByteFactory::default(),
                source,
                buffer: Vec::new(),
                timer: PhantomData::default(),
            }
        }
    }
    impl<
        Source: NetworkSource,
        Order: ByteOrder,
        InputTime: TimeRepresentation,
        B: FromBytes<Order> + InputMap,
        const BUFFERSIZE: usize,
    > From<Source>
    for NetworkEventSource<
        Source,
        Order,
        EmptyByteFactory<B, Order>,
        InputTime,
        BUFFERSIZE,
    >
    where
        <B as InputMap>::Error: std::error::Error,
    {
        fn from(value: Source) -> Self {
            Self::from_source(value)
        }
    }
}
use std::error::Error;
use rtlola_interpreter::input::InputMap;
use rtlola_interpreter::time::TimeRepresentation;
type EventResult<MappedEvent, Time, Error> = Result<Option<(MappedEvent, Time)>, Error>;
/// The main trait that has to be implemented by an input plugin
pub trait EventSource<InputTime: TimeRepresentation> {
    type MappedEvent: InputMap;
    type Error: Error;
    /// Return the data needed by the monitor to initialize the input source.
    fn init_data(
        &self,
    ) -> Result<<Self::MappedEvent as InputMap>::CreationData, Self::Error>;
    /// Queries the event source for a new Record(Event) in a blocking fashion.
    /// If there are no more records, None is returned.
    fn next_event(
        &mut self,
    ) -> EventResult<Self::MappedEvent, InputTime::InnerTime, Self::Error>;
}
use std::time::Duration;
use byteorder::ByteOrder;
use rtlola_interpreter_macros::{CompositFactory, ValueFactory};
pub(crate) struct TestInputWithMacros {
    header: Header,
    d: Message,
}
#[automatically_derived]
impl ::core::fmt::Debug for TestInputWithMacros {
    #[inline]
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        ::core::fmt::Formatter::debug_struct_field2_finish(
            f,
            "TestInputWithMacros",
            "header",
            &self.header,
            "d",
            &&self.d,
        )
    }
}
#[automatically_derived]
impl ::core::clone::Clone for TestInputWithMacros {
    #[inline]
    fn clone(&self) -> TestInputWithMacros {
        TestInputWithMacros {
            header: ::core::clone::Clone::clone(&self.header),
            d: ::core::clone::Clone::clone(&self.d),
        }
    }
}
impl rtlola_interpreter::input::AssociatedFactory for TestInputWithMacros {
    type Factory = TestInputWithMacrosFactory;
}
pub(crate) struct TestInputWithMacrosFactory {
    header_factory: <Header as rtlola_interpreter::input::AssociatedFactory>::Factory,
    d_factory: <Message as rtlola_interpreter::input::AssociatedFactory>::Factory,
}
impl rtlola_interpreter::input::EventFactory for TestInputWithMacrosFactory {
    type Record = TestInputWithMacros;
    type Error = rtlola_interpreter::input::EventFactoryError;
    type CreationData = ();
    fn try_new(
        map: std::collections::HashMap<
            String,
            rtlola_interpreter::rtlola_frontend::mir::InputReference,
        >,
        setup_data: Self::CreationData,
    ) -> Result<(Self, Vec<String>), Self::Error> {
        let mut all_found = std::collections::HashSet::with_capacity(map.len());
        let (header_factory, found) = <Header as rtlola_interpreter::input::AssociatedFactory>::Factory::try_new(
            map.clone(),
            (),
        )?;
        all_found.extend(found);
        let (d_factory, found) = <Message as rtlola_interpreter::input::AssociatedFactory>::Factory::try_new(
            map.clone(),
            (),
        )?;
        all_found.extend(found);
        Ok((
            TestInputWithMacrosFactory {
                header_factory,
                d_factory,
            },
            all_found.into_iter().collect(),
        ))
    }
    fn get_event(
        &self,
        rec: Self::Record,
    ) -> Result<rtlola_interpreter::monitor::Event, Self::Error> {
        let TestInputWithMacros { header, d, .. } = rec;
        let header_event = self.header_factory.get_event(header)?;
        let d_event = self.d_factory.get_event(d)?;
        Ok(
            ::itertools::__std_iter::IntoIterator::into_iter(header_event)
                .zip(d_event)
                .map(|(header, d)| {
                    rtlola_interpreter::Value::None.and_then(header).and_then(d)
                })
                .collect(),
        )
    }
}
pub(crate) struct Header {
    timestamp: f64,
    a: f64,
}
#[automatically_derived]
impl ::core::fmt::Debug for Header {
    #[inline]
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        ::core::fmt::Formatter::debug_struct_field2_finish(
            f,
            "Header",
            "timestamp",
            &self.timestamp,
            "a",
            &&self.a,
        )
    }
}
#[automatically_derived]
impl ::core::clone::Clone for Header {
    #[inline]
    fn clone(&self) -> Header {
        Header {
            timestamp: ::core::clone::Clone::clone(&self.timestamp),
            a: ::core::clone::Clone::clone(&self.a),
        }
    }
}
impl rtlola_interpreter::input::InputMap for Header {
    type CreationData = ();
    type Error = rtlola_interpreter::input::EventFactoryError;
    fn func_for_input(
        name: &str,
        data: Self::CreationData,
    ) -> Result<rtlola_interpreter::input::ValueGetter<Self, Self::Error>, Self::Error> {
        match name {
            "timestamp" => Ok(Box::new(|data| Ok(data.timestamp.clone().try_into()?))),
            "a" => Ok(Box::new(|data| Ok(data.a.clone().try_into()?))),
            _ => {
                Err(
                    rtlola_interpreter::input::EventFactoryError::InputStreamUnknown(
                        <[_]>::into_vec(
                            #[rustc_box]
                            ::alloc::boxed::Box::new([name.to_string()]),
                        ),
                    ),
                )
            }
        }
    }
}
impl TestInputWithMacros {
    pub(crate) fn new(header: Header, d: Message) -> Self {
        Self { header, d }
    }
}
pub(crate) enum Message {
    M0(Message0),
    M1(Message1),
}
#[automatically_derived]
impl ::core::fmt::Debug for Message {
    #[inline]
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        match self {
            Message::M0(__self_0) => {
                ::core::fmt::Formatter::debug_tuple_field1_finish(f, "M0", &__self_0)
            }
            Message::M1(__self_0) => {
                ::core::fmt::Formatter::debug_tuple_field1_finish(f, "M1", &__self_0)
            }
        }
    }
}
#[automatically_derived]
impl ::core::clone::Clone for Message {
    #[inline]
    fn clone(&self) -> Message {
        match self {
            Message::M0(__self_0) => Message::M0(::core::clone::Clone::clone(__self_0)),
            Message::M1(__self_0) => Message::M1(::core::clone::Clone::clone(__self_0)),
        }
    }
}
impl rtlola_interpreter::input::AssociatedFactory for Message {
    type Factory = MessageFactory;
}
pub(crate) struct MessageFactory {
    m_0_factory: <Message0 as rtlola_interpreter::input::AssociatedFactory>::Factory,
    m_1_factory: <Message1 as rtlola_interpreter::input::AssociatedFactory>::Factory,
}
impl rtlola_interpreter::input::EventFactory for MessageFactory {
    type Record = Message;
    type Error = rtlola_interpreter::input::EventFactoryError;
    type CreationData = ();
    fn try_new(
        map: std::collections::HashMap<
            String,
            rtlola_interpreter::rtlola_frontend::mir::InputReference,
        >,
        setup_data: Self::CreationData,
    ) -> Result<(Self, Vec<String>), Self::Error> {
        let mut all_found = std::collections::HashSet::with_capacity(map.len());
        let (m_0_factory, found) = <Message0 as rtlola_interpreter::input::AssociatedFactory>::Factory::try_new(
            map.clone(),
            (),
        )?;
        all_found.extend(found);
        let (m_1_factory, found) = <Message1 as rtlola_interpreter::input::AssociatedFactory>::Factory::try_new(
            map.clone(),
            (),
        )?;
        all_found.extend(found);
        Ok((
            MessageFactory {
                m_0_factory,
                m_1_factory,
            },
            all_found.into_iter().collect(),
        ))
    }
    fn get_event(
        &self,
        rec: Self::Record,
    ) -> Result<rtlola_interpreter::monitor::Event, Self::Error> {
        match rec {
            Message::M0(rec) => Ok(self.m_0_factory.get_event(rec)?),
            Message::M1(rec) => Ok(self.m_1_factory.get_event(rec)?),
        }
    }
}
pub(crate) struct Message0 {
    b: f64,
}
#[automatically_derived]
impl ::core::fmt::Debug for Message0 {
    #[inline]
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        ::core::fmt::Formatter::debug_struct_field1_finish(f, "Message0", "b", &&self.b)
    }
}
#[automatically_derived]
impl ::core::clone::Clone for Message0 {
    #[inline]
    fn clone(&self) -> Message0 {
        Message0 {
            b: ::core::clone::Clone::clone(&self.b),
        }
    }
}
impl rtlola_interpreter::input::InputMap for Message0 {
    type CreationData = ();
    type Error = rtlola_interpreter::input::EventFactoryError;
    fn func_for_input(
        name: &str,
        data: Self::CreationData,
    ) -> Result<rtlola_interpreter::input::ValueGetter<Self, Self::Error>, Self::Error> {
        match name {
            "b" => Ok(Box::new(|data| Ok(data.b.clone().try_into()?))),
            _ => {
                Err(
                    rtlola_interpreter::input::EventFactoryError::InputStreamUnknown(
                        <[_]>::into_vec(
                            #[rustc_box]
                            ::alloc::boxed::Box::new([name.to_string()]),
                        ),
                    ),
                )
            }
        }
    }
}
pub(crate) struct Message1 {
    c: u64,
}
#[automatically_derived]
impl ::core::fmt::Debug for Message1 {
    #[inline]
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        ::core::fmt::Formatter::debug_struct_field1_finish(f, "Message1", "c", &&self.c)
    }
}
#[automatically_derived]
impl ::core::clone::Clone for Message1 {
    #[inline]
    fn clone(&self) -> Message1 {
        Message1 {
            c: ::core::clone::Clone::clone(&self.c),
        }
    }
}
impl rtlola_interpreter::input::InputMap for Message1 {
    type CreationData = ();
    type Error = rtlola_interpreter::input::EventFactoryError;
    fn func_for_input(
        name: &str,
        data: Self::CreationData,
    ) -> Result<rtlola_interpreter::input::ValueGetter<Self, Self::Error>, Self::Error> {
        match name {
            "c" => Ok(Box::new(|data| Ok(data.c.clone().try_into()?))),
            _ => {
                Err(
                    rtlola_interpreter::input::EventFactoryError::InputStreamUnknown(
                        <[_]>::into_vec(
                            #[rustc_box]
                            ::alloc::boxed::Box::new([name.to_string()]),
                        ),
                    ),
                )
            }
        }
    }
}
