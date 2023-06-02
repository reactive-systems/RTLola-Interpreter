use std::cmp::Ordering;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::ops::{Add, Div};
use std::time::Duration;

use ordered_float::NotNan;
use rtlola_frontend::mir::{
    MemorizationBound, SlidingWindow as MirSlidingWindow, Type, Window, WindowOperation as WinOp,
};

use super::discrete_window::DiscreteWindowInstance;
use super::window_aggregations::*;
use super::Value;
use crate::Time;

const SIZE: usize = 64;

pub(crate) trait WindowInstanceTrait: Debug {
    /// Computes the current value of a sliding window instance with the given timestamp:
    /// # Arguments:
    /// * 'ts' - the current timestamp of the monitor
    /// Note: You should always call `SlidingWindow::update` before calling `SlidingWindow::get_value()`!
    fn get_value(&self, ts: Time) -> Value;
    /// Updates the value of the current bucket of a sliding window instance with the current value of the accessed stream:
    /// # Arguments:
    /// * 'v' - the current value of the accessed stream
    /// * 'ts' - the current timestamp of the monitor
    fn accept_value(&mut self, v: Value, ts: Time);
    /// Updates the buckets of a sliding window instance with the given timestamp:
    /// # Arguments:
    /// * 'ts' - the current timestamp of the monitor
    fn update_buckets(&mut self, ts: Time);
    /// Clears the current sliding window state
    fn deactivate(&mut self);

    /// Returns true if the window instance is currently active. I.e. the target stream instance currently exists.
    fn is_active(&self) -> bool;

    /// Restarts the sliding window
    fn activate(&mut self, ts: Time);
}

/// Representation of sliding window aggregations:
/// The enum differentiates the aggregation functions and between different value types, dependent on the aggregation function.
/// # Example:
/// * The aggregation function 'count' is independent of the value type.
/// * The aggregation function 'min' depends on the value type, e.g., the minimum value of unsigned values is 0, whereas the minimum value for signed values is negative.
#[derive(Debug)]
pub(crate) struct SlidingWindow {
    inner: Box<dyn WindowInstanceTrait>,
}

