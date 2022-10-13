use std::cmp::Ordering;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::ops::Add;
use std::time::Duration;

use ordered_float::NotNan;
use rtlola_frontend::mir::{Type, WindowOperation as WinOp, WindowOperation};

use super::discrete_window::DiscreteWindowInstance;
use super::window_aggregations::*;
use super::Value;
use crate::Time;

const SIZE: usize = 64;

pub(crate) trait WindowInstanceTrait: Debug {
    fn get_value(&self, ts: Time) -> Value;
    fn accept_value(&mut self, v: Value, ts: Time);
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
    ($type: ty, $dur: ident, $wait: ident, $ts: ident, $active: ident) => {
        Self {
            inner: Box::new(RealTimeWindowInstance::<$type>::new($dur, $wait, $ts, $active)),
        }
    };
}
macro_rules! create_percentile_instance {
    ($type: ty, $dur: ident, $wait: ident, $ts: ident, $active: ident, $percentile: ident) => {
        Self {
            inner: Box::new(PercentileWindow {
                inner: RealTimeWindowInstance::<$type>::new($dur, $wait, $ts, $active),
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
        dur: Duration,
        wait: bool,
        op: WinOp,
        ts: Time,
        ty: &Type,
        active: bool,
    ) -> SlidingWindow {
        match (op, ty) {
            (WinOp::Count, _) => create_window_instance!(CountIv, dur, wait, ts, active),
            (WinOp::Min, Type::UInt(_)) => create_window_instance!(MinIv<WindowUnsigned>, dur, wait, ts, active),
            (WinOp::Min, Type::Int(_)) => create_window_instance!(MinIv<WindowSigned>, dur, wait, ts, active),
            (WinOp::Min, Type::Float(_)) => create_window_instance!(MinIv<WindowFloat>, dur, wait, ts, active),
            (WinOp::Max, Type::UInt(_)) => create_window_instance!(MaxIv<WindowUnsigned>, dur, wait, ts, active),
            (WinOp::Max, Type::Int(_)) => create_window_instance!(MaxIv<WindowSigned>, dur, wait, ts, active),
            (WinOp::Max, Type::Float(_)) => create_window_instance!(MaxIv<WindowFloat>, dur, wait, ts, active),
            (WinOp::Sum, Type::UInt(_)) => create_window_instance!(SumIv<WindowUnsigned>, dur, wait, ts, active),
            (WinOp::Sum, Type::Int(_)) => create_window_instance!(SumIv<WindowSigned>, dur, wait, ts, active),
            (WinOp::Sum, Type::Float(_)) => create_window_instance!(SumIv<WindowFloat>, dur, wait, ts, active),
            (WinOp::Sum, Type::Bool) => create_window_instance!(SumIv<WindowBool>, dur, wait, ts, active),
            (WinOp::Average, Type::UInt(_)) => create_window_instance!(AvgIv<WindowUnsigned>, dur, wait, ts, active),
            (WinOp::Average, Type::Int(_)) => create_window_instance!(AvgIv<WindowSigned>, dur, wait, ts, active),
            (WinOp::Average, Type::Float(_)) => create_window_instance!(AvgIv<WindowFloat>, dur, wait, ts, active),
            (WinOp::Integral, Type::Float(_)) | (WinOp::Integral, Type::Int(_)) | (WinOp::Integral, Type::UInt(_)) => {
                create_window_instance!(IntegralIv, dur, wait, ts, active)
            },
            (WinOp::Conjunction, Type::Bool) => create_window_instance!(ConjIv, dur, wait, ts, active),
            (WinOp::Disjunction, Type::Bool) => create_window_instance!(DisjIv, dur, wait, ts, active),
            (_, Type::Option(t)) => Self::from_sliding(dur, wait, op, ts, t, active),
            (WinOp::Conjunction, _) | (WinOp::Disjunction, _) => {
                panic!("conjunction and disjunction only defined on bool")
            },
            (WinOp::Min, _) | (WinOp::Max, _) | (WinOp::Sum, _) | (WinOp::Average, _) | (WinOp::Integral, _) => {
                panic!("arithmetic operation only defined on atomic numerics")
            },
            (WinOp::Last, Type::Int(_)) => create_window_instance!(LastIv<WindowSigned>, dur, wait, ts, active),
            (WinOp::Last, Type::UInt(_)) => create_window_instance!(LastIv<WindowUnsigned>, dur, wait, ts, active),
            (WinOp::Last, Type::Float(_)) => create_window_instance!(LastIv<WindowFloat>, dur, wait, ts, active),
            (WinOp::NthPercentile(x), Type::Int(_)) => {
                create_percentile_instance!(PercentileIv<WindowSigned>, dur, wait, ts, active, x)
            },
            (WinOp::NthPercentile(x), Type::UInt(_)) => {
                create_percentile_instance!(PercentileIv<WindowUnsigned>, dur, wait, ts, active, x)
            },
            (WinOp::NthPercentile(x), Type::Float(_)) => {
                create_percentile_instance!(PercentileIv<WindowFloat>, dur, wait, ts, active, x)
            },
            (WinOp::Variance, Type::Float(_)) => create_window_instance!(VarianceIv, dur, wait, ts, active),
            (WinOp::StandardDeviation, Type::Float(_)) => create_window_instance!(SdIv, dur, wait, ts, active),
            (WinOp::Covariance, Type::Float(_)) => create_window_instance!(CovIv, dur, wait, ts, active),
            (WinOp::Product, _) => unimplemented!("product not implemented"),
            (WindowOperation::Last, _) => unimplemented!(),
            (WindowOperation::Variance, _) => unimplemented!(),
            (WindowOperation::Covariance, _) => unimplemented!(),
            (WindowOperation::StandardDeviation, _) => unimplemented!(),
            (WindowOperation::NthPercentile(_), _) => unimplemented!(),
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
            (WindowOperation::Last, _) => unimplemented!(),
            (WindowOperation::Variance, _) => unimplemented!(),
            (WindowOperation::Covariance, _) => unimplemented!(),
            (WindowOperation::StandardDeviation, _) => unimplemented!(),
            (WindowOperation::NthPercentile(_), _) => unimplemented!(),
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
    buckets: VecDeque<IV>,
    time_per_bucket: Duration,
    start_time: Time,
    last_bucket_ix: BIx,
    wait: bool,
    wait_duration: Duration,
    active: bool,
}

#[derive(Clone, Copy, Debug)]
struct BIx {
    period: usize,
    ix: usize,
}

impl BIx {
    fn new(period: usize, ix: usize) -> BIx {
        BIx { period, ix }
    }

    fn buckets_since(self, other: BIx, num_buckets: usize) -> usize {
        match self.period.cmp(&other.period) {
            Ordering::Less => panic!("`other` bucket is more recent than `self`."),
            Ordering::Greater => {
                let period_diff = self.period - other.period;
                match self.ix.cmp(&other.ix) {
                    Ordering::Equal => period_diff * num_buckets,
                    Ordering::Less => {
                        let actual_p_diff = period_diff - 1;
                        let ix_diff = num_buckets - other.ix + self.ix;
                        actual_p_diff * num_buckets + ix_diff
                    },
                    Ordering::Greater => period_diff * num_buckets + (self.ix - other.ix),
                }
            },
            Ordering::Equal => {
                match self.ix.cmp(&other.ix) {
                    Ordering::Equal => 0,
                    Ordering::Less => panic!("`other` bucket is more recent than `self`."),
                    Ordering::Greater => self.ix - other.ix,
                }
            },
        }
    }
}

impl<IV: WindowIv> WindowInstanceTrait for RealTimeWindowInstance<IV> {
    /// Clears the current sliding window state
    fn deactivate(&mut self) {
        self.last_bucket_ix = BIx::new(0, 0);
        self.active = false;
    }

    /// Returns true if the window instance is currently active. I.e. the target stream instance currently exists.
    fn is_active(&self) -> bool {
        self.active
    }

    /// Restarts the sliding window
    fn activate(&mut self, ts: Time) {
        self.buckets = VecDeque::from(vec![IV::default(ts); SIZE]);
        self.start_time = ts;
        self.active = true;
    }

    /// You should always call `WindowInstance::update_buckets` before calling `WindowInstance::get_value()`!
    fn get_value(&self, ts: Time) -> Value {
        if !self.active {
            return IV::default(ts).into();
        }
        // Reversal is essential for non-commutative operations.
        if self.wait && ts < self.wait_duration {
            return Value::None;
        }
        self.buckets
            .iter()
            .rev()
            .fold(IV::default(ts), |acc, e| acc + e.clone())
            .into()
    }

    fn accept_value(&mut self, v: Value, ts: Time) {
        assert!(self.active);
        self.update_buckets(ts);
        let b = self.buckets.get_mut(0).expect("Bug!");
        *b = b.clone() + (v, ts).into(); // TODO: Require add_assign rather than add.
    }

    fn update_buckets(&mut self, ts: Time) {
        assert!(self.active);
        let curr = self.get_current_bucket(ts);
        let last = self.last_bucket_ix;

        let diff = curr.buckets_since(last, self.buckets.len());
        self.invalidate_n(diff, ts);
        self.last_bucket_ix = curr;
    }
}

impl<IV: WindowIv> RealTimeWindowInstance<IV> {
    fn new(dur: Duration, wait: bool, ts: Time, active: bool) -> Self {
        let time_per_bucket = dur / (SIZE as u32);
        let buckets = VecDeque::from(vec![IV::default(ts); SIZE]);
        // last bucket_ix is 1, so we consider all buckets, i.e. from 1 to end and from start to 0,
        // as in use. Whenever we progress by n buckets, we invalidate the pseudo-used ones.
        // This is safe since the value within is the neutral element of the operation.
        Self {
            buckets,
            time_per_bucket,
            start_time: ts,
            last_bucket_ix: BIx::new(0, 0),
            wait,
            wait_duration: dur,
            active,
        }
    }

    fn invalidate_n(&mut self, n: usize, ts: Time) {
        assert!(self.active);
        for _ in 0..n {
            self.buckets.pop_back();
            self.buckets.push_front(IV::default(ts));
        }
    }

    fn get_current_bucket(&self, ts: Time) -> BIx {
        assert!(self.active);
        // let overall_ix = ts.duration_since(self.start_time).div_duration(self.time_per_bucket);
        assert!(ts >= self.start_time, "Time does not behave monotonically!");
        let overall_ix = Self::quickfix_duration_div(ts - self.start_time, self.time_per_bucket);
        let overall_ix = overall_ix.floor() as usize;
        let period = overall_ix / self.buckets.len();
        let ix = overall_ix % self.buckets.len();
        BIx { period, ix }
    }

    fn quickfix_duration_div(a: Duration, b: Duration) -> f64 {
        let a_secs = a.as_secs();
        let a_nanos = a.subsec_nanos();
        let b_secs = b.as_secs();
        let b_nanos = b.subsec_nanos();
        let a = (a_secs as f64) + f64::from(a_nanos) / f64::from(1_000_000_000);
        let b = (b_secs as f64) + f64::from(b_nanos) / f64::from(1_000_000_000);
        a / b
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
        if self.inner.wait && ts < self.inner.wait_duration {
            return Value::None;
        }
        self.inner
            .buckets
            .iter()
            .rev()
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
