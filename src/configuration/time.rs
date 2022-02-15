use crate::Time;
use std::ops::Sub;
use std::str::FromStr;
use std::time::{Duration, SystemTime};

pub trait TimeRepresentation: Default {
    type InnerTime;

    fn convert_from(&mut self, inner: Self::InnerTime) -> Time;
    fn convert_into(&mut self, ts: Time) -> Self::InnerTime;
}

pub trait FromFloat {
    fn parse(s: &str) -> Result<(u64, u32), String>;
}

pub trait AbsoluteTime: TimeRepresentation {
    fn set_start_time(&mut self, time: SystemTime);
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
}

#[derive(Debug, Copy, Clone, Default)]
pub struct AbsoluteFloat {
    start_time: Option<SystemTime>,
}

impl AbsoluteTime for AbsoluteFloat {
    fn set_start_time(&mut self, time: SystemTime) {
        self.start_time = Some(time);
    }
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
}

#[derive(Debug, Copy, Clone, Default)]
pub struct AbsoluteRfc {
    start_time: Option<SystemTime>,
}

impl AbsoluteTime for AbsoluteRfc {
    fn set_start_time(&mut self, time: SystemTime) {
        self.start_time = Some(time);
    }
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
}

impl<T: TimeRepresentation<InnerTime = (u64, u32)>> FromFloat for T {
    fn parse(s: &str) -> Result<(u64, u32), String> {
        match s.split_once('.') {
            Some((secs, nanos)) => {
                let secs = u64::from_str(secs).map_err(|e| e.to_string())?;
                let nanos = nanos
                    .char_indices()
                    .fold(Some(0u32), |val, (pos, c)| {
                        val.and_then(|val| c.to_digit(10).map(|c| val + c * (10u32.pow(8 - pos as u32))))
                    })
                    .ok_or("invalid character in number literal")?;
                Ok((secs, nanos))
            }
            None => {
                let secs = u64::from_str(s).map_err(|e| e.to_string())?;
                Ok((secs, 0))
            }
        }
    }
}
