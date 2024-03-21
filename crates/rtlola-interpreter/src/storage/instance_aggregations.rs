use std::ops::Add;

use rtlola_frontend::mir;
use rtlola_frontend::mir::{InstanceOperation, InstanceSelection, StreamReference, Type, Window};

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
                    InstanceOperation::Variance => {
                        todo!()
                    },
                    InstanceOperation::Covariance => {
                        todo!()
                    },
                    InstanceOperation::StandardDeviation => {
                        todo!()
                    },
                    InstanceOperation::NthPercentile(_) => {
                        todo!()
                    },
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
                    InstanceOperation::Variance => {
                        todo!()
                    },
                    InstanceOperation::Covariance => {
                        todo!()
                    },
                    InstanceOperation::StandardDeviation => {
                        todo!()
                    },
                    InstanceOperation::NthPercentile(_) => {
                        todo!()
                    },
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
    fn all_instances(&mut self, instances: &InstanceCollection) {
        self.inner.all_instances(instances)
    }

    fn accept_value(&mut self, value: Value) {
        self.inner.accept_value(value);
    }

    fn remove_value(&mut self, value: Value) {
        self.inner.remove_value(value);
    }

    fn get_value(&self) -> Value {
        self.inner.get_value()
    }
}

pub(crate) trait InstanceAggregationTrait {
    fn all_instances(&mut self, _instances: &InstanceCollection) {}

    fn accept_value(&mut self, _value: Value) {}

    fn remove_value(&mut self, _value: Value) {}

    fn get_value(&self) -> Value;
}

struct FreshAggregation<OP: TotalOp>(OP);
impl<OP: TotalOp> InstanceAggregationTrait for FreshAggregation<OP> {
    fn all_instances(&mut self, instances: &InstanceCollection) {
        let fresh = instances.fresh().map(|inst| instances.instance(inst).unwrap());
        self.0.instances(fresh)
    }

    fn get_value(&self) -> Value {
        self.0.value()
    }
}

struct AllAggregation<OP: InstanceOp>(OP);

impl<OP: InstanceOp> AllAggregation<OP> {
    fn get_value(&self) -> Value {
        self.0.value()
    }
}

struct Total<OP: TotalOp>(AllAggregation<OP>);
impl<OP: TotalOp> InstanceAggregationTrait for Total<OP> {
    fn all_instances(&mut self, instances: &InstanceCollection) {
        self.0 .0.instances(instances.instances());
    }

    fn get_value(&self) -> Value {
        self.0.get_value()
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

    fn get_value(&self) -> Value {
        self.0.get_value()
    }
}

trait InstanceOp {
    fn neutral(ty: &Type) -> Self;

    fn value(&self) -> Value;
}

trait IncrementalOp: InstanceOp {
    /// Add the value to the aggregation
    fn add(&mut self, _val: Value);

    /// Subtract the value from the aggregation
    fn sub(&mut self, _val: Value);
}

trait TotalOp: InstanceOp {
    /// Compute the value given all instances
    fn instances<'s>(&'s mut self, instances: impl IntoIterator<Item = &'s InstanceStore>);
}

struct All {
    num_false: usize,
}

impl InstanceOp for All {
    fn neutral(_ty: &Type) -> Self {
        Self { num_false: 0 }
    }

    fn value(&self) -> Value {
        Value::Bool(self.num_false == 0)
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
}

impl TotalOp for All {
    fn instances<'s>(&'s mut self, instances: impl IntoIterator<Item = &'s InstanceStore>) {
        self.num_false = instances
            .into_iter()
            .filter_map(|inst| inst.get_value(0))
            .filter_map(|val| {
                (!<Value as TryInto<bool>>::try_into(val).expect("Value types to be correct")).then_some(true)
            })
            .count()
    }
}

struct Any {
    num_true: usize,
}

impl InstanceOp for Any {
    fn neutral(_ty: &Type) -> Self {
        Self { num_true: 0 }
    }

    fn value(&self) -> Value {
        Value::Bool(self.num_true > 0)
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
}

impl TotalOp for Any {
    fn instances<'s>(&'s mut self, instances: impl IntoIterator<Item = &'s InstanceStore>) {
        self.num_true = instances
            .into_iter()
            .filter_map(|inst| inst.get_value(0))
            .filter_map(|val| {
                <Value as TryInto<bool>>::try_into(val)
                    .expect("Value types to be correct")
                    .then_some(true)
            })
            .count()
    }
}

struct Count {
    res: usize,
}

impl InstanceOp for Count {
    fn neutral(_ty: &Type) -> Self {
        Self { res: 0 }
    }

    fn value(&self) -> Value {
        Value::Unsigned(self.res as u64)
    }
}

impl IncrementalOp for Count {
    fn add(&mut self, _val: Value) {
        self.res += 1;
    }

    fn sub(&mut self, _val: Value) {
        self.res -= 1;
    }
}

impl TotalOp for Count {
    fn instances<'s>(&'s mut self, instances: impl IntoIterator<Item = &'s InstanceStore>) {
        self.res = instances.into_iter().filter_map(|inst| inst.get_value(0)).count()
    }
}

struct Min {
    res: Value,
}

impl InstanceOp for Min {
    fn neutral(_ty: &Type) -> Self {
        Self { res: Value::None }
    }

    fn value(&self) -> Value {
        self.res.clone()
    }
}

impl TotalOp for Min {
    fn instances<'s>(&'s mut self, instances: impl IntoIterator<Item = &'s InstanceStore>) {
        self.res = itertools::min(instances.into_iter().filter_map(|inst| inst.get_value(0))).unwrap_or(Value::None);
    }
}

struct Max {
    res: Value,
}

impl InstanceOp for Max {
    fn neutral(_ty: &Type) -> Self {
        Self { res: Value::None }
    }

    fn value(&self) -> Value {
        self.res.clone()
    }
}

impl TotalOp for Max {
    fn instances<'s>(&'s mut self, instances: impl IntoIterator<Item = &'s InstanceStore>) {
        self.res = itertools::max(instances.into_iter().filter_map(|inst| inst.get_value(0))).unwrap_or(Value::None);
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

    fn value(&self) -> Value {
        self.res.clone()
    }
}

impl TotalOp for Sum {
    fn instances<'s>(&'s mut self, instances: impl IntoIterator<Item = &'s InstanceStore>) {
        self.res = instances
            .into_iter()
            .filter_map(|inst| inst.get_value(0))
            .fold(self.res.for_int(0), <Value as Add>::add);
    }
}

impl IncrementalOp for Sum {
    fn add(&mut self, val: Value) {
        self.res += val;
    }

    fn sub(&mut self, val: Value) {
        self.res -= val;
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

    fn value(&self) -> Value {
        self.res.clone()
    }
}

impl TotalOp for Product {
    fn instances<'s>(&'s mut self, instances: impl IntoIterator<Item = &'s InstanceStore>) {
        self.res = instances
            .into_iter()
            .filter_map(|inst| inst.get_value(0))
            .fold(self.res.for_int(0), <Value as Add>::add);
    }
}

impl IncrementalOp for Product {
    fn add(&mut self, val: Value) {
        self.res += val;
    }

    fn sub(&mut self, val: Value) {
        self.res -= val;
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

impl TotalOp for Avg {
    fn instances<'s>(&'s mut self, instances: impl IntoIterator<Item = &'s InstanceStore>) {
        (self.sum, self.count) = instances
            .into_iter()
            .filter_map(|inst| inst.get_value(0))
            .fold((self.sum.for_int(0), 0), |(sum, count), val| (sum + val, count + 1));
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
}
