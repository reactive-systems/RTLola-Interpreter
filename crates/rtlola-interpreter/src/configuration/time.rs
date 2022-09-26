//! This module contains the different time representations of the interpreter.
//!
//! ## Time Representations
//! The RTLola interpreter supports multiple representations of time in its input and output.
//! If run in offline mode, meaning the time for an event is parsed from the input source,
//! the format in which the time is present in the input has to be set. Consider the following example CSV file:
//!
//! <pre>
//! a,b,time
//! 0,1,0.1
//! 2,3,0.2
//! 4,5,0.3
//! </pre>
//!
//! The supported time representations are:
//!
//! #### Relative
//! Time is considered relative to a fixed point in time. Call this point in time `x` then in the example above
//! the first event gets the timestamp `x + 0.1`, the second one `x + 0.2` and so forth.
//!
//! #### Incremental
//! Time is considered relative to the preceding event. This induces the following timestamps for the above example:
//!
//! <pre>
//! a,b, time
//! 0,1, x + 0.1
//! 2,3, x + 0.3
//! 4,5, x + 0.6
//! </pre>
//!
//! #### Absolute
//! Time is parsed as absolute timestamps.
//!
//! **Note**: The evaluation of periodic streams depends on the time passed between events.
//! Depending on the representation, determining the time that passed before the first event is not obvious.
//! While the relative and incremental representations do not strictly need a point of reference to determine
//! the time passed, the absolute representation requires such a point of reference.
//! This point of time can either be directly supplied during configuration using the [start_time](crate::ConfigBuilder::start_time) method
//! or inferred as the time of the first event.
//! The latter consequently assumes that no time has passed before the first event in the input.

use std::fmt::Debug;
use std::ops::Sub;
use std::str::FromStr;
use std::sync::RwLock;
use std::time::{Duration, SystemTime};

use humantime::Rfc3339Timestamp;
use lazy_static::lazy_static;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use crate::Time;

const NANOS_IN_SECOND: u64 = 1_000_000_000;

lazy_static! {
    /// Used to synchronise the start time across the program
    static ref START_TIME: RwLock<Option<SystemTime>> = RwLock::new(None);
}

pub(crate) fn init_start_time<T: TimeRepresentation>(start_time: Option<SystemTime>) {
    *START_TIME.write().unwrap() = start_time.or_else(T::default_start_time);
}

/// Precisely parses an duration from a string of the form '{secs}.{sub-secs}'
pub fn parse_float_time(s: &str) -> Result<Duration, String> {
    let num = Decimal::from_str(s).map_err(|e| e.to_string())?;
    let nanos = (num.fract() * Decimal::from(NANOS_IN_SECOND))
        .to_u32()
        .ok_or("Could not convert nano seconds")?;
    let secs = num.trunc().to_u64().ok_or("Could not convert seconds")?;
    Ok(Duration::new(secs, nanos))
}

/// The functionality a time format has to provide.
pub trait TimeRepresentation: TimeMode + Default + Clone + Send + 'static {
    /// The internal representation of the time format.
    type InnerTime: Debug + Clone + Clone + Send;

    /// Convert from the internal time representation to the monitor time.
    fn convert_from(&mut self, inner: Self::InnerTime) -> Time;
    /// Convert from monitor time to the internal representation.
    fn convert_into(&self, ts: Time) -> Self::InnerTime;

    /// Convert the internal representation into a string.
    fn to_string(&self, ts: Self::InnerTime) -> String;
    /// Parse the internal representation from a string and convert it into monitor time.
    fn parse(s: &'_ str) -> Result<Self::InnerTime, String>;

    /// Returns a default start time if applicable for the time representation.
    fn default_start_time() -> Option<SystemTime> {
        Some(SystemTime::now())
    }
}

/// This trait captures whether the time is given explicitly through a timestamp or is indirectly obtained through measurements.
pub trait TimeMode {
    /// Returns whether the time [TimeRepresentation] require an explicit timestamp
    fn requires_timestamp() -> bool {
        true
    }
}

/// This trait captures the subset [TimeRepresentation]s suitable for output.
pub trait OutputTimeRepresentation: TimeRepresentation {}

