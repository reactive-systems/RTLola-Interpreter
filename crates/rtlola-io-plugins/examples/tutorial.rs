use std::error::Error;
use std::net::{TcpStream, UdpSocket};

use rtlola_interpreter::monitor::TriggerMessages;
use rtlola_interpreter::time::{AbsoluteFloat, RealTime, TimeRepresentation};
use rtlola_interpreter::ConfigBuilder;
use rtlola_interpreter_macros::{CompositFactory, ValueFactory};
use rtlola_io_plugins::byte_plugin::upd::UdpReader;
use rtlola_io_plugins::byte_plugin::{ByteEventSource, ByteVerdictSink, SerdeByteSerializer};
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

const EVENTSOURCEADDR: &str = "127.0.0.1:2000";
const VERDICTSINKADDR: &str = "127.0.0.1:2001";
const SPEC: &str = "";
fn main() -> Result<(), Box<dyn Error + 'static>> {
    let mut event_source =
        ByteEventSource::<UdpReader, _, RealTime, 128>::from_source(UdpSocket::bind(EVENTSOURCEADDR)?.into());

    let mut verdict_sink: ByteVerdictSink<_, _, VerdictRepresentationFactory<_, _>, SerdeByteSerializer<_>, _> =
        ByteVerdictSink::from_target(TcpStream::connect(VERDICTSINKADDR)?);

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
