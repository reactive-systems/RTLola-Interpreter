use std::ops::{Add, Mul};

use num_traits::Zero;
use ordered_float::NotNan;
use rtlola_frontend::mir;
use rtlola_frontend::mir::{InstanceOperation, InstanceSelection, StreamReference, Type, Window};
use rust_decimal::Decimal;

use crate::storage::stores::InstanceCollection;
use crate::storage::InstanceStore;
use crate::Value;

pub(crate) struct InstanceAggregation {
    inner: Box<dyn InstanceAggregationTrait>,
    pub(crate) target: StreamReference,
}

impl From<&mir::InstanceAggregation> for InstanceAggregation {
    fn from(value: &mir::InstanceAggregation) -> Self {
        let inner: Box<dyn InstanceAggregationTrait> = match value.selection {
            InstanceSelection::Fresh => {
                match value.aggr {
                    InstanceOperation::Count => Box::new(FreshAggregation(Count::neutral(value.ty()))),
                    InstanceOperation::Min => Box::new(FreshAggregation(Min::neutral(value.ty()))),
                    InstanceOperation::Max => Box::new(FreshAggregation(Max::neutral(value.ty()))),
                    InstanceOperation::Sum => Box::new(FreshAggregation(Sum::neutral(value.ty()))),
                    InstanceOperation::Product => Box::new(FreshAggregation(Product::neutral(value.ty()))),
                    InstanceOperation::Average => Box::new(FreshAggregation(Avg::neutral(value.ty()))),
                    InstanceOperation::Conjunction => Box::new(FreshAggregation(All::neutral(value.ty()))),
                    InstanceOperation::Disjunction => Box::new(FreshAggregation(Any::neutral(value.ty()))),
                    InstanceOperation::Variance => Box::new(FreshAggregation(Variance::neutral(value.ty()))),
                    InstanceOperation::Covariance => Box::new(FreshAggregation(CoVar::neutral(value.ty()))),
                    InstanceOperation::StandardDeviation => Box::new(FreshAggregation(StdDev::neutral(value.ty()))),
                    InstanceOperation::NthPercentile(pctl) => Box::new(FreshAggregation(Percentile::new(pctl))),
                }
            },
            InstanceSelection::All => {
                match value.aggr {
                    InstanceOperation::Count => Box::new(Incremental(AllAggregation(Count::neutral(value.ty())))),
                    InstanceOperation::Min => Box::new(Total(AllAggregation(Min::neutral(value.ty())))),
                    InstanceOperation::Max => Box::new(Total(AllAggregation(Max::neutral(value.ty())))),
                    InstanceOperation::Sum => Box::new(Incremental(AllAggregation(Sum::neutral(value.ty())))),
                    InstanceOperation::Product => Box::new(Incremental(AllAggregation(Product::neutral(value.ty())))),
                    InstanceOperation::Average => Box::new(Incremental(AllAggregation(Avg::neutral(value.ty())))),
                    InstanceOperation::Conjunction => Box::new(Incremental(AllAggregation(All::neutral(value.ty())))),
                    InstanceOperation::Disjunction => Box::new(Incremental(AllAggregation(Any::neutral(value.ty())))),
                    InstanceOperation::Variance => Box::new(Incremental(AllAggregation(Variance::neutral(value.ty())))),
                    InstanceOperation::Covariance => Box::new(Incremental(AllAggregation(CoVar::neutral(value.ty())))),
                    InstanceOperation::StandardDeviation => {
                        Box::new(Incremental(AllAggregation(StdDev::neutral(value.ty()))))
                    },
                    InstanceOperation::NthPercentile(pctl) => Box::new(Total(AllAggregation(Percentile::new(pctl)))),
                }
            },
        };
        InstanceAggregation {
            inner,
            target: value.target,
        }
    }
}

impl InstanceAggregationTrait for InstanceAggregation {
    fn accept_value(&mut self, value: Value) {
        self.inner.accept_value(value);
    }

    fn remove_value(&mut self, value: Value) {
        self.inner.remove_value(value);
    }

    fn get_value(&self, instances: &InstanceCollection) -> Value {
        self.inner.get_value(instances)
    }
}

pub(crate) trait InstanceAggregationTrait {
    fn accept_value(&mut self, _value: Value) {}

    fn remove_value(&mut self, _value: Value) {}