/// Time represented as the unsigned number of nanoseconds relative to a fixed start time.
#[derive(Debug, Copy, Clone, Default)]
pub struct RelativeNanos {}

impl TimeRepresentation for RelativeNanos {
    type InnerTime = u64;

    fn convert_from(&mut self, nanos: Self::InnerTime) -> Time {
        Duration::from_nanos(nanos)
    }

    fn convert_into(&self, ts: Time) -> Self::InnerTime {
        ts.as_nanos() as u64
    }

    fn to_string(&self, ts: Self::InnerTime) -> String {
        ts.to_string()
    }

    fn parse(s: &'_ str) -> Result<u64, String> {
        u64::from_str(s).map_err(|e| e.to_string())
    }
}
impl OutputTimeRepresentation for RelativeNanos {}
impl TimeMode for RelativeNanos {}

/// Time represented as a positive real number representing seconds and sub-seconds relative to a fixed start time.
/// ie. 5.2
#[derive(Debug, Copy, Clone, Default)]
pub struct RelativeFloat {}

impl TimeRepresentation for RelativeFloat {
    type InnerTime = Duration;

    fn convert_from(&mut self, ts: Self::InnerTime) -> Time {
        ts
    }

    fn convert_into(&self, ts: Time) -> Self::InnerTime {
        ts
    }

    fn to_string(&self, ts: Self::InnerTime) -> String {
        let dur = self.convert_into(ts);
        format! {"{}.{:09}", dur.as_secs(), dur.subsec_nanos()}
    }

    fn parse(s: &str) -> Result<Duration, String> {
        parse_float_time(s)
    }
}
impl OutputTimeRepresentation for RelativeFloat {}
impl TimeMode for RelativeFloat {}

/// Time represented as the unsigned number in nanoseconds as the offset to the preceding event.
#[derive(Debug, Copy, Clone, Default)]
pub struct OffsetNanos {
    current: Time,
    last_time: Time,
}

impl TimeRepresentation for OffsetNanos {
    type InnerTime = u64;

    fn convert_from(&mut self, raw: Self::InnerTime) -> Time {
        self.last_time = self.current;
        self.current += Duration::from_nanos(raw);
        self.current
    }

    fn convert_into(&self, ts: Time) -> Self::InnerTime {
        ts.sub(self.last_time).as_nanos() as u64
    }

    fn to_string(&self, ts: Self::InnerTime) -> String {
        ts.to_string()
    }

    fn parse(s: &'_ str) -> Result<u64, String> {
        u64::from_str(s).map_err(|e| e.to_string())
    }
}
impl TimeMode for OffsetNanos {}

/// Time represented as a positive real number representing seconds and sub-seconds as the offset to the preceding event.
#[derive(Debug, Copy, Clone, Default)]
pub struct OffsetFloat {
    current: Time,
    last_time: Time,
}

impl TimeRepresentation for OffsetFloat {
    type InnerTime = Duration;

    fn convert_from(&mut self, ts: Duration) -> Time {
        self.last_time = self.current;
        self.current += ts;
        self.current
    }

    fn convert_into(&self, ts: Time) -> Self::InnerTime {
        ts - self.last_time
    }

    fn to_string(&self, ts: Time) -> String {
        let dur = self.convert_into(ts);
        format! {"{}.{:09}", dur.as_secs(), dur.subsec_nanos()}
    }

    fn parse(s: &str) -> Result<Duration, String> {
        parse_float_time(s)
    }
}

impl TimeMode for OffsetFloat {}

/// Time represented as wall clock time given as a positive real number representing seconds and sub-seconds since the start of the Unix Epoch.
#[derive(Debug, Copy, Clone, Default)]
pub struct AbsoluteFloat {}

impl TimeRepresentation for AbsoluteFloat {
    type InnerTime = Duration;

    fn convert_from(&mut self, ts: Duration) -> Time {
        let current = SystemTime::UNIX_EPOCH + ts;
        let st_read = *START_TIME.read().unwrap();
        if let Some(st) = st_read {
            current.duration_since(st).expect("Time did not behave monotonically!")
        } else {
            *START_TIME.write().unwrap() = Some(current);
            Duration::ZERO
        }
    }

