mod controller;
pub(crate) mod dynamic_schedule;
mod event_driven_manager;
pub mod monitor;
mod time_driven_manager;

// Re-exports
pub(crate) use self::controller::Controller;
pub(crate) use self::dynamic_schedule::DynamicSchedule;
pub(crate) use self::event_driven_manager::EventEvaluation;
pub use self::monitor::Monitor;
pub(crate) use self::time_driven_manager::EvaluationTask;
pub(crate) use self::time_driven_manager::TimeEvaluation;
use crate::basics::Time;
use crate::storage::Value;

pub type Event = Vec<Value>;

#[derive(Debug, Clone)]
pub(crate) enum WorkItem<'a> {
    Event(EventEvaluation, Time),
    Time(TimeEvaluation<'a>, Time),
    End,
}

pub(crate) const CAP_WORK_QUEUE: usize = 8;
pub(crate) const CAP_LOCAL_QUEUE: usize = 4096;