    fn get_value(&self, instances: &InstanceCollection) -> Value;
}

struct FreshAggregation<OP: TotalOp>(OP);
impl<OP: TotalOp> InstanceAggregationTrait for FreshAggregation<OP> {
    fn get_value(&self, instances: &InstanceCollection) -> Value {
        let fresh = instances.fresh().map(|inst| instances.instance(inst).unwrap());
        self.0.for_instances(fresh)
    }
}

struct AllAggregation<OP: InstanceOp>(OP);

struct Total<OP: TotalOp>(AllAggregation<OP>);
impl<OP: TotalOp> InstanceAggregationTrait for Total<OP> {
    fn get_value(&self, instances: &InstanceCollection) -> Value {
        self.0 .0.for_instances(instances.instances())
    }
}

struct Incremental<OP: IncrementalOp>(AllAggregation<OP>);

impl<OP: IncrementalOp> InstanceAggregationTrait for Incremental<OP> {
    fn accept_value(&mut self, value: Value) {
        self.0 .0.add(value);
    }

    fn remove_value(&mut self, value: Value) {
        self.0 .0.sub(value);
    }

    fn get_value(&self, _instances: &InstanceCollection) -> Value {
        self.0 .0.value()
    }
}

trait InstanceOp {
    /// Returns the neutral element of the operator.
    fn neutral(ty: &Type) -> Self;
}

trait IncrementalOp: InstanceOp {
    /// Add the value to the aggregation
    fn add(&mut self, val: Value);

    /// Subtract the value from the aggregation
    fn sub(&mut self, val: Value);

    /// Returns the value computed by the Op
    fn value(&self) -> Value;
}

trait TotalOp: InstanceOp {
    /// Compute the value given all instances
    fn for_instances<'s>(&'s self, instances: impl IntoIterator<Item = &'s InstanceStore>) -> Value;
}

struct All {
    num_false: usize,
}

impl InstanceOp for All {
    fn neutral(_ty: &Type) -> Self {
        Self { num_false: 0 }
    }
}

impl IncrementalOp for All {
    fn add(&mut self, val: Value) {
        if !<Value as TryInto<bool>>::try_into(val).expect("Value types to be correct") {
            self.num_false += 1;
        }
    }

    fn sub(&mut self, val: Value) {
        if !<Value as TryInto<bool>>::try_into(val).expect("Value types to be correct") {
            self.num_false -= 1;
        }
    }

    fn value(&self) -> Value {
        Value::Bool(self.num_false == 0)
    }
}

impl TotalOp for All {
    fn for_instances<'s>(&'s self, instances: impl IntoIterator<Item = &'s InstanceStore>) -> Value {
        let count = instances
            .into_iter()
            .filter_map(|inst| inst.get_value(0))
            .filter_map(|val| {
                (!<Value as TryInto<bool>>::try_into(val).expect("Value types to be correct")).then_some(true)
            })
            .count();
        Self { num_false: count }.value()
    }
}

struct Any {
    num_true: usize,
}

impl InstanceOp for Any {
    fn neutral(_ty: &Type) -> Self {
        Self { num_true: 0 }
    }
}

impl IncrementalOp for Any {
    fn add(&mut self, val: Value) {
        if <Value as TryInto<bool>>::try_into(val).expect("Value types to be correct") {
            self.num_true += 1;
        }
    }

    fn sub(&mut self, val: Value) {
        if <Value as TryInto<bool>>::try_into(val).expect("Value types to be correct") {
            self.num_true -= 1;
        }
    }

    fn value(&self) -> Value {
        Value::Bool(self.num_true > 0)
    }
}

impl TotalOp for Any {
    fn for_instances<'s>(&'s self, instances: impl IntoIterator<Item = &'s InstanceStore>) -> Value {
        let num_true = instances
            .into_iter()
            .filter_map(|inst| inst.get_value(0))
            .filter_map(|val| {
                <Value as TryInto<bool>>::try_into(val)
                    .expect("Value types to be correct")
                    .then_some(true)
            })
            .count();
        Self { num_true }.value()
    }
}

struct Count {
    res: usize,
}

impl InstanceOp for Count {
    fn neutral(_ty: &Type) -> Self {
        Self { res: 0 }
    }
}