    fn convert_into(&self, ts: Time) -> Self::InnerTime {
        let ts = START_TIME.read().unwrap().unwrap() + ts;
        ts.duration_since(SystemTime::UNIX_EPOCH)
            .expect("Time did not behave monotonically!")
    }

    fn to_string(&self, ts: Time) -> String {
        let dur = self.convert_into(ts);
        format! {"{}.{:09}", dur.as_secs(), dur.subsec_nanos()}
    }

    fn parse(s: &str) -> Result<Duration, String> {
        parse_float_time(s)
    }

    fn default_start_time() -> Option<SystemTime> {
        None
    }
}
impl OutputTimeRepresentation for AbsoluteFloat {}
impl TimeMode for AbsoluteFloat {}

/// Time represented as wall clock time in RFC3339 format.
#[derive(Debug, Copy, Clone, Default)]
pub struct AbsoluteRfc {}

impl TimeRepresentation for AbsoluteRfc {
    type InnerTime = Rfc3339Timestamp;

    fn convert_from(&mut self, rfc: Self::InnerTime) -> Time {
        let current = rfc.get_ref();
        let st_read = *START_TIME.read().unwrap();
        if let Some(st) = st_read {
            current.duration_since(st).expect("Time did not behave monotonically!")
        } else {
            *START_TIME.write().unwrap() = Some(*current);
            Duration::ZERO
        }
    }

    fn convert_into(&self, ts: Time) -> Self::InnerTime {
        let ts = START_TIME.read().unwrap().unwrap() + ts;
        humantime::format_rfc3339(ts)
    }

    fn to_string(&self, ts: Self::InnerTime) -> String {
        ts.to_string()
    }

    fn parse(s: &'_ str) -> Result<Self::InnerTime, String> {
        let ts = humantime::parse_rfc3339(s).map_err(|e| e.to_string())?;
        Ok(humantime::format_rfc3339(ts))
    }

    fn default_start_time() -> Option<SystemTime> {
        None
    }
}
impl OutputTimeRepresentation for AbsoluteRfc {}
impl TimeMode for AbsoluteRfc {}

/// Time is set to be a fixed delay between input events.
/// The time given is ignored, and the fixed delay is applied.
#[derive(Debug, Clone, Copy, Default)]
pub struct DelayTime {
    current: Duration,
    delay: Duration,
}

impl DelayTime {
    /// Creates a new DelayTime with a given delay.
    pub fn new(delay: Duration) -> Self {
        DelayTime {
            current: Default::default(),
            delay,
        }
    }
}

impl TimeRepresentation for DelayTime {
    type InnerTime = ();

    fn convert_from(&mut self, _inner: Self::InnerTime) -> Time {
        self.current += self.delay;
        self.current
    }

    fn convert_into(&self, _ts: Time) -> Self::InnerTime {}

    fn parse(_s: &str) -> Result<(), String> {
        Ok(())
    }

    fn to_string(&self, _ts: Self::InnerTime) -> String {
        format! {"{}.{:09}", self.current.as_secs(), self.current.subsec_nanos()}
    }
}

impl TimeMode for DelayTime {
    fn requires_timestamp() -> bool {
        false
    }
}

/// Time is set to be real-time. I.e. the input time is ignored and the current timestamp in rfc3339 format is taken instead.
#[derive(Debug, Copy, Clone, Default)]
pub struct RealTime {
    last_ts: Time,
}
impl TimeRepresentation for RealTime {
    type InnerTime = ();

    fn convert_from(&mut self, _inner: Self::InnerTime) -> Time {
        let current = SystemTime::now();
        let st_read = *START_TIME.read().unwrap();
        self.last_ts = if let Some(st) = st_read {
            current.duration_since(st).expect("Time did not behave monotonically!")
        } else {
            *START_TIME.write().unwrap() = Some(current);
            Duration::ZERO
        };
        self.last_ts
    }

    fn convert_into(&self, _ts: Time) -> Self::InnerTime {}

    fn to_string(&self, _ts: Self::InnerTime) -> String {
        let ts = START_TIME.read().unwrap().unwrap() + self.last_ts;
        humantime::format_rfc3339(ts).to_string()
    }

    fn parse(_s: &str) -> Result<(), String> {
        Ok(())
    }
}

impl TimeMode for RealTime {
    fn requires_timestamp() -> bool {
        false
    }
}
