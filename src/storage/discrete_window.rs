use super::window::WindowIV;
use super::Value;
use crate::basics::Time;
use std::collections::VecDeque;

#[derive(Debug)]
pub(crate) struct DiscreteWindowInstance<IV: WindowIV> {
    buckets: VecDeque<IV>,
    next_bucket: usize,
    wait: bool,
}

impl<IV: WindowIV> DiscreteWindowInstance<IV> {
    pub(crate) fn new(size: usize, wait: bool, ts: Time) -> DiscreteWindowInstance<IV> {
        let buckets = VecDeque::from(vec![IV::default(ts); size]);
        DiscreteWindowInstance { buckets, next_bucket: 0, wait }
    }

    /// You should always call `WindowInstance::update_buckets` before calling `WindowInstance::get_value()`!
    pub(crate) fn get_value(&self, ts: Time) -> Value {
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
        let b = self.buckets.get_mut(self.next_bucket).expect("Bug!");
        *b = (v, ts).into(); // TODO: Require add_assign rather than add.
        self.next_bucket = (self.next_bucket + 1) % self.buckets.len();
    }

    pub(crate) fn update_buckets(&mut self, _ts: Time) {}
}