impl IncrementalOp for Count {
    fn add(&mut self, _val: Value) {
        self.res += 1;
    }

    fn sub(&mut self, _val: Value) {
        self.res -= 1;
    }

    fn value(&self) -> Value {
        Value::Unsigned(self.res as u64)
    }
}

impl TotalOp for Count {
    fn for_instances<'s>(&'s self, instances: impl IntoIterator<Item = &'s InstanceStore>) -> Value {
        let res = instances.into_iter().filter_map(|inst| inst.get_value(0)).count();
        Self { res }.value()
    }
}

struct Min {}

impl InstanceOp for Min {
    fn neutral(_ty: &Type) -> Self {
        Self {}
    }
}

impl TotalOp for Min {
    fn for_instances<'s>(&'s self, instances: impl IntoIterator<Item = &'s InstanceStore>) -> Value {
        itertools::min(instances.into_iter().filter_map(|inst| inst.get_value(0))).unwrap_or(Value::None)
    }
}

struct Max {}

impl InstanceOp for Max {
    fn neutral(_ty: &Type) -> Self {
        Self {}
    }
}

impl TotalOp for Max {
    fn for_instances<'s>(&'s self, instances: impl IntoIterator<Item = &'s InstanceStore>) -> Value {
        itertools::max(instances.into_iter().filter_map(|inst| inst.get_value(0))).unwrap_or(Value::None)
    }
}

struct Sum {
    res: Value,
}

impl InstanceOp for Sum {
    fn neutral(ty: &Type) -> Self {
        Self {
            res: Value::from_int(ty, 0),
        }
    }
}

impl TotalOp for Sum {
    fn for_instances<'s>(&'s self, instances: impl IntoIterator<Item = &'s InstanceStore>) -> Value {
        instances
            .into_iter()
            .filter_map(|inst| inst.get_value(0))
            .fold(self.res.for_int(0), <Value as Add>::add)
    }
}

impl IncrementalOp for Sum {
    fn add(&mut self, val: Value) {
        self.res += val;
    }

    fn sub(&mut self, val: Value) {
        self.res -= val;
    }

    fn value(&self) -> Value {
        self.res.clone()
    }
}

struct Product {
    res: Value,
}

impl InstanceOp for Product {
    fn neutral(ty: &Type) -> Self {
        Self {
            res: Value::from_int(ty, 1),
        }
    }
}

impl TotalOp for Product {
    fn for_instances<'s>(&'s self, instances: impl IntoIterator<Item = &'s InstanceStore>) -> Value {
        instances
            .into_iter()
            .filter_map(|inst| inst.get_value(0))
            .fold(self.res.for_int(1), <Value as Mul>::mul)
    }
}

impl IncrementalOp for Product {
    fn add(&mut self, val: Value) {
        self.res *= val;
    }

    fn sub(&mut self, val: Value) {
        self.res /= val;
    }

    fn value(&self) -> Value {
        self.res.clone()
    }
}

struct Avg {
    sum: Value,
    count: usize,
}

impl InstanceOp for Avg {
    fn neutral(ty: &Type) -> Self {
        Self {
            sum: Value::from_int(ty, 0),
            count: 0,
        }
    }
}

impl TotalOp for Avg {
    fn for_instances<'s>(&'s self, instances: impl IntoIterator<Item = &'s InstanceStore>) -> Value {
        let (sum, count) = instances
            .into_iter()
            .filter_map(|inst| inst.get_value(0))
            .fold((self.sum.for_int(0), 0), |(sum, count), val| (sum + val, count + 1));
        Self { sum, count }.value()
    }
}

impl IncrementalOp for Avg {
    fn add(&mut self, val: Value) {
        self.sum += val;
        self.count += 1;
    }

    fn sub(&mut self, val: Value) {
        self.sum -= val;
        self.count -= 1;
    }

    fn value(&self) -> Value {
        if self.count > 0 {
            match self.sum {
                Value::Unsigned(sum) => Value::Unsigned(sum / self.count as u64),
                Value::Signed(sum) => Value::Signed(sum / self.count as i64),
                Value::Float(sum) => Value::Float(sum / self.count as f64),
                _ => unreachable!("Incompatible Value Types"),
            }
        } else {
            Value::None
        }
    }
}

