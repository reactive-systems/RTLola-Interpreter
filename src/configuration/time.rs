use crate::Time;
use humantime::Rfc3339Timestamp;
use lazy_static::lazy_static;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::fmt::Debug;
use std::ops::Sub;
use std::str::FromStr;
use std::sync::RwLock;
use std::time::{Duration, SystemTime};

const NANOS_IN_SECOND: u64 = 1_000_000_000;

lazy_static! {
    /// Used to synchronise the start time across the program
    static ref START_TIME: RwLock<Option<SystemTime>> = RwLock::new(None);
}

pub(crate) fn init_start_time<T: TimeRepresentation>(start_time: Option<SystemTime>) {
    *START_TIME.write().unwrap() = start_time.or(T::default_start_time());
}

pub fn parse_float_time(s: &str) -> Result<Duration, String> {
    let num = Decimal::from_str(s).map_err(|e| e.to_string())?;
    let nanos = (num.fract() * Decimal::from(NANOS_IN_SECOND)).to_u32().ok_or("Could not convert nano seconds")?;
    let secs = num.trunc().to_u64().ok_or("Could not convert seconds")?;
    Ok(Duration::new(secs, nanos))
}

pub trait TimeRepresentation: Default + Clone + Send + Sync + 'static {
    type InnerTime: Clone;

    fn convert_from(&mut self, inner: Self::InnerTime) -> Time;
    fn convert_into(&mut self, ts: Time) -> Self::InnerTime;

    fn convert_into_string(&mut self, ts: Time) -> String;
    fn parse(&mut self, s: &'_ str) -> Result<Time, String>;

    fn default_start_time() -> Option<SystemTime> {
        Some(SystemTime::now())
    }
}

pub trait FromFloat {
    fn parse(s: &str) -> Result<(u64, u32), String>;
}

#[derive(Debug, Copy, Clone, Default)]
pub struct RelativeNanos {}

impl TimeRepresentation for RelativeNanos {
    type InnerTime = u64;

    fn convert_from(&mut self, nanos: Self::InnerTime) -> Time {
        Duration::from_nanos(nanos)
    }

    fn convert_into(&mut self, ts: Time) -> Self::InnerTime {
        ts.as_nanos() as u64
    }

    fn convert_into_string(&mut self, ts: Time) -> String {
        self.convert_into(ts).to_string()
    }

    fn parse(&mut self, s: &'_ str) -> Result<Time, String> {
        u64::from_str(s).map(|n| self.convert_from(n)).map_err(|e| e.to_string())
    }
}

#[derive(Debug, Copy, Clone, Default)]
pub struct RelativeFloat {}

impl TimeRepresentation for RelativeFloat {
    type InnerTime = Duration;

    fn convert_from(&mut self, ts: Self::InnerTime) -> Time {
        ts
    }

    fn convert_into(&mut self, ts: Time) -> Self::InnerTime {
        ts
    }

    fn convert_into_string(&mut self, ts: Time) -> String {
        let dur = self.convert_into(ts);
        let (secs, sub_secs) = (dur.as_secs(), dur.subsec_nanos());
        let decimal = Decimal::from(secs) + Decimal::new(sub_secs as i64, 9);
        decimal.round_dp(9).to_string()
    }