macro_rules! create_window_instance {
    ($type: ty, $w: ident, $ts: ident, $active: ident) => {
        Self {
            inner: Box::new(RealTimeWindowInstance::<$type>::new($w, $ts, $active)),
        }
    };
}
macro_rules! create_percentile_instance {
    ($type: ty, $w: ident, $ts: ident, $active: ident, $percentile: ident) => {
        Self {
            inner: Box::new(PercentileWindow {
                inner: RealTimeWindowInstance::<$type>::new($w, $ts, $active),
                percentile: $percentile,
            }),
        }
    };
}
macro_rules! create_discrete_window_instance {
    ($type: ty, $dur: ident, $wait: ident, $ts: ident, $active: ident) => {
        Self {
            inner: Box::new(DiscreteWindowInstance::<$type>::new($dur, $wait, $ts, $active)),
        }
    };
}
macro_rules! create_discrete_percentile_instance {
    ($type: ty, $dur: ident, $wait: ident, $ts: ident, $active: ident, $percentile: ident) => {
        Self {
            inner: Box::new(PercentileWindow {
                inner: DiscreteWindowInstance::<$type>::new($dur, $wait, $ts, $active),
                percentile: $percentile,
            }),
        }
    };
}
impl SlidingWindow {
    /// Returns a sliding window instance, from:
    /// # Arguments:
    /// * 'dur'- the duration of the window
    /// * 'wait' - the boolean flag to decide if the window returns its value after the complete duration has passed
    /// * 'op' - the type of the aggregation function
    /// * 'ts' - the starting time of the window
    /// * 'ty' - the value type of the aggregated stream
    pub(crate) fn from_sliding(
        ts: Time,
        window: &MirSlidingWindow,
        active: bool,
    ) -> SlidingWindow {
        match (window.op, &window.ty) {
            (WinOp::Count, _) => create_window_instance!(CountIv, window, ts, active),
            (WinOp::Min, Type::UInt(_)) => create_window_instance!(MinIv<WindowUnsigned>, window, ts, active),
            (WinOp::Min, Type::Int(_)) => create_window_instance!(MinIv<WindowSigned>, window, ts, active),
            (WinOp::Min, Type::Float(_)) => create_window_instance!(MinIv<WindowFloat>, window, ts, active),
            (WinOp::Max, Type::UInt(_)) => create_window_instance!(MaxIv<WindowUnsigned>, window, ts, active),
            (WinOp::Max, Type::Int(_)) => create_window_instance!(MaxIv<WindowSigned>, window, ts, active),
            (WinOp::Max, Type::Float(_)) => create_window_instance!(MaxIv<WindowFloat>, window, ts, active),
            (WinOp::Sum, Type::UInt(_)) => create_window_instance!(SumIv<WindowUnsigned>, window, ts, active),
            (WinOp::Sum, Type::Int(_)) => create_window_instance!(SumIv<WindowSigned>, window, ts, active),
            (WinOp::Sum, Type::Float(_)) => create_window_instance!(SumIv<WindowFloat>, window, ts, active),
            (WinOp::Sum, Type::Bool) => create_window_instance!(SumIv<WindowBool>, window, ts, active),
            (WinOp::Average, Type::UInt(_)) => create_window_instance!(AvgIv<WindowUnsigned>, window, ts, active),
            (WinOp::Average, Type::Int(_)) => create_window_instance!(AvgIv<WindowSigned>, window, ts, active),
            (WinOp::Average, Type::Float(_)) => create_window_instance!(AvgIv<WindowFloat>, window, ts, active),
            (WinOp::Integral, Type::Float(_)) | (WinOp::Integral, Type::Int(_)) | (WinOp::Integral, Type::UInt(_)) => {
                create_window_instance!(IntegralIv, window, ts, active)
            },
            (WinOp::Conjunction, Type::Bool) => create_window_instance!(ConjIv, window, ts, active),
            (WinOp::Disjunction, Type::Bool) => create_window_instance!(DisjIv, window, ts, active),
            (_, Type::Option(t)) => Self::from_sliding(ts, &MirSlidingWindow{
                target: window.target,
                caller: window.caller,
                duration: window.duration,
                num_buckets: window.num_buckets,
                bucket_size: window.bucket_size,
                wait: window.wait,
                op: window.op,
                reference: window.reference,
                ty: t.as_ref().clone(),
            }, active),
            (WinOp::Conjunction, _) | (WinOp::Disjunction, _) => {
                panic!("conjunction and disjunction only defined on bool")
            },
            (WinOp::Min, _) | (WinOp::Max, _) | (WinOp::Sum, _) | (WinOp::Average, _) | (WinOp::Integral, _) => {
                panic!("arithmetic operation only defined on atomic numerics")
            },
            (WinOp::Last, Type::Int(_)) => create_window_instance!(LastIv<WindowSigned>, window, ts, active),
            (WinOp::Last, Type::UInt(_)) => create_window_instance!(LastIv<WindowUnsigned>, window, ts, active),
            (WinOp::Last, Type::Float(_)) => create_window_instance!(LastIv<WindowFloat>, window, ts, active),
            (WinOp::NthPercentile(x), Type::Int(_)) => {
                create_percentile_instance!(PercentileIv<WindowSigned>, window, ts, active, x)
            },
            (WinOp::NthPercentile(x), Type::UInt(_)) => {
                create_percentile_instance!(PercentileIv<WindowUnsigned>, window, ts, active, x)
            },
            (WinOp::NthPercentile(x), Type::Float(_)) => {
                create_percentile_instance!(PercentileIv<WindowFloat>, window, ts, active, x)
            },
            (WinOp::Variance, Type::Float(_)) => create_window_instance!(VarianceIv, window, ts, active),
            (WinOp::StandardDeviation, Type::Float(_)) => create_window_instance!(SdIv, window, ts, active),
            (WinOp::Covariance, Type::Float(_)) => create_window_instance!(CovIv, window, ts, active),
            (WinOp::Product, _) => unimplemented!("product not implemented"),
            (WinOp::Last, _) => unimplemented!(),
            (WinOp::Variance, _) => unimplemented!(),
            (WinOp::Covariance, _) => unimplemented!(),
            (WinOp::StandardDeviation, _) => unimplemented!(),
            (WinOp::NthPercentile(_), _) => unimplemented!(),
        }
    }