// See https://en.wikipedia.org/wiki/Algorithms_for_calculating_variance#Welford's_online_algorithm for reference
struct Variance {
    count: usize,
    m_2: Decimal,
    sum: Decimal,
}

impl InstanceOp for Variance {
    fn neutral(_ty: &Type) -> Self {
        Variance {
            count: 0,
            m_2: Decimal::zero(),
            sum: Decimal::zero(),
        }
    }
}

impl TotalOp for Variance {
    fn for_instances<'s>(&'s self, instances: impl IntoIterator<Item = &'s InstanceStore>) -> Value {
        let (sum, m_2, count) = instances
            .into_iter()
            .filter_map(|inst| inst.get_value(0).and_then(|v| v.try_into().ok()))
            .fold(
                (Decimal::from(0), Decimal::from(0), 0usize),
                |(sum, m_2, count), val: Decimal| {
                    let avg_old = if count > 0 {
                        sum / Decimal::from(count)
                    } else {
                        Decimal::zero()
                    };
                    let avg_current = (sum + val) / Decimal::from(count + 1);
                    (sum + val, m_2 + (val - avg_old) * (val - avg_current), count + 1)
                },
            );
        Self { sum, m_2, count }.value()
    }
}

impl IncrementalOp for Variance {
    fn add(&mut self, val: Value) {
        let val: Decimal = val.try_into().unwrap();
        let avg_old = if self.count > 0 {
            self.sum / Decimal::from(self.count)
        } else {
            Decimal::zero()
        };
        self.count += 1;
        self.sum += val;
        let avg_current = (self.sum) / Decimal::from(self.count);
        self.m_2 += (val - avg_old) * (val - avg_current);
    }

    fn sub(&mut self, val: Value) {
        // M_(n-1) = M_n - (val-old_avg) * (val - new_avg)
        let val: Decimal = val.try_into().unwrap();
        let avg_old = if self.count > 0 {
            self.sum / Decimal::from(self.count)
        } else {
            Decimal::zero()
        };
        self.count -= 1;
        self.sum -= val;
        let avg_current = if self.count > 0 {
            (self.sum) / Decimal::from(self.count)
        } else {
            Decimal::zero()
        };
        self.m_2 -= (val - avg_old) * (val - avg_current);
    }

    fn value(&self) -> Value {
        if self.count == 0 {
            return Value::Float(NotNan::from(0));
        }
        Value::try_from(self.m_2 / Decimal::from(self.count)).expect("")
    }
}

struct StdDev(Variance);

impl InstanceOp for StdDev {
    fn neutral(ty: &Type) -> Self {
        Self(Variance::neutral(ty))
    }
}

impl TotalOp for StdDev {
    fn for_instances<'s>(&'s self, instances: impl IntoIterator<Item = &'s InstanceStore>) -> Value {
        let inner = self.0.for_instances(instances);
        inner.pow(Value::try_from(0.5).unwrap())
    }
}

impl IncrementalOp for StdDev {
    fn add(&mut self, val: Value) {
        self.0.add(val)
    }

    fn sub(&mut self, val: Value) {
        self.0.sub(val)
    }

    fn value(&self) -> Value {
        let inner = self.0.value();
        inner.pow(Value::try_from(0.5).unwrap())
    }
}

// See https://en.wikipedia.org/wiki/Algorithms_for_calculating_variance#Online for reference
struct CoVar {
    count: usize,
    co_moment: Decimal,
    sum_x: Decimal,
    sum_y: Decimal,
}

impl CoVar {
    fn unwrap_value(val: Value) -> Option<(Decimal, Decimal)> {
        if let Value::Tuple(inner) = val {
            let [x, y]: [Value; 2] = inner.into_vec().try_into().unwrap();
            x.try_into().ok().and_then(|x| y.try_into().ok().map(|y| (x, y)))
        } else {
            None
        }
    }
}

impl InstanceOp for CoVar {
    fn neutral(_ty: &Type) -> Self {
        Self {
            count: 0,
            co_moment: Decimal::zero(),
            sum_x: Decimal::zero(),
            sum_y: Decimal::zero(),
        }
    }
}

