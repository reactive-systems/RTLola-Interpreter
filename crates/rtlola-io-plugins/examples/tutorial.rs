use std::error::Error;
use std::io::ErrorKind;
use std::net::{TcpStream, UdpSocket};

use byteorder::{ByteOrder, LittleEndian};
use rtlola_interpreter::input::{AssociatedFactory, EventFactory, EventFactoryError};
use rtlola_interpreter::monitor::TriggerMessages;
use rtlola_interpreter::time::{AbsoluteFloat, RealTime};
use rtlola_interpreter::ConfigBuilder;
use rtlola_interpreter_macros::{CompositFactory, ValueFactory};
use rtlola_io_plugins::byte_plugin::upd::UdpWrapper;
use rtlola_io_plugins::byte_plugin::{
    ByteEventSource, ByteParsingError, ByteVerdictSink, FromBytes, StatelessVerdictByteFactory,
};
use rtlola_io_plugins::{EventSource, VerdictRepresentationFactory, VerdictsSink};
use serde::{Deserialize, Serialize};

#[derive(ValueFactory, Serialize, Deserialize)]
struct Gnss {
    lat: f64,
    lon: f64,
}

#[derive(ValueFactory, Serialize, Deserialize)]
#[factory(prefix)]
struct Intruder {
    id: u64,
    lat: f64,
    lon: f64,
}

#[derive(CompositFactory, Serialize, Deserialize)]
enum ExampleInputs {
    Gnss(Gnss),
    Intruder(Intruder),
}

impl<B: ByteOrder> FromBytes<B> for ExampleInputs {
    type Error = <<Self as AssociatedFactory>::Factory as EventFactory>::Error;

    fn from_bytes(data: &[u8]) -> Result<(Self, usize), rtlola_io_plugins::byte_plugin::ByteParsingError<Self::Error>>
    where
        Self: Sized,
    {
        let res: ExampleInputs = bincode::deserialize(&data).map_err(|e| {
            if matches!(ErrorKind::UnexpectedEof, _e) {
                ByteParsingError::Incomplete
            } else {
                ByteParsingError::Inner(EventFactoryError::Other(e))
            }
        })?;
        let size =
            bincode::serialized_size(&res).map_err(|e| ByteParsingError::Inner(EventFactoryError::Other(e)))? as usize;
        Ok((res, size))
    }
}

const EVENTSOURCEADDR: &str = "127.0.0.1:2000";
const VERDICTSINKADDR: &str = "127.0.0.1:2001";
const SPEC: &str = "";
fn main() -> Result<(), Box<dyn Error + 'static>> {
    let mut event_source = ByteEventSource::<UdpWrapper, LittleEndian, _, RealTime, 128>::from_source(
        UdpSocket::bind(EVENTSOURCEADDR)?.into(),
    );

    let verdict_factory = VerdictRepresentationFactory::<TriggerMessages, AbsoluteFloat>::default();
    let verdict_factory: StatelessVerdictByteFactory<LittleEndian, _, _, _> =
        StatelessVerdictByteFactory::new(verdict_factory);

    let mut verdict_sink = ByteVerdictSink::<_, _, _, _, _>::new(TcpStream::connect(VERDICTSINKADDR)?, verdict_factory);

    let mut monitor = ConfigBuilder::new()
        .spec_str(SPEC)
        .online()
        .with_event_factory::<ExampleInputsFactory>()
        .with_verdict::<TriggerMessages>()
        .output_time::<AbsoluteFloat>()
        .monitor()?;

    while let Some((ev, ts)) = event_source.next_event()? {
        let verdicts = monitor.accept_event(ev, ts)?;
        verdict_sink.sink_verdicts(verdicts)?;
    }

    Ok(())
}