    pub(crate) fn from_discrete(
        size: usize,
        wait: bool,
        op: WinOp,
        ts: Time,
        ty: &Type,
        active: bool,
    ) -> SlidingWindow {
        match (op, ty) {
            (WinOp::Count, _) => create_discrete_window_instance!(CountIv, size, wait, ts, active),
            (WinOp::Min, Type::UInt(_)) => {
                create_discrete_window_instance!(MinIv<WindowUnsigned>, size, wait, ts, active)
            },
            (WinOp::Min, Type::Int(_)) => create_discrete_window_instance!(MinIv<WindowSigned>, size, wait, ts, active),
            (WinOp::Min, Type::Float(_)) => {
                create_discrete_window_instance!(MinIv<WindowFloat>, size, wait, ts, active)
            },
            (WinOp::Max, Type::UInt(_)) => {
                create_discrete_window_instance!(MaxIv<WindowUnsigned>, size, wait, ts, active)
            },
            (WinOp::Max, Type::Int(_)) => create_discrete_window_instance!(MaxIv<WindowSigned>, size, wait, ts, active),
            (WinOp::Max, Type::Float(_)) => {
                create_discrete_window_instance!(MaxIv<WindowFloat>, size, wait, ts, active)
            },
            (WinOp::Sum, Type::UInt(_)) => {
                create_discrete_window_instance!(SumIv<WindowUnsigned>, size, wait, ts, active)
            },
            (WinOp::Sum, Type::Int(_)) => create_discrete_window_instance!(SumIv<WindowSigned>, size, wait, ts, active),
            (WinOp::Sum, Type::Float(_)) => {
                create_discrete_window_instance!(SumIv<WindowFloat>, size, wait, ts, active)
            },
            (WinOp::Sum, Type::Bool) => create_discrete_window_instance!(SumIv<WindowBool>, size, wait, ts, active),
            (WinOp::Average, Type::UInt(_)) => {
                create_discrete_window_instance!(AvgIv<WindowUnsigned>, size, wait, ts, active)
            },
            (WinOp::Average, Type::Int(_)) => {
                create_discrete_window_instance!(AvgIv<WindowSigned>, size, wait, ts, active)
            },
            (WinOp::Average, Type::Float(_)) => {
                create_discrete_window_instance!(AvgIv<WindowFloat>, size, wait, ts, active)
            },
            (WinOp::Integral, Type::Float(_)) | (WinOp::Integral, Type::Int(_)) | (WinOp::Integral, Type::UInt(_)) => {
                create_discrete_window_instance!(IntegralIv, size, wait, ts, active)
            },
            (WinOp::Conjunction, Type::Bool) => create_discrete_window_instance!(ConjIv, size, wait, ts, active),
            (WinOp::Disjunction, Type::Bool) => create_discrete_window_instance!(DisjIv, size, wait, ts, active),
            (_, Type::Option(t)) => Self::from_discrete(size, wait, op, ts, t, active),
            (WinOp::Conjunction, _) | (WinOp::Disjunction, _) => {
                panic!("conjunction and disjunction only defined on bool")
            },
            (WinOp::Min, _) | (WinOp::Max, _) | (WinOp::Sum, _) | (WinOp::Average, _) | (WinOp::Integral, _) => {
                panic!("arithmetic operation only defined on atomic numerics")
            },
            (WinOp::Last, Type::Int(_)) => {
                create_discrete_window_instance!(LastIv<WindowSigned>, size, wait, ts, active)
            },
            (WinOp::Last, Type::UInt(_)) => {
                create_discrete_window_instance!(LastIv<WindowUnsigned>, size, wait, ts, active)
            },
            (WinOp::Last, Type::Float(_)) => {
                create_discrete_window_instance!(LastIv<WindowFloat>, size, wait, ts, active)
            },
            (WinOp::NthPercentile(x), Type::Int(_)) => {
                create_discrete_percentile_instance!(PercentileIv<WindowSigned>, size, wait, ts, active, x)
            },
            (WinOp::NthPercentile(x), Type::UInt(_)) => {
                create_discrete_percentile_instance!(PercentileIv<WindowUnsigned>, size, wait, ts, active, x)
            },
            (WinOp::NthPercentile(x), Type::Float(_)) => {
                create_discrete_percentile_instance!(PercentileIv<WindowFloat>, size, wait, ts, active, x)
            },
            (WinOp::Variance, Type::Float(_)) => create_discrete_window_instance!(VarianceIv, size, wait, ts, active),
            (WinOp::StandardDeviation, Type::Float(_)) => {
                create_discrete_window_instance!(SdIv, size, wait, ts, active)
            },
            (WinOp::Covariance, Type::Float(_)) => create_discrete_window_instance!(CovIv, size, wait, ts, active),
            (WinOp::Product, _) => unimplemented!("product not implemented"),
            (WinOp::Last, _) => unimplemented!(),
            (WinOp::Variance, _) => unimplemented!(),
            (WinOp::Covariance, _) => unimplemented!(),
            (WinOp::StandardDeviation, _) => unimplemented!(),
            (WinOp::NthPercentile(_), _) => unimplemented!(),
        }
    }

