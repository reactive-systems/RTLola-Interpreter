use std::marker::PhantomData;
use std::ops::Add;

use ordered_float::NotNan;
use rust_decimal::prelude::*;

use crate::storage::window::{WindowGeneric, WindowIv};
use crate::storage::Value;
use crate::Time;

#[derive(Clone, Debug)]
pub(crate) struct SumIv<G: WindowGeneric> {
    v: Value,
    _marker: PhantomData<G>,
}

impl<G: WindowGeneric> WindowIv for SumIv<G> {
    fn default(time: Time) -> SumIv<G> {
        let v = (G::from_value(Value::Unsigned(0)), time);
        Self::from(v)
    }
}

impl<G: WindowGeneric> From<SumIv<G>> for Value {
    fn from(iv: SumIv<G>) -> Self {
        iv.v
    }
}

impl<G: WindowGeneric> Add for SumIv<G> {
    type Output = SumIv<G>;

    fn add(self, other: SumIv<G>) -> SumIv<G> {
        (self.v + other.v, Time::default()).into() // Timestamp will be discarded, anyway.
    }
}

impl<G: WindowGeneric> From<(Value, Time)> for SumIv<G> {
    fn from(v: (Value, Time)) -> SumIv<G> {
        SumIv {
            v: G::from_value(v.0),
            _marker: PhantomData,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ConjIv {
    res: bool,
}

impl WindowIv for ConjIv {
    fn default(_ts: Time) -> Self {
        true.into()
    }
}

impl From<ConjIv> for Value {
    fn from(iv: ConjIv) -> Self {
        Value::Bool(iv.res)
    }
}

#[allow(clippy::suspicious_arithmetic_impl)]
impl Add for ConjIv {
    type Output = ConjIv;

    fn add(self, other: ConjIv) -> ConjIv {
        (self.res && other.res).into()
    }
}

impl From<(Value, Time)> for ConjIv {
    fn from(v: (Value, Time)) -> ConjIv {
        match v.0 {
            Value::Bool(b) => b.into(),
            _ => unreachable!("Type error."),
        }
    }
}

impl From<bool> for ConjIv {
    fn from(v: bool) -> ConjIv {
        ConjIv { res: v }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct DisjIv {
    res: bool,
}

impl WindowIv for DisjIv {
    fn default(_ts: Time) -> Self {
        false.into()
    }
}

impl From<DisjIv> for Value {
    fn from(iv: DisjIv) -> Self {
        Value::Bool(iv.res)
    }
}

#[allow(clippy::suspicious_arithmetic_impl)]
impl Add for DisjIv {
    type Output = DisjIv;

    fn add(self, other: DisjIv) -> DisjIv {
        (self.res || other.res).into()
    }
}

impl From<(Value, Time)> for DisjIv {
    fn from(v: (Value, Time)) -> DisjIv {
        match v.0 {
            Value::Bool(b) => b.into(),
            _ => unreachable!("Type error."),
        }
    }
}

impl From<bool> for DisjIv {
    fn from(v: bool) -> DisjIv {
        DisjIv { res: v }
    }
}

// TODO: Generic for floats...
#[derive(Clone, Debug)]
pub(crate) struct AvgIv<G: WindowGeneric> {
    sum: Value,
    num: u64,
    _marker: PhantomData<G>,
}

impl<G: WindowGeneric> WindowIv for AvgIv<G> {
    fn default(_time: Time) -> AvgIv<G> {
        AvgIv {
            sum: Value::None,
            num: 0,
            _marker: PhantomData,
        }
    }
}

impl<G: WindowGeneric> From<AvgIv<G>> for Value {
    fn from(iv: AvgIv<G>) -> Value {
        match iv.sum {
            Value::None => Value::None,
            Value::Unsigned(u) => Value::Unsigned(u / iv.num),
            Value::Signed(u) => Value::Signed(u / iv.num as i64),
            Value::Float(u) => Value::Float(u / iv.num as f64),
            _ => unreachable!("Type error."),
        }
    }
}

impl<G: WindowGeneric> Add for AvgIv<G> {
    type Output = AvgIv<G>;

    fn add(self, other: AvgIv<G>) -> AvgIv<G> {
        match (&self.sum, &other.sum) {
            (Value::None, Value::None) => Self::default(Time::default()),
            (_, Value::None) => self,
            (Value::None, _) => other,
            _ => {
                let sum = self.sum + other.sum;
                let num = self.num + other.num;
                AvgIv {
                    sum,
                    num,
                    _marker: PhantomData,
                }
            },
        }
    }
}

impl<G: WindowGeneric> From<(Value, Time)> for AvgIv<G> {
    fn from(v: (Value, Time)) -> AvgIv<G> {
        AvgIv {
            sum: G::from_value(v.0),
            num: 1u64,
            _marker: PhantomData,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct IntegralIv {
    volume: Decimal,
    end_value: Decimal,
    end_time: Time,
    start_value: Decimal,
    start_time: Time,
    valid: bool,
}

impl WindowIv for IntegralIv {
    fn default(time: Time) -> IntegralIv {
        IntegralIv {
            volume: Decimal::zero(),
            end_value: Decimal::zero(),
            end_time: time,
            start_value: Decimal::zero(),
            start_time: time,
            valid: false,
        }
    }
}

impl From<IntegralIv> for Value {
    fn from(iv: IntegralIv) -> Value {
        Value::try_from(iv.volume).unwrap()
    }
}

#[allow(clippy::suspicious_arithmetic_impl)]
impl Add for IntegralIv {
    type Output = IntegralIv;

    fn add(self, other: IntegralIv) -> IntegralIv {
        match (self.valid, other.valid) {
            (false, false) => return self,
            (false, true) => return other,
            (true, false) => return self,
            (true, true) => {},
        }

        let start_volume = self.volume + other.volume;
        assert!(other.start_time >= self.end_time, "Time does not behave monotonically!");
        let time_diff_dur = other.start_time - self.end_time;
        let time_diff = (Decimal::from(time_diff_dur.as_secs()))
            + (Decimal::from(time_diff_dur.subsec_nanos()) / Decimal::from(100_000_000));
        let value_sum = other.start_value + self.end_value;

        let additional_volume = value_sum * time_diff / Decimal::from(2);

        let volume = start_volume + additional_volume;
        let end_value = other.end_value;
        let end_time = other.end_time;
        let start_value = self.start_value;
        let start_time = self.start_time;

        IntegralIv {
            volume,
            end_value,
            end_time,
            start_value,
            start_time,
            valid: true,
        }
    }
}

impl From<(Value, Time)> for IntegralIv {
    fn from(v: (Value, Time)) -> IntegralIv {
        let f = match v.0 {
            Value::Signed(i) => Decimal::from(i),
            Value::Unsigned(u) => Decimal::from(u),
            Value::Float(f) => Decimal::from_f64(f.into()).unwrap(),
            _ => unreachable!("Type error."),
        };
        IntegralIv {
            volume: Decimal::zero(),
            end_value: f,
            end_time: v.1,
            start_value: f,
            start_time: v.1,
            valid: true,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CountIv(u64);

impl WindowIv for CountIv {
    fn default(_time: Time) -> CountIv {
        CountIv(0)
    }
}

impl From<CountIv> for Value {
    fn from(iv: CountIv) -> Value {
        Value::Unsigned(iv.0)
    }
}

impl Add for CountIv {
    type Output = CountIv;

    fn add(self, other: CountIv) -> CountIv {
        CountIv(self.0 + other.0)
    }
}

impl From<(Value, Time)> for CountIv {
    fn from(_v: (Value, Time)) -> CountIv {
        CountIv(1)
    }
}

//////////////////// MIN/MAX ////////////////////

#[derive(Clone, Debug)]
pub(crate) struct MaxIv<G: WindowGeneric> {
    max: Value,
    _marker: PhantomData<G>,
}

impl<G: WindowGeneric> WindowIv for MaxIv<G> {
    fn default(_ts: Time) -> MaxIv<G> {
        Self::from((Value::None, Time::default()))
    }
}

impl<G: WindowGeneric> From<MaxIv<G>> for Value {
    fn from(iv: MaxIv<G>) -> Value {
        iv.max
    }
}

impl<G: WindowGeneric> Add for MaxIv<G> {
    type Output = MaxIv<G>;

    fn add(self, other: MaxIv<G>) -> MaxIv<G> {
        let max = match (self.max, other.max) {
            (Value::None, Value::None) => Value::None,
            (Value::None, rhs) => rhs,
            (lhs, Value::None) => lhs,
            (Value::Unsigned(lhs), Value::Unsigned(rhs)) => Value::Unsigned(lhs.max(rhs)),
            (Value::Signed(lhs), Value::Signed(rhs)) => Value::Signed(lhs.max(rhs)),
            (Value::Float(lhs), Value::Float(rhs)) => Value::Float(lhs.max(rhs)),
            _ => unreachable!("mixed types in sliding window aggregation"),
        };
        MaxIv {
            max,
            _marker: PhantomData,
        }
    }
}

impl<G: WindowGeneric> From<(Value, Time)> for MaxIv<G> {
    fn from(v: (Value, Time)) -> MaxIv<G> {
        MaxIv {
            max: v.0,
            _marker: PhantomData,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct MinIv<G: WindowGeneric> {
    min: Value,
    _marker: PhantomData<G>,
}

impl<G: WindowGeneric> WindowIv for MinIv<G> {
    fn default(_ts: Time) -> MinIv<G> {
        Self::from((Value::None, Time::default()))
    }
}

impl<G: WindowGeneric> From<MinIv<G>> for Value {
    fn from(iv: MinIv<G>) -> Value {
        iv.min
    }
}

impl<G: WindowGeneric> Add for MinIv<G> {
    type Output = MinIv<G>;

    fn add(self, other: MinIv<G>) -> MinIv<G> {
        let min = match (self.min, other.min) {
            (Value::None, Value::None) => Value::None,
            (Value::None, rhs) => rhs,
            (lhs, Value::None) => lhs,
            (Value::Unsigned(lhs), Value::Unsigned(rhs)) => Value::Unsigned(lhs.min(rhs)),
            (Value::Signed(lhs), Value::Signed(rhs)) => Value::Signed(lhs.min(rhs)),
            (Value::Float(lhs), Value::Float(rhs)) => Value::Float(lhs.min(rhs)),
            _ => unreachable!("mixed types in sliding window aggregation"),
        };
        MinIv {
            min,
            _marker: PhantomData,
        }
    }
}

impl<G: WindowGeneric> From<(Value, Time)> for MinIv<G> {
    fn from(v: (Value, Time)) -> MinIv<G> {
        MinIv {
            min: v.0,
            _marker: PhantomData,
        }
    }
}

//////////////////////////////////////////////
//////////////////// LAST ////////////////////
//////////////////////////////////////////////

#[derive(Clone, Debug)]
pub(crate) struct LastIv<G: WindowGeneric> {
    val: Value,
    ts: Time,
    _marker: PhantomData<G>,
}

impl<G: WindowGeneric> WindowIv for LastIv<G> {
    fn default(ts: Time) -> LastIv<G> {
        LastIv {
            val: Value::None,
            ts,
            _marker: PhantomData,
        }
    }
}

impl<G: WindowGeneric> From<LastIv<G>> for Value {
    fn from(iv: LastIv<G>) -> Value {
        iv.val
    }
}

impl<G: WindowGeneric> From<(Value, Time)> for LastIv<G> {
    fn from(v: (Value, Time)) -> LastIv<G> {
        LastIv {
            val: v.0,
            ts: v.1,
            _marker: PhantomData,
        }
    }
}

impl<G: WindowGeneric> Add for LastIv<G> {
    type Output = LastIv<G>;

    fn add(self, other: LastIv<G>) -> LastIv<G> {
        let (val, ts) = match (self.val, self.ts, other.val, other.ts) {
            (Value::None, _, Value::None, _) => (Value::None, Time::default()),
            (Value::None, _, rhs, r_ts) => (rhs, r_ts),
            (lhs, l_ts, Value::None, _) => (lhs, l_ts),
            (Value::Unsigned(lhs), l_ts, Value::Unsigned(rhs), r_ts) => {
                if l_ts > r_ts {
                    (Value::Unsigned(lhs), l_ts)
                } else {
                    (Value::Unsigned(rhs), r_ts)
                }
            },
            (Value::Signed(lhs), l_ts, Value::Signed(rhs), r_ts) => {
                if l_ts > r_ts {
                    (Value::Signed(lhs), l_ts)
                } else {
                    (Value::Signed(rhs), r_ts)
                }
            },
            (Value::Float(lhs), l_ts, Value::Float(rhs), r_ts) => {
                if l_ts > r_ts {
                    (Value::Float(lhs), l_ts)
                } else {
                    (Value::Float(rhs), r_ts)
                }
            },
            _ => unreachable!("mixed types in sliding window aggregation"),
        };
        LastIv {
            val,
            ts,
            _marker: PhantomData,
        }
    }
}

///////////////////////////////////////////////
///////////////// Percentile //////////////////
//////////////////////////////////////////////

#[derive(Clone, Debug)]
pub(crate) struct PercentileIv<G: WindowGeneric> {
    values: Vec<Value>,
    count: usize,
    _marker: PhantomData<G>,
}

impl<G: WindowGeneric> WindowIv for PercentileIv<G> {
    fn default(_ts: Time) -> PercentileIv<G> {
        PercentileIv {
            values: vec![],
            count: 0,
            _marker: PhantomData,
        }
    }
}

impl<G: WindowGeneric> PercentileIv<G> {
    pub(crate) fn percentile_get_value(self, percentile: u8) -> Value {
        let idx: f32 = self.count as f32 * (percentile as f32 / 100.0);
        let int_idx = (idx.ceil() as usize) - 1;
        if self.values.is_empty() {
            return Value::None;
        }
        let PercentileIv {
            mut values,
            count: _,
            _marker: _,
        } = self;
        values.sort_unstable_by(|a, b| {
            match (a, b) {
                (Value::Signed(x), Value::Signed(y)) => x.cmp(y),
                (Value::Unsigned(x), Value::Unsigned(y)) => x.cmp(y),
                (Value::Float(x), Value::Float(y)) => x.partial_cmp(y).unwrap(),
                _ => unimplemented!("only primitive types implemented for percentile"),
            }
        });
        let values = values;
        let v_idx = values[int_idx].clone();

        let denominator = match &values[0] {
            Value::Unsigned(_) => Value::Unsigned(2),
            Value::Signed(_) => Value::Signed(2),
            Value::Float(_) => Value::try_from(2.0).unwrap(),
            _ => unreachable!("Type error."),
        };

        if idx.fract() > 0.0 {
            v_idx
        } else {
            (v_idx + values[int_idx + 1].clone()) / denominator
        }
    }
}

impl<G: WindowGeneric> From<PercentileIv<G>> for Value {
    fn from(_iv: PercentileIv<G>) -> Value {
        panic!("for percentile windows, call percentile_get_value(usize) instead")
    }
}

impl<G: WindowGeneric> From<(Value, Time)> for PercentileIv<G> {
    fn from(v: (Value, Time)) -> PercentileIv<G> {
        let (values, count) = if matches!(v.0, Value::None) {
            (vec![], 0)
        } else {
            (vec![v.0], 1)
        };
        PercentileIv {
            values,
            count,
            _marker: PhantomData,
        }
    }
}

impl<G: WindowGeneric> Add for PercentileIv<G> {
    type Output = PercentileIv<G>;

    fn add(self, other: PercentileIv<G>) -> PercentileIv<G> {
        let PercentileIv {
            values,
            count,
            _marker: _,
        } = self;
        let PercentileIv {
            values: o_values,
            count: o_count,
            _marker: _,
        } = other;
        //TODO MERGE - would save sorting in get_value
        let values = values.into_iter().chain(o_values).collect::<Vec<Value>>();
        let count = count + o_count;
        PercentileIv {
            values,
            count,
            _marker: PhantomData,
        }
    }
}

///////////////////////////////////////////////
//////////////// SD/Variance //////////////////
///////////////////////////////////////////////

#[derive(Clone, Debug)]
pub(crate) struct VarianceIv {
    count: usize,
    m_2: Decimal,
    sum: Decimal,
}

impl WindowIv for VarianceIv {
    fn default(_ts: Time) -> VarianceIv {
        VarianceIv {
            count: 0,
            m_2: Decimal::zero(),
            sum: Decimal::zero(),
        }
    }
}

impl From<VarianceIv> for Value {
    fn from(iv: VarianceIv) -> Value {
        if iv.count == 0 {
            return Value::Float(NotNan::from(0));
        }
        Value::try_from(iv.m_2 / Decimal::from(iv.count)).expect("")
    }
}

impl From<(Value, Time)> for VarianceIv {
    fn from(v: (Value, Time)) -> VarianceIv {
        VarianceIv {
            count: 1,
            m_2: 0.0.try_into().unwrap(),
            sum: v.0.try_into().unwrap(),
        }
    }
}

// This is baded on: https://en.wikipedia.org/wiki/Algorithms_for_calculating_variance#Parallel_algorithm
impl Add for VarianceIv {
    type Output = VarianceIv;

    fn add(self, other: VarianceIv) -> VarianceIv {
        if self.count == 0 {
            return other;
        }
        if other.count == 0 {
            return self;
        }

        let VarianceIv { count, m_2: var, sum } = self;

        let VarianceIv {
            count: o_count,
            m_2: o_var,
            sum: o_sum,
        } = other;

        let mean_diff = (o_sum / (Decimal::from(o_count))) - (sum / Decimal::from(count));

        let new_var =
            var + o_var + mean_diff * mean_diff * (Decimal::from(count * o_count)) / (Decimal::from(count + o_count));

        let new_count = count + o_count;
        VarianceIv {
            count: new_count,
            m_2: new_var,
            sum: sum + o_sum,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SdIv {
    viv: VarianceIv,
}

impl WindowIv for SdIv {
    fn default(ts: Time) -> SdIv {
        SdIv {
            viv: VarianceIv::default(ts),
        }
    }
}

impl From<SdIv> for Value {
    fn from(iv: SdIv) -> Value {
        let v: Value = iv.viv.into();
        v.pow(Value::try_from(0.5).unwrap())
    }
}

impl From<(Value, Time)> for SdIv {
    fn from(v: (Value, Time)) -> SdIv {
        let viv = VarianceIv::from(v);
        SdIv { viv }
    }
}

impl Add for SdIv {
    type Output = SdIv;

    fn add(self, other: SdIv) -> SdIv {
        SdIv {
            viv: self.viv + other.viv,
        }
    }
}

///////////////////////////////////////////////
//////////////// Covariance //////////////////
///////////////////////////////////////////////
// See https://en.wikipedia.org/wiki/Algorithms_for_calculating_variance#Online for refernce
#[derive(Clone, Debug)]
pub(crate) struct CovIv {
    count: usize,
    co_moment: Decimal,
    sum_x: Decimal,
    sum_y: Decimal,
}

impl WindowIv for CovIv {
    fn default(_ts: Time) -> CovIv {
        CovIv {
            count: 0,
            co_moment: Decimal::zero(),
            sum_x: Decimal::zero(),
            sum_y: Decimal::zero(),
        }
    }
}

impl From<CovIv> for Value {
    fn from(iv: CovIv) -> Value {
        if iv.count == 0 {
            return Value::None;
        }
        Value::try_from(iv.co_moment / Decimal::from(iv.count)).unwrap()
    }
}

impl From<(Value, Time)> for CovIv {
    fn from(v: (Value, Time)) -> CovIv {
        let (x, y) = match v.0 {
            Value::Tuple(ref inner_tup) => (inner_tup[0].clone(), inner_tup[1].clone()),
            _ => unreachable!("covariance expects tuple input, ensured by type checker"),
        };
        CovIv {
            count: 1,
            co_moment: Decimal::zero(),
            sum_x: x.try_into().unwrap(),
            sum_y: y.try_into().unwrap(),
        }
    }
}

impl Add for CovIv {
    type Output = CovIv;

    fn add(self, other: CovIv) -> CovIv {
        if self.count == 0 {
            return other;
        }
        if other.count == 0 {
            return self;
        }

        let CovIv {
            count,
            co_moment,
            sum_x,
            sum_y,
        } = self;

        let CovIv {
            count: o_count,
            co_moment: o_co_moment,
            sum_x: o_sum_x,
            sum_y: o_sum_y,
        } = other;

        let new_count = count + o_count;
        let count = Decimal::from(count);
        let o_count = Decimal::from(o_count);

        let mean_diff_x = (o_sum_x / o_count) - (sum_x / count);
        let mean_diff_y = (o_sum_y / o_count) - (sum_y / count);

        let new_co_moment =
            co_moment + o_co_moment + mean_diff_x * mean_diff_y * (count * o_count / Decimal::from(new_count));

        CovIv {
            count: new_count,
            co_moment: new_co_moment,
            sum_x: sum_x + o_sum_x,
            sum_y: sum_y + o_sum_y,
        }
    }
}
