use rtlola_frontend::mir;
use rtlola_frontend::mir::{InstanceOperation, InstanceSelection};

use crate::Value;

pub(crate) struct InstanceAggregation(Box<dyn InstanceAggregationTrait>);

impl From<mir::InstanceAggregation> for InstanceAggregation {
    fn from(value: mir::InstanceAggregation) -> Self {
        InstanceAggregation(match value.selection {
            InstanceSelection::Fresh => {
                match value.aggr {
                    InstanceOperation::Count => {
                        todo!()
                    },
                    InstanceOperation::Min => {
                        todo!()
                    },
                    InstanceOperation::Max => {
                        todo!()
                    },
                    InstanceOperation::Sum => {
                        todo!()
                    },
                    InstanceOperation::Product => {
                        todo!()
                    },
                    InstanceOperation::Average => {
                        todo!()
                    },
                    InstanceOperation::Conjunction => Box::new(FreshAggregation(All::NEUTRAL)),
                    InstanceOperation::Disjunction => Box::new(FreshAggregation(Any::NEUTRAL)),
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
                    InstanceOperation::Count => {
                        todo!()
                    },
                    InstanceOperation::Min => {
                        todo!()
                    },
                    InstanceOperation::Max => {
                        todo!()
                    },
                    InstanceOperation::Sum => {
                        todo!()
                    },
                    InstanceOperation::Product => {
                        todo!()
                    },
                    InstanceOperation::Average => {
                        todo!()
                    },
                    InstanceOperation::Conjunction => Box::new(AllAggregation(All::NEUTRAL)),
                    InstanceOperation::Disjunction => Box::new(AllAggregation(Any::NEUTRAL)),
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
        })
    }
}

pub(crate) trait InstanceAggregationTrait {
    fn new_cycle(&mut self) {}

    fn accept_value(&mut self, value: Value);

    fn remove_value(&mut self, _value: Value) {}

    fn get_value(&self) -> Value;
}

pub(crate) struct FreshAggregation<OP: InstanceOp>(OP);
impl<OP: InstanceOp> InstanceAggregationTrait for FreshAggregation<OP> {
    fn new_cycle(&mut self) {
        self.0 = OP::NEUTRAL
    }

    fn accept_value(&mut self, value: Value) {
        self.0.add(value);
    }

    fn get_value(&self) -> Value {
        self.0.value()
    }
}

pub(crate) struct AllAggregation<OP: InstanceOp>(OP);

impl<OP: InstanceOp> InstanceAggregationTrait for AllAggregation<OP> {
    fn accept_value(&mut self, value: Value) {
        self.0.add(value);
    }

    fn remove_value(&mut self, value: Value) {
        self.0.sub(value)
    }

    fn get_value(&self) -> Value {
        self.0.value()
    }
}

trait InstanceOp {
    const NEUTRAL: Self;

    /// Add the value to the aggregation
    fn add(&mut self, val: Value);

    /// Subtract the value from the aggregation
    fn sub(&mut self, val: Value);

    fn value(&self) -> Value;
}

struct All {
    num_false: usize,
}

impl InstanceOp for All {
    const NEUTRAL: Self = Self { num_false: 0 };

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

struct Any {
    num_true: usize,
}

impl InstanceOp for Any {
    const NEUTRAL: Self = Self { num_true: 0 };

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