    /// Updates the buckets of a sliding window instance with the given timestamp:
    /// # Arguments:
    /// * 'ts' - the current timestamp of the monitor
    pub(crate) fn update(&mut self, ts: Time) {
        self.inner.update_buckets(ts);
    }

    /// Computes the current value of a sliding window instance with the given timestamp:
    /// # Arguments:
    /// * 'ts' - the current timestamp of the monitor
    /// Note: You should always call `SlidingWindow::update` before calling `SlidingWindow::get_value()`!
    pub(crate) fn get_value(&self, ts: Time) -> Value {
        self.inner.get_value(ts)
    }

    /// Updates the value of the first bucket of a sliding window instance with the current value of the accessed stream:
    /// # Arguments:
    /// * 'v' - the current value of the accessed stream
    /// * 'ts' - the current timestamp of the monitor
    pub(crate) fn accept_value(&mut self, v: Value, ts: Time) {
        self.inner.accept_value(v, ts);
    }

    /// Clears the current sliding window state
    pub(crate) fn deactivate(&mut self) {
        self.inner.deactivate();
    }

    /// Returns true if the window instance is currently active. I.e. the target stream instance currently exists.
    pub(crate) fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    /// Restarts the sliding window
    pub(crate) fn activate(&mut self, ts: Time) {
        self.inner.activate(ts);
    }
}

// TODO: Consider using None rather than Default.
/// Trait to summarize common logic for the different window aggregations, e.g., returning a default value for an empty bucket
pub(crate) trait WindowIv:
    Clone + Add<Output = Self> + From<(Value, Time)> + Sized + Debug + Into<Value>
{
    fn default(ts: Time) -> Self;
}

/// Struct to summarize common logic for the different window aggregations, e.g. iterating over the buckets to compute the result of an aggregation
#[derive(Debug)]
pub(crate) struct RealTimeWindowInstance<IV: WindowIv> {
    buckets: Vec<IV>,
    start_time: Time,
    last_update: Time,
    total_duration: Duration,
    bucket_duration: Duration,
    current_bucket: usize,
    wait: bool,
    active: bool,
}

impl<IV: WindowIv> WindowInstanceTrait for RealTimeWindowInstance<IV> {
    /// Clears the current sliding window state
    fn deactivate(&mut self) {
        self.active = false;
    }

    /// Returns true if the window instance is currently active. I.e. the target stream instance currently exists.
    fn is_active(&self) -> bool {
        self.active
    }

    /// Restarts the sliding window
    fn activate(&mut self, ts: Time) {
        self.clear_all_buckets(ts);
        self.current_bucket = 0;
        self.start_time = ts;
        self.last_update = ts;
        self.active = true;
    }

    /// You should always call `WindowInstance::update_buckets` before calling `WindowInstance::get_value()`!
    fn get_value(&self, ts: Time) -> Value {
        if !self.active {
            return IV::default(ts).into();
        }
        // Reversal is essential for non-commutative operations.
        if self.wait && ts < self.total_duration {
            return Value::None;
        }
        self.buckets
            .iter()
            .fold(IV::default(ts), |acc, e| acc + e.clone())
            .into()
    }

    fn accept_value(&mut self, v: Value, ts: Time) {
        assert!(self.active);
        self.update_buckets(ts);
        let b = self.buckets.get_mut(self.current_bucket).expect("Bug!");
        *b = b.clone() + (v, ts).into(); // TODO: Require add_assign rather than add.
    }

    fn update_buckets(&mut self, ts: Time) {
        assert!(self.active);
        let curr = self.get_current_bucket(ts);
        let last = self.current_bucket;

        // rounds taken in the ringbuffer since the last update
        let rounds = ((ts.as_nanos() - self.last_update.as_nanos()) / self.total_duration.as_nanos()) as u64;
        dbg!(rounds);
        if rounds >= 1 {
            // clear all buckets
            self.clear_all_buckets(ts);
        } else {
            // clear only buckets between curren and last bucket.
            if curr > last {
                self.clear_buckets(ts, last+1, curr+1);
            } else if curr < last {
                self.clear_buckets(ts, last + 1, self.buckets.len());
                self.clear_buckets(ts, 0, curr+1);
            }
        }
        self.current_bucket = curr;
        self.last_update = ts;
    }
}