impl TotalOp for CoVar {
    fn for_instances<'s>(&'s self, instances: impl IntoIterator<Item = &'s InstanceStore>) -> Value {
        let (sum_x, sum_y, co_moment, count) = instances
            .into_iter()
            .filter_map(|inst| inst.get_value(0).and_then(Self::unwrap_value))
            .fold(
                (Decimal::from(0), Decimal::from(0), Decimal::from(0), 0usize),
                |(sum_x, sum_y, co_moment, count), (x, y): (Decimal, Decimal)| {
                    let avg_x_old = if count > 0 {
                        sum_x / Decimal::from(count)
                    } else {
                        Decimal::zero()
                    };
                    let avg_y_current = (sum_y + y) / Decimal::from(count + 1);
                    (
                        sum_x + x,
                        sum_y + y,
                        co_moment + (x - avg_x_old) * (y - avg_y_current),
                        count + 1,
                    )
                },
            );
        Self {
            sum_x,
            sum_y,
            co_moment,
            count,
        }
        .value()
    }
}

impl IncrementalOp for CoVar {
    fn add(&mut self, val: Value) {
        let (x, y) = Self::unwrap_value(val).expect("Covariance to be applied to tuples");
        let avg_x_old = if self.count > 0 {
            self.sum_x / Decimal::from(self.count)
        } else {
            Decimal::zero()
        };
        let avg_y_current = (self.sum_y + y) / Decimal::from(self.count + 1);
        self.sum_x += x;
        self.sum_y += y;
        self.co_moment += (x - avg_x_old) * (y - avg_y_current);
        self.count += 1;
    }

    fn sub(&mut self, val: Value) {
        let (x, y) = Self::unwrap_value(val).expect("Covariance to be applied to tuples");
        let avg_x_old = if self.count - 1 > 0 {
            (self.sum_x - x) / Decimal::from(self.count - 1)
        } else {
            Decimal::zero()
        };
        let avg_y_current = if self.count > 0 {
            (self.sum_y) / Decimal::from(self.count)
        } else {
            Decimal::zero()
        };
        self.sum_x -= x;
        self.sum_y -= y;
        self.co_moment -= (x - avg_x_old) * (y - avg_y_current);
        self.count -= 1;
    }

    fn value(&self) -> Value {
        if self.count == 0 {
            return Value::None;
        }
        Value::try_from(self.co_moment / Decimal::from(self.count)).expect("")
    }
}

struct Percentile {
    percentile: u8,
}

impl Percentile {
    fn new(percentile: u8) -> Self {
        Percentile { percentile }
    }
}

impl InstanceOp for Percentile {
    fn neutral(_ty: &Type) -> Self {
        panic!("Should not be used with percentile.")
    }
}

impl TotalOp for Percentile {
    fn for_instances<'s>(&'s self, instances: impl IntoIterator<Item = &'s InstanceStore>) -> Value {
        let mut values: Vec<Value> = instances.into_iter().filter_map(|inst| inst.get_value(0)).collect();

        values.sort_unstable_by(|l, r| {
            match (l, r) {
                (Value::Signed(x), Value::Signed(y)) => x.cmp(y),
                (Value::Unsigned(x), Value::Unsigned(y)) => x.cmp(y),
                (Value::Float(x), Value::Float(y)) => x.partial_cmp(y).unwrap(),
                _ => unimplemented!("only primitive types implemented for percentile"),
            }
        });
        let count = (values.len() - 1) * self.percentile as usize;
        if count % 100 == 0 {
            let idx = count / 100;
            values[idx].clone()
        } else {
            // Take the average of the adjacent values
            let idx = count as f64 / 100.0;
            let frac = idx.fract();
            let idx = idx.floor() as usize;
            let diff = Value::try_from(frac).unwrap() * (values[idx + 1].clone() - values[idx].clone());
            values[idx].clone() + diff
        }
    }
}

#[cfg(test)]
mod tests {
    use rtlola_frontend::mir::{FloatTy, MemorizationBound, Type};

    use crate::storage::instance_aggregations::{
        All, AllAggregation, Any, Avg, CoVar, Count, FreshAggregation, Incremental, InstanceOp, Max, Min, Percentile,
        Product, StdDev, Sum, Total, Variance,
    };
    use crate::storage::stores::InstanceCollection;
    use crate::storage::InstanceAggregationTrait;
    use crate::Value;

    const FLOAT_TY: Type = Type::Float(FloatTy::Float64);

    const BOOL_TY: Type = Type::Bool;

