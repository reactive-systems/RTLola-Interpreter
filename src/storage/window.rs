use super::discrete_window::DiscreteWindowInstance;
use super::window_aggregations::*;
use super::Value;
use crate::basics::Time;
use ordered_float::NotNan;
use rtlola_frontend::mir::{Type, WindowOperation as WinOp};
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::ops::Add;
use std::time::Duration;

#[macro_export]
macro_rules! match_and_run {
    ( $window:expr, $name:ident $( , $arg:ident )* ) => {
        match $window {
            SlidingWindow::Count(wi) => wi.$name($($arg),*),
            SlidingWindow::CountDiscrete(wi) => wi.$name($($arg),*),
            SlidingWindow::MinUnsigned(wi) => wi.$name($($arg),*),
            SlidingWindow::MinUnsignedDiscrete(wi) => wi.$name($($arg),*),
            SlidingWindow::MinSigned(wi) => wi.$name($($arg),*),
            SlidingWindow::MinSignedDiscrete(wi) => wi.$name($($arg),*),
            SlidingWindow::MinFloat(wi) => wi.$name($($arg),*),
            SlidingWindow::MinFloatDiscrete(wi) => wi.$name($($arg),*),
            SlidingWindow::MaxUnsigned(wi) => wi.$name($($arg),*),
            SlidingWindow::MaxUnsignedDiscrete(wi) => wi.$name($($arg),*),
            SlidingWindow::MaxSigned(wi) => wi.$name($($arg),*),
            SlidingWindow::MaxSignedDiscrete(wi) => wi.$name($($arg),*),
            SlidingWindow::MaxFloat(wi) => wi.$name($($arg),*),
            SlidingWindow::MaxFloatDiscrete(wi) => wi.$name($($arg),*),
            SlidingWindow::SumUnsigned(wi) => wi.$name($($arg),*),
            SlidingWindow::SumUnsignedDiscrete(wi) => wi.$name($($arg),*),
            SlidingWindow::SumSigned(wi) => wi.$name($($arg),*),
            SlidingWindow::SumSignedDiscrete(wi) => wi.$name($($arg),*),
            SlidingWindow::SumFloat(wi) => wi.$name($($arg),*),
            SlidingWindow::SumFloatDiscrete(wi) => wi.$name($($arg),*),
            SlidingWindow::SumBool(wi) => wi.$name($($arg),*),
            SlidingWindow::SumBoolDiscrete(wi) => wi.$name($($arg),*),
            SlidingWindow::Conjunction(wi) => wi.$name($($arg),*),
            SlidingWindow::ConjunctionDiscrete(wi) => wi.$name($($arg),*),
            SlidingWindow::Disjunction(wi) => wi.$name($($arg),*),
            SlidingWindow::DisjunctionDiscrete(wi) => wi.$name($($arg),*),
            SlidingWindow::AvgUnsigned(wi) => wi.$name($($arg),*),
            SlidingWindow::AvgUnsignedDiscrete(wi) => wi.$name($($arg),*),
            SlidingWindow::AvgSigned(wi) => wi.$name($($arg),*),
            SlidingWindow::AvgSignedDiscrete(wi) => wi.$name($($arg),*),
            SlidingWindow::AvgFloat(wi) => wi.$name($($arg),*),
            SlidingWindow::AvgFloatDiscrete(wi) => wi.$name($($arg),*),
            SlidingWindow::Integral(wi) => wi.$name($($arg),*),
            SlidingWindow::IntegralDiscrete(wi) => wi.$name($($arg),*),
            SlidingWindow::LastSigned(wi) => wi.$name($($arg),*),
            SlidingWindow::LastUnsigned(wi) => wi.$name($($arg),*),
            SlidingWindow::LastFloat(wi) => wi.$name($($arg),*),
            SlidingWindow::LastDiscreteSigned(wi) => wi.$name($($arg),*),
            SlidingWindow::LastDiscreteUnsigned(wi) => wi.$name($($arg),*),
            SlidingWindow::LastDiscreteFloat(wi) => wi.$name($($arg),*),
            SlidingWindow::PercentileSigned(_, wi) => wi.$name($($arg),*),
            SlidingWindow::PercentileUnsigned(_, wi) => wi.$name($($arg),*),
            SlidingWindow::PercentileFloat(_, wi) => wi.$name($($arg),*),
            SlidingWindow::PercentileDiscreteSigned(_, wi) => wi.$name($($arg),*),
            SlidingWindow::PercentileDiscreteUnsigned(_, wi) => wi.$name($($arg),*),
            SlidingWindow::PercentileDiscreteFloat(_, wi) => wi.$name($($arg),*),
            SlidingWindow::Variance(wi) => wi.$name($($arg),*),
            SlidingWindow::VarianceDiscrete(wi) => wi.$name($($arg),*),
            SlidingWindow::StandardDeviation(wi) => wi.$name($($arg),*),
            SlidingWindow::StandardDeviationDiscrete(wi) => wi.$name($($arg),*),
            SlidingWindow::Covariance(wi) => wi.$name($($arg),*),
            SlidingWindow::CovarianceDiscrete(wi) => wi.$name($($arg),*),
        }
    };
}