impl<IV: WindowIv> RealTimeWindowInstance<IV> {
    fn new(window: &MirSlidingWindow, ts: Time, active: bool) -> Self {
        let num_buckets = if let MemorizationBound::Bounded(num_buckets) = window.memory_bound() {
            num_buckets as usize
        } else {
            unreachable!()
        };

        let buckets = vec![IV::default(ts); num_buckets];
        // last bucket_ix is 1, so we consider all buckets, i.e. from 1 to end and from start to 0,
        // as in use. Whenever we progress by n buckets, we invalidate the pseudo-used ones.
        // This is safe since the value within is the neutral element of the operation.
        Self {
            buckets,
            bucket_duration: window.bucket_size,
            total_duration: window.duration,
            start_time: ts,
            last_update: ts,
            current_bucket: 0,
            wait: window.wait,
            active,
        }
    }

    fn get_current_bucket(&self, ts: Time) -> usize {
        assert!(self.active);
        assert!(ts >= self.start_time, "Time does not behave monotonically!");
        let ts = ts.as_nanos() as usize;
        let window_duration = self.total_duration.as_nanos() as usize;
        let bucket_duration = self.bucket_duration.as_nanos() as usize;
        let relative_to_window = ts % window_duration;
        let idx = relative_to_window / bucket_duration;
        dbg!(self.buckets.len());
        dbg!(ts, window_duration, bucket_duration, relative_to_window, idx);
        dbg!(if ts % bucket_duration == 0 {
            // A bucket includes time from x < ts <= x + bucket_duration
            // Consequently, if we hit the "edge" of bucket we have to chose the previous one
            if idx > 0 {
                idx - 1
            } else {
                self.buckets.len() -1
            }

        } else {
            idx
        })
    }

    // clear buckets starting from `start` to `last` including start, excluding end
    fn clear_buckets(&mut self, ts: Time, start: usize, end: usize) {
        dbg!(&self.buckets);
        self.buckets[start..end]
            .iter_mut()
            .for_each(|x| *x = IV::default(ts));
        dbg!(&self.buckets);
    }

    fn clear_all_buckets(&mut self, ts: Time) {
        self.clear_buckets(ts, 0, self.buckets.len())
    }
}

#[derive(Debug)]
pub(crate) struct PercentileWindow<IC: WindowInstanceTrait> {
    pub(crate) inner: IC,
    pub(crate) percentile: u8,
}

impl<G: WindowGeneric> WindowInstanceTrait for PercentileWindow<RealTimeWindowInstance<PercentileIv<G>>> {
    fn get_value(&self, ts: Time) -> Value {
        // Reversal is essential for non-commutative operations.
        if self.inner.wait && ts < self.inner.total_duration {
            return Value::None;
        }
        self.inner
            .buckets
            .iter()
            .fold(PercentileIv::default(ts), |acc, e| acc + e.clone())
            .percentile_get_value(self.percentile)
    }

    fn accept_value(&mut self, v: Value, ts: Time) {
        self.inner.accept_value(v, ts)
    }

    fn update_buckets(&mut self, ts: Time) {
        self.inner.update_buckets(ts)
    }

    fn deactivate(&mut self) {
        self.inner.deactivate()
    }

    fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    fn activate(&mut self, ts: Time) {
        self.inner.activate(ts)
    }
}

pub(crate) trait WindowGeneric: Debug + Clone {
    fn from_value(v: Value) -> Value;
}

#[derive(Debug, Clone)]
pub(crate) struct WindowSigned {}
impl WindowGeneric for WindowSigned {
    fn from_value(v: Value) -> Value {
        match v {
            Value::Signed(_) => v,
            Value::Unsigned(u) => Value::Signed(u as i64),
            _ => unreachable!("Type error."),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct WindowBool {}
impl WindowGeneric for WindowBool {
    fn from_value(v: Value) -> Value {
        match v {
            Value::Bool(b) if b => Value::Unsigned(1),
            Value::Bool(_) => Value::Unsigned(0),
            _ => unreachable!("Type error."),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct WindowUnsigned {}
impl WindowGeneric for WindowUnsigned {
    fn from_value(v: Value) -> Value {
        match v {
            Value::Unsigned(_) => v,
            _ => unreachable!("Type error."),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct WindowFloat {}
impl WindowGeneric for WindowFloat {
    fn from_value(v: Value) -> Value {
        let f = match v {
            Value::Signed(i) => i as f64,
            Value::Unsigned(u) => u as f64,
            Value::Float(f) => f.into(),
            _ => unreachable!("Type error."),
        };
        Value::Float(NotNan::new(f).unwrap())
    }
}