    fn float(f: f64) -> Value {
        Value::try_from(f).unwrap()
    }

    fn tuple(a: f64, b: f64) -> Value {
        Value::Tuple(vec![float(a), float(b)].into_boxed_slice())
    }

    fn prepare_store(values: &[Value]) -> InstanceCollection {
        prepare_store_with_ty(values, FLOAT_TY)
    }

    fn prepare_store_with_ty(values: &[Value], ty: Type) -> InstanceCollection {
        let mut res = InstanceCollection::new(&ty, MemorizationBound::Bounded(1));
        for (idx, val) in values.into_iter().enumerate() {
            let parameter = Value::Unsigned(idx as u64);
            res.create_instance(&[parameter.clone()]).unwrap();
            res.instance_mut(&[parameter]).unwrap().push_value(val.clone());
        }
        res
    }

    fn apply_incremental_cb<F>(values: &[Value], aggr: &mut impl InstanceAggregationTrait, rand_val: F)
    where
        F: Fn(usize) -> Value,
    {
        for (idx, val) in values.into_iter().enumerate() {
            aggr.accept_value(val.clone());
            // Also add random values to test removal
            let idx_val = rand_val(idx);
            aggr.accept_value(idx_val.clone());
            aggr.remove_value(idx_val);
        }
    }

    fn apply_incremental(values: &[Value], aggr: &mut impl InstanceAggregationTrait) {
        apply_incremental_cb(values, aggr, |idx| float((idx + 10) as f64))
    }

    #[test]
    fn test_count() {
        let values = &[float(42.0), float(5.0), float(7.0), float(-13.0)];
        let store = prepare_store(values);

        let fresh = FreshAggregation(Count::neutral(&FLOAT_TY));
        let mut all = Incremental(AllAggregation(Count::neutral(&FLOAT_TY)));
        apply_incremental(values, &mut all);

        assert_eq!(fresh.get_value(&store), Value::Unsigned(4));
        assert_eq!(all.get_value(&store), Value::Unsigned(4));
    }

    #[test]
    fn test_min() {
        let values = &[float(42.0), float(5.0), float(7.0), float(-13.0)];
        let store = prepare_store(values);

        let fresh = FreshAggregation(Min::neutral(&FLOAT_TY));
        let all = Total(AllAggregation(Min::neutral(&FLOAT_TY)));

        assert_eq!(fresh.get_value(&store), float(-13.0));
        assert_eq!(all.get_value(&store), float(-13.0));
    }

    #[test]
    fn test_max() {
        let values = &[float(42.0), float(5.0), float(7.0), float(-13.0)];
        let store = prepare_store(values);

        let fresh = FreshAggregation(Max::neutral(&FLOAT_TY));
        let all = Total(AllAggregation(Max::neutral(&FLOAT_TY)));

        assert_eq!(fresh.get_value(&store), float(42.0));
        assert_eq!(all.get_value(&store), float(42.0));
    }

    #[test]
    fn test_sum() {
        let values = &[float(42.0), float(5.0), float(7.0), float(-13.0)];
        let store = prepare_store(values);

        let fresh = FreshAggregation(Sum::neutral(&FLOAT_TY));
        let mut all = Incremental(AllAggregation(Sum::neutral(&FLOAT_TY)));
        apply_incremental(values, &mut all);

        assert_eq!(fresh.get_value(&store), float(41.0));
        assert_eq!(all.get_value(&store), float(41.0));
    }

    #[test]
    fn test_avg() {
        let values = &[float(42.0), float(5.0), float(7.0), float(-13.0)];
        let store = prepare_store(values);

        let fresh = FreshAggregation(Avg::neutral(&FLOAT_TY));
        let mut all = Incremental(AllAggregation(Avg::neutral(&FLOAT_TY)));
        apply_incremental(values, &mut all);

        assert_eq!(fresh.get_value(&store), float(10.25));
        assert_eq!(all.get_value(&store), float(10.25));
    }

    #[test]
    fn test_product() {
        let values = &[float(42.0), float(5.0), float(7.0), float(-13.0)];
        let store = prepare_store(values);

        let fresh = FreshAggregation(Product::neutral(&FLOAT_TY));
        let mut all = Incremental(AllAggregation(Product::neutral(&FLOAT_TY)));
        apply_incremental(values, &mut all);

        assert_eq!(fresh.get_value(&store), float(-19110.0));
        assert_eq!(all.get_value(&store), float(-19110.0));
    }

