use rtlola_interpreter::input::InputMap;
use rtlola_interpreter::time::{DelayTime, RealTime, TimeRepresentation};

/// Trait to convert a value interpreted as time to a [TimeRepresentation] used by the [rtlola_interpreter::Monitor].
pub trait TimeConverter<T: TimeRepresentation>: Sized + InputMap {
    /// Converts a value to a [TimeRepresentation].
    fn convert_time(&self) -> Result<<T as TimeRepresentation>::InnerTime, <Self as InputMap>::Error>;
}

impl<Map: InputMap> TimeConverter<DelayTime> for Map {
    fn convert_time(&self) -> Result<<DelayTime as TimeRepresentation>::InnerTime, <Self as InputMap>::Error> {
        Ok(())
    }
}

impl<Map: InputMap> TimeConverter<RealTime> for Map {
    fn convert_time(&self) -> Result<<RealTime as TimeRepresentation>::InnerTime, <Self as InputMap>::Error> {
        Ok(())
    }
}
