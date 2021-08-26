use crate::basics::Time;
use crate::storage::{
    window::{WindowFloat, WindowGeneric, WindowIV},
    Value,
};
use std::marker::PhantomData;
use std::ops::Add;

#[derive(Clone, Debug)]
pub(crate) struct SumIV<G: WindowGeneric> {
    v: Value,
    _marker: PhantomData<G>,
}

impl<G: WindowGeneric> WindowIV for SumIV<G> {
    fn default(time: Time) -> SumIV<G> {
        let v = (G::from_value(Value::Unsigned(0)), time);
        Self::from(v)
    }
}

impl<G: WindowGeneric> From<SumIV<G>> for Value {
    fn from(iv: SumIV<G>) -> Self {
        iv.v
    }
}

impl<G: WindowGeneric> Add for SumIV<G> {
    type Output = SumIV<G>;
    fn add(self, other: SumIV<G>) -> SumIV<G> {
        (self.v + other.v, Time::default()).into() // Timestamp will be discarded, anyway.
    }
}

impl<G: WindowGeneric> From<(Value, Time)> for SumIV<G> {
    fn from(v: (Value, Time)) -> SumIV<G> {
        SumIV { v: G::from_value(v.0), _marker: PhantomData }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ConjIV {
    res: bool,
}

impl WindowIV for ConjIV {
    fn default(_ts: Time) -> Self {
        true.into()
    }
}

impl From<ConjIV> for Value {
    fn from(iv: ConjIV) -> Self {
        Value::Bool(iv.res)
    }
}

#[allow(clippy::suspicious_arithmetic_impl)]
impl Add for ConjIV {
    type Output = ConjIV;
    fn add(self, other: ConjIV) -> ConjIV {
        (self.res && other.res).into()
    }
}

impl From<(Value, Time)> for ConjIV {
    fn from(v: (Value, Time)) -> ConjIV {
        match v.0 {
            Value::Bool(b) => b.into(),
            _ => unreachable!("Type error."),
        }
    }
}

impl From<bool> for ConjIV {
    fn from(v: bool) -> ConjIV {
        ConjIV { res: v }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct DisjIV {
    res: bool,
}

impl WindowIV for DisjIV {
    fn default(_ts: Time) -> Self {
        false.into()
    }
}

impl From<DisjIV> for Value {
    fn from(iv: DisjIV) -> Self {
        Value::Bool(iv.res)
    }
}

#[allow(clippy::suspicious_arithmetic_impl)]
impl Add for DisjIV {
    type Output = DisjIV;
    fn add(self, other: DisjIV) -> DisjIV {
        (self.res || other.res).into()
    }
}

impl From<(Value, Time)> for DisjIV {
    fn from(v: (Value, Time)) -> DisjIV {
        match v.0 {
            Value::Bool(b) => b.into(),
            _ => unreachable!("Type error."),
        }
    }
}

impl From<bool> for DisjIV {
    fn from(v: bool) -> DisjIV {
        DisjIV { res: v }
    }
}

// TODO: Generic for floats...
#[derive(Clone, Debug)]
pub(crate) struct AvgIV<G: WindowGeneric> {
    sum: Value,
    num: u64,
    _marker: PhantomData<G>,
}

impl<G: WindowGeneric> WindowIV for AvgIV<G> {
    fn default(_time: Time) -> AvgIV<G> {
        AvgIV { sum: Value::None, num: 0, _marker: PhantomData }
    }
}

impl<G: WindowGeneric> From<AvgIV<G>> for Value {
    fn from(iv: AvgIV<G>) -> Value {
        match iv.sum {
            Value::None => Value::None,
            Value::Unsigned(u) => Value::Unsigned(u / iv.num),
            Value::Signed(u) => Value::Signed(u / iv.num as i64),
            Value::Float(u) => Value::Float(u / iv.num as f64),
            _ => unreachable!("Type error."),
        }
    }
}

impl<G: WindowGeneric> Add for AvgIV<G> {
    type Output = AvgIV<G>;
    fn add(self, other: AvgIV<G>) -> AvgIV<G> {
        match (&self.sum, &other.sum) {
            (Value::None, Value::None) => Self::default(Time::default()),
            (_, Value::None) => self,
            (Value::None, _) => other,
            _ => {
                let sum = self.sum + other.sum;
                let num = self.num + other.num;
                AvgIV { sum, num, _marker: PhantomData }
            }
        }
    }
}

impl<G: WindowGeneric> From<(Value, Time)> for AvgIV<G> {
    fn from(v: (Value, Time)) -> AvgIV<G> {
        AvgIV { sum: G::from_value(v.0), num: 1u64, _marker: PhantomData }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct IntegralIV {
    volume: f64,
    end_value: f64,
    end_time: Time,
    start_value: f64,
    start_time: Time,
    valid: bool,
}

impl WindowIV for IntegralIV {
    fn default(time: Time) -> IntegralIV {
        IntegralIV { volume: 0f64, end_value: 0f64, end_time: time, start_value: 0f64, start_time: time, valid: false }
    }
}

impl From<IntegralIV> for Value {
    fn from(iv: IntegralIV) -> Value {
        Value::new_float(iv.volume)
    }
}

#[allow(clippy::suspicious_arithmetic_impl)]
impl Add for IntegralIV {
    type Output = IntegralIV;
    fn add(self, other: IntegralIV) -> IntegralIV {
        match (self.valid, other.valid) {
            (false, false) => return self,
            (false, true) => return other,
            (true, false) => return self,
            (true, true) => {}
        }

        let start_volume = self.volume + other.volume;

        assert!(other.start_time >= self.end_time, "Time does not behave monotonically!");
        let time_diff = other.start_time - self.end_time;
        let time_diff_secs = (time_diff.as_secs() as f64) + (f64::from(time_diff.subsec_nanos())) / (100_000_000f64);
        let time_diff = time_diff_secs;
        let value_sum = other.start_value + self.end_value;

        let additional_volume = value_sum * time_diff / 2f64;

        let volume = start_volume + additional_volume;
        let end_value = other.end_value;
        let end_time = other.end_time;
        let start_value = self.start_value;
        let start_time = self.start_time;

        IntegralIV { volume, end_value, end_time, start_value, start_time, valid: true }
    }
}

impl From<(Value, Time)> for IntegralIV {
    fn from(v: (Value, Time)) -> IntegralIV {
        let f = match v.0 {
            Value::Signed(i) => (i as f64),
            Value::Unsigned(u) => (u as f64),
            Value::Float(f) => (f.into()),
            _ => unreachable!("Type error."),
        };
        IntegralIV { volume: 0f64, end_value: f, end_time: v.1, start_value: f, start_time: v.1, valid: true }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CountIV(u64);

impl WindowIV for CountIV {
    fn default(_time: Time) -> CountIV {
        CountIV(0)
    }
}

impl From<CountIV> for Value {
    fn from(iv: CountIV) -> Value {
        Value::Unsigned(iv.0)
    }
}

impl Add for CountIV {
    type Output = CountIV;
    fn add(self, other: CountIV) -> CountIV {
        CountIV(self.0 + other.0)
    }
}

impl From<(Value, Time)> for CountIV {
    fn from(_v: (Value, Time)) -> CountIV {
        CountIV(1)
    }
}

//////////////////// MIN/MAX ////////////////////

#[derive(Clone, Debug)]
pub(crate) struct MaxIV<G: WindowGeneric> {
    max: Value,
    _marker: PhantomData<G>,
}

impl<G: WindowGeneric> WindowIV for MaxIV<G> {
    fn default(_ts: Time) -> MaxIV<G> {
        Self::from((Value::None, Time::default()))
    }
}

impl<G: WindowGeneric> From<MaxIV<G>> for Value {
    fn from(iv: MaxIV<G>) -> Value {
        iv.max
    }
}

impl<G: WindowGeneric> Add for MaxIV<G> {
    type Output = MaxIV<G>;
    fn add(self, other: MaxIV<G>) -> MaxIV<G> {
        let max = match (self.max, other.max) {
            (Value::None, Value::None) => Value::None,
            (Value::None, rhs) => rhs,
            (lhs, Value::None) => lhs,
            (Value::Unsigned(lhs), Value::Unsigned(rhs)) => Value::Unsigned(lhs.max(rhs)),
            (Value::Signed(lhs), Value::Signed(rhs)) => Value::Signed(lhs.max(rhs)),
            (Value::Float(lhs), Value::Float(rhs)) => Value::Float(lhs.max(rhs)),
            _ => unreachable!("Mixed types in sliding window aggregation."),
        };
        MaxIV { max, _marker: PhantomData }
    }
}

impl<G: WindowGeneric> From<(Value, Time)> for MaxIV<G> {
    fn from(v: (Value, Time)) -> MaxIV<G> {
        MaxIV { max: v.0, _marker: PhantomData }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct MinIV<G: WindowGeneric> {
    min: Value,
    _marker: PhantomData<G>,
}

impl<G: WindowGeneric> WindowIV for MinIV<G> {
    fn default(_ts: Time) -> MinIV<G> {
        Self::from((Value::None, Time::default()))
    }
}

impl<G: WindowGeneric> From<MinIV<G>> for Value {
    fn from(iv: MinIV<G>) -> Value {
        iv.min
    }
}

impl<G: WindowGeneric> Add for MinIV<G> {
    type Output = MinIV<G>;
    fn add(self, other: MinIV<G>) -> MinIV<G> {
        let min = match (self.min, other.min) {
            (Value::None, Value::None) => Value::None,
            (Value::None, rhs) => rhs,
            (lhs, Value::None) => lhs,
            (Value::Unsigned(lhs), Value::Unsigned(rhs)) => Value::Unsigned(lhs.min(rhs)),
            (Value::Signed(lhs), Value::Signed(rhs)) => Value::Signed(lhs.min(rhs)),
            (Value::Float(lhs), Value::Float(rhs)) => Value::Float(lhs.min(rhs)),
            _ => unreachable!("Mixed types in sliding window aggregation."),
        };
        MinIV { min, _marker: PhantomData }
    }
}

impl<G: WindowGeneric> From<(Value, Time)> for MinIV<G> {
    fn from(v: (Value, Time)) -> MinIV<G> {
        MinIV { min: v.0, _marker: PhantomData }
    }
}

//////////////////////////////////////////////
//////////////////// LAST ////////////////////
//////////////////////////////////////////////

#[derive(Clone, Debug)]
pub(crate) struct LastIV<G: WindowGeneric> {
    val: Value,
    ts: Time,
    _marker: PhantomData<G>,
}

impl<G: WindowGeneric> WindowIV for LastIV<G> {
    fn default(ts: Time) -> LastIV<G> {
        LastIV { val: Value::None, ts, _marker: PhantomData }
    }
}

impl<G: WindowGeneric> From<LastIV<G>> for Value {
    fn from(iv: LastIV<G>) -> Value {
        iv.val
    }
}

impl<G: WindowGeneric> From<(Value, Time)> for LastIV<G> {
    fn from(v: (Value, Time)) -> LastIV<G> {
        LastIV { val: v.0, ts: v.1, _marker: PhantomData }
    }
}

impl<G: WindowGeneric> Add for LastIV<G> {
    type Output = LastIV<G>;
    fn add(self, other: LastIV<G>) -> LastIV<G> {
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
            }
            (Value::Signed(lhs), l_ts, Value::Signed(rhs), r_ts) => {
                if l_ts > r_ts {
                    (Value::Signed(lhs), l_ts)
                } else {
                    (Value::Signed(rhs), r_ts)
                }
            }
            (Value::Float(lhs), l_ts, Value::Float(rhs), r_ts) => {
                if l_ts > r_ts {
                    (Value::Float(lhs), l_ts)
                } else {
                    (Value::Float(rhs), r_ts)
                }
            }
            _ => unreachable!("Mixed types in sliding window aggregation."),
        };
        LastIV { val, ts, _marker: PhantomData }
    }
}

///////////////////////////////////////////////
///////////////// Percentile //////////////////
//////////////////////////////////////////////

#[derive(Clone, Debug)]
pub(crate) struct PercentileIV<G: WindowGeneric> {
    values: Vec<Value>,
    count: usize,
    _marker: PhantomData<G>,
}

impl<G: WindowGeneric> WindowIV for PercentileIV<G> {
    fn default(_ts: Time) -> PercentileIV<G> {
        PercentileIV { values: vec![], count: 0, _marker: PhantomData }
    }
}

impl<G: WindowGeneric> PercentileIV<G> {
    pub(crate) fn percentile_get_value(self, percentile: usize) -> Value {
        let idx: f32 = self.count as f32 * (percentile as f32 / 100.0);
        let int_idx = (idx.ceil() as usize) - 1;
        let idx = idx;
        if self.values.is_empty() {
            return Value::None;
        }
        let PercentileIV { mut values, count: _, _marker: _ } = self;
        values.sort_unstable_by(|a, b| match (a, b) {
            (Value::Signed(x), Value::Signed(y)) => x.cmp(&y),
            (Value::Unsigned(x), Value::Unsigned(y)) => x.cmp(&y),
            (Value::Float(x), Value::Float(y)) => x.partial_cmp(&y).unwrap(),
            _ => unimplemented!("only primitive types implemented for percentile"),
        });
        let values = values;
        let v_idx = values[int_idx].clone();

        let denominator = match &values[0] {
            Value::Unsigned(_) => Value::Unsigned(2),
            Value::Signed(_) => Value::Signed(2),
            Value::Float(_) => Value::new_float(2.0),
            _ => unreachable!("Type error."),
        };

        if idx.fract() > 0.0 {
            v_idx
        } else {
            (v_idx + values[int_idx + 1].clone()) / denominator
        }
    }
}

impl<G: WindowGeneric> From<PercentileIV<G>> for Value {
    fn from(_iv: PercentileIV<G>) -> Value {
        panic!("for percentile windows, call percentile_get_value(usize) instead")
    }
}

impl<G: WindowGeneric> From<(Value, Time)> for PercentileIV<G> {
    fn from(v: (Value, Time)) -> PercentileIV<G> {
        let (values, count) = if matches!(v.0, Value::None) { (vec![], 0) } else { (vec![v.0], 1) };
        PercentileIV { values, count, _marker: PhantomData }
    }
}

impl<G: WindowGeneric> Add for PercentileIV<G> {
    type Output = PercentileIV<G>;
    fn add(self, other: PercentileIV<G>) -> PercentileIV<G> {
        let PercentileIV { values, count, _marker: _ } = self;
        let PercentileIV { values: o_values, count: o_count, _marker: _ } = other;
        //TODO MERGE - would save sorting in get_value
        let values = values.into_iter().chain(o_values).collect::<Vec<Value>>();
        let count = count + o_count;
        PercentileIV { values, count, _marker: PhantomData }
    }
}

///////////////////////////////////////////////
//////////////// SD/Variance //////////////////
///////////////////////////////////////////////

#[derive(Clone, Debug)]
pub(crate) struct VarianceIV {
    count: Value,
    var: Value,
    mean: Value,
}

impl WindowIV for VarianceIV {
    fn default(_ts: Time) -> VarianceIV {
        VarianceIV { count: Value::new_float(0.0), var: Value::None, mean: Value::None }
    }
}

impl From<VarianceIV> for Value {
    fn from(iv: VarianceIV) -> Value {
        iv.var / iv.count
    }
}

impl From<(Value, Time)> for VarianceIV {
    fn from(v: (Value, Time)) -> VarianceIV {
        VarianceIV { count: Value::new_float(1.0), var: Value::new_float(0.0), mean: v.0 }
    }
}

impl Add for VarianceIV {
    type Output = VarianceIV;
    fn add(self, other: VarianceIV) -> VarianceIV {
        if self.mean == Value::None {
            return other;
        }
        if other.mean == Value::None {
            return self;
        }

        let VarianceIV { count, var, mean } = self;

        let VarianceIV { count: o_count, var: o_var, mean: o_mean } = other;

        let mean_diff = o_mean - mean.clone();
        let new_var = var
            + o_var
            + (mean_diff.clone())
                * (mean_diff.clone())
                * (count.clone() * o_count.clone() / (count.clone() + o_count.clone()));
        let new_mean = mean + mean_diff * (o_count.clone() / (count.clone() + o_count.clone()));

        let new_count = count + o_count;
        VarianceIV { count: new_count, var: new_var, mean: new_mean }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SDIV {
    viv: VarianceIV,
}

impl WindowIV for SDIV {
    fn default(ts: Time) -> SDIV {
        SDIV { viv: VarianceIV::default(ts) }
    }
}

impl From<SDIV> for Value {
    fn from(iv: SDIV) -> Value {
        let v: Value = iv.viv.into();
        v.pow(Value::new_float(0.5))
    }
}

impl From<(Value, Time)> for SDIV {
    fn from(v: (Value, Time)) -> SDIV {
        let viv = VarianceIV::from(v);
        SDIV { viv }
    }
}

impl Add for SDIV {
    type Output = SDIV;
    fn add(self, other: SDIV) -> SDIV {
        SDIV { viv: self.viv + other.viv }
    }
}

///////////////////////////////////////////////
//////////////// Covariance //////////////////
///////////////////////////////////////////////

//TODO NOT FINAL DO NOT USE
#[derive(Clone, Debug)]
pub(crate) struct CovIV {
    count: Value,
    cov: Value,
    mean_x: Value,
    mean_y: Value,
    avg_iv: AvgIV<WindowFloat>,
}

impl WindowIV for CovIV {
    fn default(ts: Time) -> CovIV {
        CovIV {
            count: Value::new_float(0.0),
            cov: Value::None,
            mean_x: Value::None,
            mean_y: Value::None,
            avg_iv: AvgIV::default(ts),
        }
    }
}

impl From<CovIV> for Value {
    fn from(iv: CovIV) -> Value {
        iv.cov / iv.count
    }
}

impl From<(Value, Time)> for CovIV {
    fn from(v: (Value, Time)) -> CovIV {
        let (x, y) = match v.0 {
            Value::Tuple(ref inner_tup) => (inner_tup[0].clone(), inner_tup[1].clone()),
            _ => unreachable!("covariance expects tuple input"),
        };
        CovIV { count: Value::new_float(1.0), cov: Value::new_float(0.0), mean_x: x, mean_y: y, avg_iv: AvgIV::from(v) }
    }
}

impl Add for CovIV {
    type Output = CovIV;
    fn add(self, other: CovIV) -> CovIV {
        if self.mean_x == Value::None {
            return other;
        }
        if other.mean_x == Value::None {
            return self;
        }

        let CovIV { count, cov: _cov, mean_x, mean_y, avg_iv } = self;

        let CovIV { count: o_count, cov: o_cov, mean_x: o_mean_x, mean_y: _o_mean_y, avg_iv: o_avg_iv } = other;

        unimplemented!("covariance is not yet implemented")
    }
}