    #[test]
    fn test_conjunction() {
        let values = &[
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
        ];
        let store = prepare_store_with_ty(values, BOOL_TY);

        let fresh = FreshAggregation(All::neutral(&BOOL_TY));
        let mut all = Incremental(AllAggregation(All::neutral(&BOOL_TY)));
        apply_incremental_cb(values, &mut all, |idx| Value::Bool(idx % 2 == 0));

        assert_eq!(fresh.get_value(&store), Value::Bool(false));
        assert_eq!(all.get_value(&store), Value::Bool(false));
    }

    #[test]
    fn test_disjunction() {
        let values = &[
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false),
        ];
        let store = prepare_store_with_ty(values, BOOL_TY);

        let fresh = FreshAggregation(Any::neutral(&BOOL_TY));
        let mut all = Incremental(AllAggregation(Any::neutral(&BOOL_TY)));
        apply_incremental_cb(values, &mut all, |idx| Value::Bool(idx % 2 == 0));

        assert_eq!(fresh.get_value(&store), Value::Bool(true));
        assert_eq!(all.get_value(&store), Value::Bool(true));
    }

    #[test]
    fn test_variance() {
        let values = &[float(42.0), float(5.0), float(7.0), float(-13.0)];
        let store = prepare_store(values);

        let fresh = FreshAggregation(Variance::neutral(&FLOAT_TY));
        let mut all = Incremental(AllAggregation(Variance::neutral(&FLOAT_TY)));
        apply_incremental(values, &mut all);

        assert_eq!(fresh.get_value(&store), float(396.6875));
        assert_eq!(all.get_value(&store), float(396.6875));
    }

    #[test]
    fn test_stddev() {
        let values = &[float(42.0), float(5.0), float(7.0), float(-13.0)];
        let store = prepare_store(values);

        let fresh = FreshAggregation(StdDev::neutral(&FLOAT_TY));
        let mut all = Incremental(AllAggregation(StdDev::neutral(&FLOAT_TY)));
        apply_incremental(values, &mut all);

        assert_eq!(fresh.get_value(&store), float(19.917015338649513));
        assert_eq!(all.get_value(&store), float(19.917015338649513));
    }

    #[test]
    fn test_covariance() {
        let ty = Type::Tuple(vec![FLOAT_TY, FLOAT_TY]);
        let values = &[
            tuple(42.0, 15.0),
            tuple(5.0, 8.0),
            tuple(7.0, 23.0),
            tuple(-13.0, -11.0),
        ];
        let store = prepare_store_with_ty(values, ty);

        let fresh = FreshAggregation(CoVar::neutral(&FLOAT_TY));
        let mut all = Incremental(AllAggregation(CoVar::neutral(&FLOAT_TY)));
        apply_incremental_cb(values, &mut all, |idx| tuple((idx + 5) as f64, (idx + 13) as f64));

        assert_eq!(fresh.get_value(&store), float(153.8125));
        assert_eq!(all.get_value(&store), float(153.8125));
    }

    #[test]
    fn test_percentile() {
        let values = &[float(42.0), float(5.0), float(7.0), float(-13.0)];
        let store = prepare_store(values);

        let mut fresh = FreshAggregation(Percentile::new(50));
        let mut all = Total(AllAggregation(Percentile::new(50)));

        assert_eq!(fresh.get_value(&store), float(6.0));
        assert_eq!(all.get_value(&store), float(6.0));

        fresh = FreshAggregation(Percentile::new(25));
        all = Total(AllAggregation(Percentile::new(25)));

        assert_eq!(fresh.get_value(&store), float(0.5));
        assert_eq!(all.get_value(&store), float(0.5));

        fresh = FreshAggregation(Percentile::new(75));
        all = Total(AllAggregation(Percentile::new(75)));

        assert_eq!(fresh.get_value(&store), float(15.75));
        assert_eq!(all.get_value(&store), float(15.75));

        fresh = FreshAggregation(Percentile::new(42));
        all = Total(AllAggregation(Percentile::new(42)));

        assert_eq!(fresh.get_value(&store), float(5.52));
        assert_eq!(all.get_value(&store), float(5.52));
    }
}
