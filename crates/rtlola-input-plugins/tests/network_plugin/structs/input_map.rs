use std::io::ErrorKind;
use std::time::Duration;

use byteorder::ByteOrder;
use rtlola_input_plugins::network_plugin::time_converter::TimeConverter;
use rtlola_input_plugins::network_plugin::{ByteParsingError, FromBytes};
use rtlola_interpreter::config::OfflineMode;
use rtlola_interpreter::input::{EventFactoryError, InputMap, MappedFactory};
use rtlola_interpreter::monitor::{Change, Incremental, Verdicts};
use rtlola_interpreter::time::{AbsoluteFloat, TimeRepresentation};
use rtlola_interpreter::{ConfigBuilder, Monitor, Value};
use serde::{Deserialize, Serialize};

use super::SPEC;
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct TestInput {
    timestamp: Duration,
    a: f64,
    d: Message,
}

impl TestInput {
    pub(crate) fn new(ts: Duration, a: f64, d: Message) -> Self {
        Self { timestamp: ts, a, d }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) enum Message {
    M0 { b: f64 },
    M1 { c: u64 },
}

impl InputMap for TestInput {
    type CreationData = ();
    type Error = EventFactoryError;

    fn func_for_input(
        name: &str,
        _data: Self::CreationData,
    ) -> Result<rtlola_interpreter::ValueGetter<Self, Self::Error>, Self::Error> {
        match name {
            "a" => Ok(Box::new(|data: &Self| Ok(data.a.try_into()?))),
            "b" => {
                Ok(Box::new(|data: &Self| {
                    match data.d {
                        Message::M0 { b } => Ok(b.try_into()?),
                        _ => Ok(Value::None),
                    }
                }))
            },
            "c" => {
                Ok(Box::new(|data: &Self| {
                    match data.d {
                        Message::M1 { c } => Ok(c.into()),
                        _ => Ok(Value::None),
                    }
                }))
            },
            _ => unimplemented!(),
        }
    }
}

impl<B: ByteOrder> FromBytes<B> for TestInput {
    type Error = <Self as InputMap>::Error;

    fn from_bytes(data: &[u8]) -> Result<(Self, usize), ByteParsingError<<Self as InputMap>::Error>>
    where
        Self: Sized,
    {
        let res: TestInput = bincode::deserialize(&data).map_err(|e| {
            if matches!(ErrorKind::UnexpectedEof, e) {
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

impl TimeConverter<AbsoluteFloat> for TestInput {
    fn convert_time(&self) -> Result<<AbsoluteFloat as TimeRepresentation>::InnerTime, <Self as InputMap>::Error> {
        Ok(self.timestamp)
    }
}

pub(crate) fn create_monitor(
) -> Monitor<MappedFactory<TestInput>, OfflineMode<AbsoluteFloat>, Incremental, AbsoluteFloat> {
    let cfg = ConfigBuilder::new()
        .spec_str(SPEC)
        .offline::<AbsoluteFloat>()
        .with_mapped_events::<TestInput>()
        .with_verdict::<Incremental>()
        .output_time::<AbsoluteFloat>()
        .build();
    cfg.monitor().unwrap()
}

pub(crate) fn create_events() -> Vec<TestInput> {
    let r0 = TestInput::new(Duration::from_secs(1), 5.0, Message::M0 { b: 5.0 });
    let r1 = TestInput::new(Duration::from_secs(2), 2.0, Message::M1 { c: 2 });
    vec![r0, r1]
}
