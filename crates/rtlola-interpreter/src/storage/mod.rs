pub(crate) use self::stores::{GlobalStore, InstanceStore};
pub use self::value::Value;
pub use self::value::ValueConvertError;
pub(crate) use self::window::SlidingWindow;

mod discrete_window;
mod stores;
mod value;
mod window;
mod window_aggregations;