const SIZE: usize = 64;

/// Representation of sliding window aggregations:
/// The enum differentiates the aggregation functions and between different value types, dependent on the aggregation function.
/// # Example:
/// * The aggregation function 'count' is independent of the value type.
/// * The aggregation function 'min' depends on the value type, e.g., the minimum value of unsigned values is 0, whereas the minimum value for signed values is negative.
#[derive(Debug)]
pub(crate) enum SlidingWindow {
    Count(RealTimeWindowInstance<CountIv>),
    CountDiscrete(DiscreteWindowInstance<CountIv>),
    MinUnsigned(RealTimeWindowInstance<MinIv<WindowSigned>>),
    MinUnsignedDiscrete(DiscreteWindowInstance<MinIv<WindowSigned>>),
    MinSigned(RealTimeWindowInstance<MinIv<WindowUnsigned>>),
    MinSignedDiscrete(DiscreteWindowInstance<MinIv<WindowUnsigned>>),
    MinFloat(RealTimeWindowInstance<MinIv<WindowFloat>>),
    MinFloatDiscrete(DiscreteWindowInstance<MinIv<WindowFloat>>),
    MaxUnsigned(RealTimeWindowInstance<MaxIv<WindowSigned>>),
    MaxUnsignedDiscrete(DiscreteWindowInstance<MaxIv<WindowSigned>>),
    MaxSigned(RealTimeWindowInstance<MaxIv<WindowUnsigned>>),
    MaxSignedDiscrete(DiscreteWindowInstance<MaxIv<WindowUnsigned>>),
    MaxFloat(RealTimeWindowInstance<MaxIv<WindowFloat>>),
    MaxFloatDiscrete(DiscreteWindowInstance<MaxIv<WindowFloat>>),
    SumUnsigned(RealTimeWindowInstance<SumIv<WindowUnsigned>>),
    SumUnsignedDiscrete(DiscreteWindowInstance<SumIv<WindowUnsigned>>),
    SumSigned(RealTimeWindowInstance<SumIv<WindowSigned>>),
    SumSignedDiscrete(DiscreteWindowInstance<SumIv<WindowSigned>>),
    SumFloat(RealTimeWindowInstance<SumIv<WindowFloat>>),
    SumFloatDiscrete(DiscreteWindowInstance<SumIv<WindowFloat>>),
    SumBool(RealTimeWindowInstance<SumIv<WindowBool>>),
    SumBoolDiscrete(DiscreteWindowInstance<SumIv<WindowBool>>),
    AvgUnsigned(RealTimeWindowInstance<AvgIv<WindowUnsigned>>),
    AvgUnsignedDiscrete(DiscreteWindowInstance<AvgIv<WindowUnsigned>>),
    AvgSigned(RealTimeWindowInstance<AvgIv<WindowSigned>>),
    AvgSignedDiscrete(DiscreteWindowInstance<AvgIv<WindowSigned>>),
    AvgFloat(RealTimeWindowInstance<AvgIv<WindowFloat>>),
    AvgFloatDiscrete(DiscreteWindowInstance<AvgIv<WindowFloat>>),
    Integral(RealTimeWindowInstance<IntegralIv>),
    IntegralDiscrete(DiscreteWindowInstance<IntegralIv>),
    Conjunction(RealTimeWindowInstance<ConjIv>),
    ConjunctionDiscrete(DiscreteWindowInstance<ConjIv>),
    Disjunction(RealTimeWindowInstance<DisjIv>),
    DisjunctionDiscrete(DiscreteWindowInstance<DisjIv>),
    LastSigned(RealTimeWindowInstance<LastIv<WindowSigned>>),
    LastUnsigned(RealTimeWindowInstance<LastIv<WindowUnsigned>>),
    LastFloat(RealTimeWindowInstance<LastIv<WindowFloat>>),
    LastDiscreteSigned(DiscreteWindowInstance<LastIv<WindowSigned>>),
    LastDiscreteUnsigned(DiscreteWindowInstance<LastIv<WindowUnsigned>>),
    LastDiscreteFloat(DiscreteWindowInstance<LastIv<WindowFloat>>),
    PercentileSigned(usize, RealTimeWindowInstance<PercentileIv<WindowSigned>>),
    PercentileUnsigned(usize, RealTimeWindowInstance<PercentileIv<WindowUnsigned>>),
    PercentileFloat(usize, RealTimeWindowInstance<PercentileIv<WindowFloat>>),
    PercentileDiscreteSigned(usize, DiscreteWindowInstance<PercentileIv<WindowSigned>>),
    PercentileDiscreteUnsigned(usize, DiscreteWindowInstance<PercentileIv<WindowUnsigned>>),
    PercentileDiscreteFloat(usize, DiscreteWindowInstance<PercentileIv<WindowFloat>>),
    Variance(RealTimeWindowInstance<VarianceIv>),
    VarianceDiscrete(DiscreteWindowInstance<VarianceIv>),
    StandardDeviation(RealTimeWindowInstance<SdIv>),
    StandardDeviationDiscrete(DiscreteWindowInstance<SdIv>),
    Covariance(RealTimeWindowInstance<CovIv>),
    CovarianceDiscrete(DiscreteWindowInstance<CovIv>),
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
            (WinOp::Count, _) => SlidingWindow::Count(RealTimeWindowInstance::new(dur, wait, active, ts)),
            (WinOp::Min, Type::UInt(_)) => {
                SlidingWindow::MinUnsigned(RealTimeWindowInstance::new(dur, wait, active, ts))
            }
            (WinOp::Min, Type::Int(_)) => SlidingWindow::MinSigned(RealTimeWindowInstance::new(dur, wait, active, ts)),
            (WinOp::Min, Type::Float(_)) => SlidingWindow::MinFloat(RealTimeWindowInstance::new(dur, wait, active, ts)),
            (WinOp::Max, Type::UInt(_)) => {
                SlidingWindow::MaxUnsigned(RealTimeWindowInstance::new(dur, wait, active, ts))
            }
            (WinOp::Max, Type::Int(_)) => SlidingWindow::MaxSigned(RealTimeWindowInstance::new(dur, wait, active, ts)),
            (WinOp::Max, Type::Float(_)) => SlidingWindow::MaxFloat(RealTimeWindowInstance::new(dur, wait, active, ts)),
            (WinOp::Sum, Type::UInt(_)) => {
                SlidingWindow::SumUnsigned(RealTimeWindowInstance::new(dur, wait, active, ts))
            }
            (WinOp::Sum, Type::Int(_)) => SlidingWindow::SumSigned(RealTimeWindowInstance::new(dur, wait, active, ts)),
            (WinOp::Sum, Type::Float(_)) => SlidingWindow::SumFloat(RealTimeWindowInstance::new(dur, wait, active, ts)),
            (WinOp::Sum, Type::Bool) => SlidingWindow::SumBool(RealTimeWindowInstance::new(dur, wait, active, ts)),
            (WinOp::Average, Type::UInt(_)) => {
                SlidingWindow::AvgUnsigned(RealTimeWindowInstance::new(dur, wait, active, ts))
            }
            (WinOp::Average, Type::Int(_)) => {
                SlidingWindow::AvgSigned(RealTimeWindowInstance::new(dur, wait, active, ts))
            }
            (WinOp::Average, Type::Float(_)) => {
                SlidingWindow::AvgFloat(RealTimeWindowInstance::new(dur, wait, active, ts))
            }
            (WinOp::Integral, _) => SlidingWindow::Integral(RealTimeWindowInstance::new(dur, wait, active, ts)),
            (WinOp::Conjunction, Type::Bool) => {
                SlidingWindow::Conjunction(RealTimeWindowInstance::new(dur, wait, active, ts))
            }
            (WinOp::Disjunction, Type::Bool) => {
                SlidingWindow::Disjunction(RealTimeWindowInstance::new(dur, wait, active, ts))
            }
            (WinOp::Last, Type::Int(_)) => SlidingWindow::LastSigned(RealTimeWindowInstance::new(dur, wait, active, ts)),
            (WinOp::Last, Type::UInt(_)) => SlidingWindow::LastUnsigned(RealTimeWindowInstance::new(dur, wait, active, ts)),
            (WinOp::Last, Type::Float(_)) => SlidingWindow::LastFloat(RealTimeWindowInstance::new(dur, wait, active, ts)),
            (WinOp::NthPercentile(x), Type::Int(_)) => {
                SlidingWindow::PercentileSigned(x, RealTimeWindowInstance::new(dur, wait, active, ts))
            }
            (WinOp::NthPercentile(x), Type::UInt(_)) => {
                SlidingWindow::PercentileUnsigned(x, RealTimeWindowInstance::new(dur, wait, active, ts))
            }
            (WinOp::NthPercentile(x), Type::Float(_)) => {
                SlidingWindow::PercentileFloat(x, RealTimeWindowInstance::new(dur, wait, active, ts))
            }
            (WinOp::Variance, Type::Float(_)) => SlidingWindow::Variance(RealTimeWindowInstance::new(dur, wait, active, ts)),
            (WinOp::StandardDeviation, Type::Float(_)) => {
                SlidingWindow::StandardDeviation(RealTimeWindowInstance::new(dur, wait, active, ts))
            }
            (WinOp::Covariance, Type::Float(_)) => SlidingWindow::Covariance(RealTimeWindowInstance::new(dur, wait, active, ts)),
            (_, Type::Option(t)) => SlidingWindow::from_sliding(dur, wait, op, ts, t, active),
            _ => unimplemented!(),
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
            (WinOp::Count, _) => SlidingWindow::CountDiscrete(DiscreteWindowInstance::new(size, wait, active, ts)),
            (WinOp::Min, Type::UInt(_)) => {
                SlidingWindow::MinUnsignedDiscrete(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::Min, Type::Int(_)) => {
                SlidingWindow::MinSignedDiscrete(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::Min, Type::Float(_)) => {
                SlidingWindow::MinFloatDiscrete(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::Max, Type::UInt(_)) => {
                SlidingWindow::MaxUnsignedDiscrete(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::Max, Type::Int(_)) => {
                SlidingWindow::MaxSignedDiscrete(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::Max, Type::Float(_)) => {
                SlidingWindow::MaxFloatDiscrete(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::Sum, Type::UInt(_)) => {
                SlidingWindow::SumUnsignedDiscrete(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::Sum, Type::Int(_)) => {
                SlidingWindow::SumSignedDiscrete(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::Sum, Type::Float(_)) => {
                SlidingWindow::SumFloatDiscrete(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::Sum, Type::Bool) => {
                SlidingWindow::SumBoolDiscrete(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::Average, Type::UInt(_)) => {
                SlidingWindow::AvgUnsignedDiscrete(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::Average, Type::Int(_)) => {
                SlidingWindow::AvgSignedDiscrete(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::Average, Type::Float(_)) => {
                SlidingWindow::AvgFloatDiscrete(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::Integral, _) => {
                SlidingWindow::IntegralDiscrete(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::Conjunction, Type::Bool) => {
                SlidingWindow::ConjunctionDiscrete(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::Disjunction, Type::Bool) => {
                SlidingWindow::DisjunctionDiscrete(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::Last, Type::Int(_)) => {
                SlidingWindow::LastDiscreteSigned(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::Last, Type::UInt(_)) => {
                SlidingWindow::LastDiscreteUnsigned(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::Last, Type::Float(_)) => {
                SlidingWindow::LastDiscreteFloat(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::NthPercentile(x), Type::Int(_)) => {
                SlidingWindow::PercentileDiscreteSigned(x, DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::NthPercentile(x), Type::UInt(_)) => {
                SlidingWindow::PercentileDiscreteUnsigned(x, DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::NthPercentile(x), Type::Float(_)) => {
                SlidingWindow::PercentileDiscreteFloat(x, DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::Variance, Type::Float(_)) => {
                SlidingWindow::VarianceDiscrete(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::StandardDeviation, Type::Float(_)) => {
                SlidingWindow::StandardDeviationDiscrete(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (WinOp::Covariance, Type::Float(_)) => {
                SlidingWindow::CovarianceDiscrete(DiscreteWindowInstance::new(size, wait, active, ts))
            }
            (_, Type::Option(t)) => SlidingWindow::from_discrete(size, wait, op, ts, t, active),
            _ => unimplemented!(),
        }
    }

    /// Updates the buckets of a sliding window instance with the given timestamp:
    /// # Arguments:
    /// * 'ts' - the current timestamp of the monitor
    pub(crate) fn update(&mut self, ts: Time) {
        match_and_run!(self, update_buckets, ts);
    }

    /// Computes the current value of a sliding window instance with the given timestamp:
    /// # Arguments:
    /// * 'ts' - the current timestamp of the monitor
    /// Note: You should always call `SlidingWindow::update` before calling `SlidingWindow::get_value()`!
    pub(crate) fn get_value(&self, ts: Time) -> Value {
        match self {
            SlidingWindow::Count(wi) => wi.get_value(ts),
            SlidingWindow::CountDiscrete(wi) => wi.get_value(ts),
            SlidingWindow::MinUnsigned(wi) => wi.get_value(ts),
            SlidingWindow::MinUnsignedDiscrete(wi) => wi.get_value(ts),
            SlidingWindow::MinSigned(wi) => wi.get_value(ts),
            SlidingWindow::MinSignedDiscrete(wi) => wi.get_value(ts),
            SlidingWindow::MinFloat(wi) => wi.get_value(ts),
            SlidingWindow::MinFloatDiscrete(wi) => wi.get_value(ts),
            SlidingWindow::MaxUnsigned(wi) => wi.get_value(ts),
            SlidingWindow::MaxUnsignedDiscrete(wi) => wi.get_value(ts),
            SlidingWindow::MaxSigned(wi) => wi.get_value(ts),
            SlidingWindow::MaxSignedDiscrete(wi) => wi.get_value(ts),
            SlidingWindow::MaxFloat(wi) => wi.get_value(ts),
            SlidingWindow::MaxFloatDiscrete(wi) => wi.get_value(ts),
            SlidingWindow::SumUnsigned(wi) => wi.get_value(ts),
            SlidingWindow::SumUnsignedDiscrete(wi) => wi.get_value(ts),
            SlidingWindow::SumSigned(wi) => wi.get_value(ts),
            SlidingWindow::SumSignedDiscrete(wi) => wi.get_value(ts),
            SlidingWindow::SumFloat(wi) => wi.get_value(ts),
            SlidingWindow::SumFloatDiscrete(wi) => wi.get_value(ts),
            SlidingWindow::SumBool(wi) => wi.get_value(ts),
            SlidingWindow::SumBoolDiscrete(wi) => wi.get_value(ts),
            SlidingWindow::Conjunction(wi) => wi.get_value(ts),
            SlidingWindow::ConjunctionDiscrete(wi) => wi.get_value(ts),
            SlidingWindow::Disjunction(wi) => wi.get_value(ts),
            SlidingWindow::DisjunctionDiscrete(wi) => wi.get_value(ts),
            SlidingWindow::AvgUnsigned(wi) => wi.get_value(ts),
            SlidingWindow::AvgUnsignedDiscrete(wi) => wi.get_value(ts),
            SlidingWindow::AvgSigned(wi) => wi.get_value(ts),
            SlidingWindow::AvgSignedDiscrete(wi) => wi.get_value(ts),
            SlidingWindow::AvgFloat(wi) => wi.get_value(ts),
            SlidingWindow::AvgFloatDiscrete(wi) => wi.get_value(ts),
            SlidingWindow::Integral(wi) => wi.get_value(ts),
            SlidingWindow::IntegralDiscrete(wi) => wi.get_value(ts),
            SlidingWindow::LastSigned(wi) => wi.get_value(ts),
            SlidingWindow::LastUnsigned(wi) => wi.get_value(ts),
            SlidingWindow::LastFloat(wi) => wi.get_value(ts),
            SlidingWindow::LastDiscreteSigned(wi) => wi.get_value(ts),
            SlidingWindow::LastDiscreteUnsigned(wi) => wi.get_value(ts),
            SlidingWindow::LastDiscreteFloat(wi) => wi.get_value(ts),
            SlidingWindow::PercentileSigned(p, wi) => wi.get_value_percentile(ts, *p),
            SlidingWindow::PercentileUnsigned(p, wi) => wi.get_value_percentile(ts, *p),
            SlidingWindow::PercentileFloat(p, wi) => wi.get_value_percentile(ts, *p),
            SlidingWindow::PercentileDiscreteSigned(p, wi) => wi.get_value_percentile(ts, *p),
            SlidingWindow::PercentileDiscreteUnsigned(p, wi) => wi.get_value_percentile(ts, *p),
            SlidingWindow::PercentileDiscreteFloat(p, wi) => wi.get_value_percentile(ts, *p),
            SlidingWindow::Variance(wi) => wi.get_value(ts),
            SlidingWindow::VarianceDiscrete(wi) => wi.get_value(ts),
            SlidingWindow::StandardDeviation(wi) => wi.get_value(ts),
            SlidingWindow::StandardDeviationDiscrete(wi) => wi.get_value(ts),
            SlidingWindow::Covariance(wi) => wi.get_value(ts),
            SlidingWindow::CovarianceDiscrete(wi) => wi.get_value(ts),
        }
    }

    /// Updates the value of the first bucket of a sliding window instance with the current value of the accessed stream:
    /// # Arguments:
    /// * 'v' - the current value of the accessed stream
    /// * 'ts' - the current timestamp of the monitor
    pub(crate) fn accept_value(&mut self, v: Value, ts: Time) {
        match_and_run!(self, accept_value, v, ts);
    }

    /// Clears the current sliding window state
    pub(crate) fn deactivate(&mut self) {
        match_and_run!(self, deactivate);
    }

    /// Returns true if the window instance is currently active. I.e. the target stream instance currently exists.
    pub(crate) fn is_active(&self) -> bool {
        match_and_run!(self, is_active)
    }

    /// Restarts the sliding window
    pub(crate) fn activate(&mut self, ts: Time) {
        match_and_run!(self, activate, ts);
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
                    }
                    Ordering::Greater => period_diff * num_buckets + (self.ix - other.ix),
                }
            }
            Ordering::Equal => match self.ix.cmp(&other.ix) {
                Ordering::Equal => 0,
                Ordering::Less => panic!("`other` bucket is more recent than `self`."),
                Ordering::Greater => self.ix - other.ix,
            },
        }
    }
}

impl<IV: WindowIv> RealTimeWindowInstance<IV> {
    /// Creates a new window instance
    fn new(dur: Duration, wait: bool, active: bool, ts: Time) -> RealTimeWindowInstance<IV> {
        let time_per_bucket = dur / (SIZE as u32);
        let buckets = VecDeque::from(vec![IV::default(ts); SIZE]);
        // last bucket_ix is 1, so we consider all buckets, i.e. from 1 to end and from start to 0,
        // as in use. Whenever we progress by n buckets, we invalidate the pseudo-used ones.
        // This is safe since the value within is the neutral element of the operation.
        RealTimeWindowInstance {
            buckets,
            time_per_bucket,
            start_time: ts,
            last_bucket_ix: BIx::new(0, 0),
            wait,
            wait_duration: dur,
            active,
        }
    }

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
        self.buckets.iter().rev().fold(IV::default(ts), |acc, e| acc + e.clone()).into()
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
            Value::Signed(i) => (i as f64),
            Value::Unsigned(u) => (u as f64),
            Value::Float(f) => (f.into()),
            _ => unreachable!("Type error."),
        };
        Value::Float(NotNan::new(f).unwrap())
    }
}

impl<G: WindowGeneric> WindowInstance<PercentileIv<G>> {
    fn get_value_percentile(&self, ts: Time, percentile: usize) -> Value {
        // Reversal is essential for non-commutative operations.
        if self.wait && ts < self.wait_duration {
            return Value::None;
        }
        self.buckets
            .iter()
            .rev()
            .fold(PercentileIv::default(ts), |acc, e| acc + e.clone())
            .percentile_get_value(percentile)
    }
}
