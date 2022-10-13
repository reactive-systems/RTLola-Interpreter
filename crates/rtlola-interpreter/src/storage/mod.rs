pub(crate) use self::stores::{GlobalStore, InstanceStore};
pub use self::value::Value;
pub(crate) use self::window::SlidingWindow;

mod discrete_window;
mod stores;
mod value;
mod window;
mod window_aggregations;
