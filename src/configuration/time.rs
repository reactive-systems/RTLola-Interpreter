use crate::Time;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::convert::TryFrom;
use std::fmt::Display;
use std::ops::Sub;
use std::str::FromStr;
use std::time::{Duration, SystemTime};

const NANOS_IN_SECOND: Decimal = Decimal::from(1_000_000_000);

pub trait TimeRepresentation: Default {
    type InnerTime;

    fn convert_from(&mut self, inner: Self::InnerTime) -> Time;
    fn convert_into(&mut self, ts: Time) -> Self::InnerTime;

    fn convert_into_string(&mut self, ts: Time) -> String
    where
        Self::InnerTime: Display,
    {
        let inner = self.convert_into(ts);
        inner.to_string()
    }

    fn parse<'a>(&mut self, s: &'a str) -> Result<Time, String>
    where
        Self::InnerTime: TryFrom<&'a str>,
    {
        Self::InnerTime::try_from(s).map(|i| self.convert_from(i)).map_err(|e| e.to_string())
    }

    fn default_start_time() -> Option<SystemTime> {
        Some(SystemTime::now())
    }
    fn set_start_time(&mut self, _time: Option<SystemTime>) {}
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
}

#[derive(Debug, Copy, Clone, Default)]
pub struct RelativeFloat {}

impl TimeRepresentation for RelativeFloat {
    type InnerTime = (u64, u32);

    fn convert_from(&mut self, (secs, sub_secs): Self::InnerTime) -> Time {
        Duration::new(secs, sub_secs)
    }

    fn convert_into(&mut self, ts: Time) -> Self::InnerTime {
        (ts.as_secs(), ts.subsec_nanos())
    }

    fn convert_into_string(&mut self, ts: Time) -> String {
        let (secs, sub_secs) = self.convert_into(ts);
        let decimal = Decimal::from(secs) + Decimal::new(sub_secs as i64, 9);
        decimal.round_dp(9).to_string()
    }

    fn parse(&mut self, s: &str) -> Result<Time, String> {
        let num = Decimal::from_str(s)?;
        let nanos = (num.fract() * NANOS_IN_SECOND).to_u32().ok_or("Could not convert nano seconds")?;
        let secs = num.trunc().to_u64().ok_or("Could not convert seconds")?;
        Ok(self.convert_from((secs, nanos)))
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
}

#[derive(Debug, Copy, Clone, Default)]
pub struct OffsetFloat {
    current: Time,
    last_time: Time,
}

impl TimeRepresentation for OffsetFloat {
    type InnerTime = (u64, u32);

    fn convert_from(&mut self, (secs, nanos): Self::InnerTime) -> Time {
        self.last_time = self.current;
        self.current += Duration::new(secs, nanos);
        self.current
    }

    fn convert_into(&mut self, ts: Time) -> Self::InnerTime {
        let dur = ts - self.last_time;
        (dur.as_secs(), dur.subsec_nanos())
    }

    fn convert_into_string(&mut self, ts: Time) -> String {
        let (secs, sub_secs) = self.convert_into(ts);
        let decimal = Decimal::from(secs) + Decimal::new(sub_secs as i64, 9);
        decimal.round_dp(9).to_string()
    }

    fn parse(&mut self, s: &str) -> Result<Time, String> {
        let num = Decimal::from_str(s)?;
        let nanos = (num.fract() * NANOS_IN_SECOND).to_u32().ok_or("Could not convert nano seconds")?;
        let secs = num.trunc().to_u64().ok_or("Could not convert seconds")?;
        Ok(self.convert_from((secs, nanos)))
    }
}

#[derive(Debug, Copy, Clone, Default)]
pub struct AbsoluteFloat {
    start_time: Option<SystemTime>,
}

impl TimeRepresentation for AbsoluteFloat {
    type InnerTime = (u64, u32);

    fn convert_from(&mut self, (secs, nanos): Self::InnerTime) -> Time {
        let current = SystemTime::UNIX_EPOCH + Duration::new(secs, nanos);
        if self.start_time.is_none() {
            self.start_time = Some(current);
        }
        current.duration_since(self.start_time.unwrap()).expect("Time did not behave monotonically!")
    }

    fn convert_into(&mut self, ts: Time) -> Self::InnerTime {
        let ts = self.start_time.unwrap() + ts;
        let dur = ts.duration_since(SystemTime::UNIX_EPOCH).expect("Time did not behave monotonically!");
        (dur.as_secs(), dur.subsec_nanos())
    }

    fn convert_into_string(&mut self, ts: Time) -> String {
        let (secs, sub_secs) = self.convert_into(ts);
        let decimal = Decimal::from(secs) + Decimal::new(sub_secs as i64, 9);
        decimal.round_dp(9).to_string()
    }

    fn parse(&mut self, s: &str) -> Result<Time, String> {
        let num = Decimal::from_str(s)?;
        let nanos = (num.fract() * NANOS_IN_SECOND).to_u32().ok_or("Could not convert nano seconds")?;
        let secs = num.trunc().to_u64().ok_or("Could not convert seconds")?;
        Ok(self.convert_from((secs, nanos)))
    }

    fn default_start_time() -> Option<SystemTime> {
        None
    }

    fn set_start_time(&mut self, time: Option<SystemTime>) {
        self.start_time = time;
    }
}

#[derive(Debug, Copy, Clone, Default)]
pub struct AbsoluteRfc {
    start_time: Option<SystemTime>,
}

impl TimeRepresentation for AbsoluteRfc {
    type InnerTime = String;

    fn convert_from(&mut self, rfc: Self::InnerTime) -> Time {
        let current = humantime::parse_rfc3339(&rfc).unwrap();
        if self.start_time.is_none() {
            self.start_time = Some(current);
        }
        current.duration_since(self.start_time.unwrap()).expect("Time did not behave monotonically!")
    }

    fn convert_into(&mut self, ts: Time) -> Self::InnerTime {
        let ts = self.start_time.unwrap() + ts;
        humantime::format_rfc3339(ts).to_string()
    }

    fn default_start_time() -> Option<SystemTime> {
        None
    }

    fn set_start_time(&mut self, time: Option<SystemTime>) {
        self.start_time = time;
    }
}
