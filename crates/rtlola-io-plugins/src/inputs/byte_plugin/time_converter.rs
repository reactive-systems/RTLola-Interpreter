//! Module that contains the [TimeConverter] and implementations for some [rtlola_interpreter::time::TimeRepresentation]
use rtlola_interpreter::input::{AssociatedEventFactory, EventFactory};
use rtlola_interpreter::time::{DelayTime, RealTime, TimeRepresentation};

/// Trait to convert a value interpreted as time to a [TimeRepresentation] used by the [rtlola_interpreter::Monitor].
pub trait TimeConverter<T: TimeRepresentation>: Sized + AssociatedEventFactory {
    /// Converts a value to a [TimeRepresentation].
    fn convert_time(
        &self,
    ) -> Result<
        <T as TimeRepresentation>::InnerTime,
        <<Self as AssociatedEventFactory>::Factory as EventFactory>::Error,
    >;
}

impl<Map: AssociatedEventFactory> TimeConverter<DelayTime> for Map {
    fn convert_time(
        &self,
    ) -> Result<
        <DelayTime as TimeRepresentation>::InnerTime,
        <<Self as AssociatedEventFactory>::Factory as EventFactory>::Error,
    > {
        Ok(())
    }
}

impl<Map: AssociatedEventFactory> TimeConverter<RealTime> for Map {
    fn convert_time(
        &self,
    ) -> Result<
        <RealTime as TimeRepresentation>::InnerTime,
        <<Self as AssociatedEventFactory>::Factory as EventFactory>::Error,
    > {
        Ok(())
    }
}
