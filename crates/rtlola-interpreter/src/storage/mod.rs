pub(crate) use self::instance_aggregations::InstanceAggregationTrait;
pub(crate) use self::stores::{
    GlobalStore, InstanceStore, WindowParameterization, WindowParameterizationKind,
};
pub use self::value::{Value, ValueConvertError};
pub(crate) use self::window::SlidingWindow;

mod discrete_window;
mod instance_aggregations;
mod stores;
mod value;
mod window;
mod window_aggregations;