    fn parse(&mut self, s: &str) -> Result<Time, String> {
        parse_float_time(s)
    }
}

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

    fn convert_into(&mut self, ts: Time) -> Self::InnerTime {
        ts.sub(self.last_time).as_nanos() as u64
    }

    fn convert_into_string(&mut self, ts: Time) -> String {
        self.convert_into(ts).to_string()
    }

    fn parse(&mut self, s: &'_ str) -> Result<Time, String> {
        u64::from_str(s).map(|n| self.convert_from(n)).map_err(|e| e.to_string())
    }
}

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

    fn convert_into(&mut self, ts: Time) -> Self::InnerTime {
        let dur = ts - self.last_time;
        dur
    }

    fn convert_into_string(&mut self, ts: Time) -> String {
        let dur = self.convert_into(ts);
        let (secs, sub_secs) = (dur.as_secs(), dur.subsec_nanos());
        let decimal = Decimal::from(secs) + Decimal::new(sub_secs as i64, 9);
        decimal.round_dp(9).to_string()
    }

    fn parse(&mut self, s: &str) -> Result<Time, String> {
        let dur = parse_float_time(s)?;
        Ok(self.convert_from(dur))
    }
}

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

    fn convert_into(&mut self, ts: Time) -> Self::InnerTime {
        let ts = START_TIME.read().unwrap().unwrap() + ts;
        ts.duration_since(SystemTime::UNIX_EPOCH).expect("Time did not behave monotonically!")
    }

    fn convert_into_string(&mut self, ts: Time) -> String {
        let dur = self.convert_into(ts);
        let (secs, sub_secs) = (dur.as_secs(), dur.subsec_nanos());
        let decimal = Decimal::from(secs) + Decimal::new(sub_secs as i64, 9);
        decimal.round_dp(9).to_string()
    }

    fn parse(&mut self, s: &str) -> Result<Time, String> {
        let dur = parse_float_time(s)?;
        Ok(self.convert_from(dur))
    }

    fn default_start_time() -> Option<SystemTime> {
        None
    }
}

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

    fn convert_into(&mut self, ts: Time) -> Self::InnerTime {
        let ts = START_TIME.read().unwrap().unwrap() + ts;
        humantime::format_rfc3339(ts)
    }

    fn convert_into_string(&mut self, ts: Time) -> String {
        self.convert_into(ts).to_string()
    }

    fn parse(&mut self, s: &'_ str) -> Result<Time, String> {
        let ts = humantime::parse_rfc3339(s).map_err(|e| e.to_string())?;
        let rfc = humantime::format_rfc3339(ts);
        Ok(self.convert_from(rfc))
    }

    fn default_start_time() -> Option<SystemTime> {
        None
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DelayTime {
    current: Duration,
    delay: Duration,
}

impl DelayTime {
    pub fn new(delay: Duration) -> Self {
        DelayTime { current: Default::default(), delay }
    }
}

impl TimeRepresentation for DelayTime {
    type InnerTime = ();

    fn convert_from(&mut self, _inner: Self::InnerTime) -> Time {
        self.current += self.delay;
        self.current
    }

    fn convert_into(&mut self, _ts: Time) -> Self::InnerTime {
        ()
    }

    fn parse(&mut self, _s: &str) -> Result<Time, String> {
        Ok(self.convert_from(()))
    }

    fn convert_into_string(&mut self, ts: Time) -> String {
        let (secs, sub_secs) = (ts.as_secs(), ts.subsec_nanos());
        let decimal = Decimal::from(secs) + Decimal::new(sub_secs as i64, 9);
        decimal.round_dp(9).to_string()
    }
}

#[derive(Debug, Copy, Clone, Default)]
pub struct RealTime {}
impl TimeRepresentation for RealTime {
    type InnerTime = ();

    fn convert_from(&mut self, _inner: Self::InnerTime) -> Time {
        let current = SystemTime::now();
        let st_read = *START_TIME.read().unwrap();
        if let Some(st) = st_read {
            current.duration_since(st).expect("Time did not behave monotonically!")
        } else {
            *START_TIME.write().unwrap() = Some(current);
            Duration::ZERO
        }
    }

    fn convert_into(&mut self, _ts: Time) -> Self::InnerTime {
        ()
    }

    fn convert_into_string(&mut self, ts: Time) -> String {
        let ts = START_TIME.read().unwrap().unwrap() + ts;
        humantime::format_rfc3339(ts).to_string()
    }

    fn parse(&mut self, _s: &str) -> Result<Time, String> {
        Ok(self.convert_from(()))
    }
}
