use std::time::Duration;

use rtlola_interpreter::config::OfflineMode;
use rtlola_interpreter::input::{AssociatedFactory, EventFactory};
use rtlola_interpreter::monitor::Incremental;
use rtlola_interpreter::time::{AbsoluteFloat, TimeRepresentation};
use rtlola_interpreter::{ConfigBuilder, Monitor};
use rtlola_interpreter_macros::{CompositFactory, ValueFactory};
use rtlola_io_plugins::byte_plugin::time_converter::TimeConverter;
use serde::{Deserialize, Serialize};

use super::SPEC;

#[derive(Debug, Clone, Deserialize, Serialize, CompositFactory)]
pub(crate) struct TestInputWithMacros {
    header: Header,
    d: Message,
}

impl TestInputWithMacros {
    pub(crate) fn new(ts: f64, a: f64, d: Message) -> Self {
        Self {
            header: Header { timestamp: ts, a },
            d,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, ValueFactory)]
pub(crate) struct Header {
    timestamp: f64,
    a: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, CompositFactory)]
pub(crate) enum Message {
    M0(Message0),
    M1(Message1),
}

#[derive(Debug, Clone, Deserialize, Serialize, ValueFactory)]
pub(crate) struct Message0 {
    b: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, ValueFactory)]
pub(crate) struct Message1 {
    c: u64,
}

impl TimeConverter<AbsoluteFloat> for TestInputWithMacros {
    fn convert_time(
        &self,
    ) -> Result<
        <AbsoluteFloat as TimeRepresentation>::InnerTime,
        <<Self as AssociatedFactory>::Factory as EventFactory>::Error,
    > {
        Ok(Duration::from_secs_f64(self.header.timestamp))
    }
}

pub(crate) fn create_monitor(
) -> Monitor<TestInputWithMacrosFactory, OfflineMode<AbsoluteFloat>, Incremental, AbsoluteFloat> {
    let cfg = ConfigBuilder::new()
        .spec_str(SPEC)
        .offline::<AbsoluteFloat>()
        .with_event_factory::<<TestInputWithMacros as AssociatedFactory>::Factory>()
        .with_verdict::<Incremental>()
        .output_time::<AbsoluteFloat>()
        .build();
    cfg.monitor().unwrap()
}

pub(crate) fn create_events() -> Vec<TestInputWithMacros> {
    let r0 = TestInputWithMacros::new(1.0, 5.0, Message::M0(Message0 { b: 5.0 }));
    let r1 = TestInputWithMacros::new(2.0, 2.0, Message::M1(Message1 { c: 2 }));
    vec![r0, r1]
}
