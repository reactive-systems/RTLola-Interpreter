use super::window::WindowIv;
use super::Value;
use crate::basics::Time;
use crate::storage::window::WindowGeneric;
use crate::storage::window_aggregations::PercentileIv;
use std::collections::VecDeque;

#[derive(Debug)]
pub(crate) struct DiscreteWindowInstance<IV: WindowIv> {
    buckets: VecDeque<IV>,
    next_bucket: usize,
    wait: bool,
    active: bool,
}

impl<IV: WindowIV> DiscreteWindowInstance<IV> {
    /// Creates a new discrete window instance
    pub(crate) fn new(size: usize, wait: bool, active: bool, ts: Time) -> DiscreteWindowInstance<IV> {
        let buckets = VecDeque::from(vec![IV::default(ts); size]);
        DiscreteWindowInstance { buckets, next_bucket: 0, wait, active }
    }

    /// You should always call `WindowInstance::update_buckets` before calling `WindowInstance::get_value()`!
    pub(crate) fn get_value(&self, ts: Time) -> Value {
        if !self.active {
            return IV::default(ts).into();
        }
        let size = self.buckets.len();
        self.buckets
            .iter()
            .cycle()
            .skip(self.next_bucket)
            .take(size)
            .fold(IV::default(ts), |acc, e| acc + e.clone())
            .into()
    }

    pub(crate) fn accept_value(&mut self, v: Value, ts: Time) {
        assert!(self.active);
        let b = self.buckets.get_mut(self.next_bucket).expect("Bug!");
        *b = (v, ts).into(); // TODO: Require add_assign rather than add.
        self.next_bucket = (self.next_bucket + 1) % self.buckets.len();
    }

    pub(crate) fn update_buckets(&mut self, _ts: Time) {}

    /// Clears the current sliding window state
    pub(crate) fn deactivate(&mut self) {
        self.next_bucket = 0;
        self.active = false;
    }

    /// Returns true if the window instance is currently active. I.e. the target stream instance currently exists.
    pub(crate) fn is_active(&self) -> bool {
        self.active
    }

    /// Restarts the sliding window
    pub(crate) fn activate(&mut self, ts: Time) {
        self.buckets = VecDeque::from(vec![IV::default(ts); self.buckets.len()]);
        self.active = true;
    }
}

impl<G: WindowGeneric> DiscreteWindowInstance<PercentileIv<G>> {
    pub(crate) fn get_value_percentile(&self, ts: Time, percentile: usize) -> Value {
        let size = self.buckets.len();
        self.buckets
            .iter()
            .cycle()
            .skip(self.next_bucket)
            .take(size)
            .fold(PercentileIv::default(ts), |acc, e| acc + e.clone())
            .percentile_get_value(percentile)
    }
}
