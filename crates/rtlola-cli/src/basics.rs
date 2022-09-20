pub(crate) mod dynamic_schedule;
pub(crate) mod time_driven_manager;

// Re-exports
pub(crate) use self::dynamic_schedule::DynamicSchedule;
pub(crate) use self::time_driven_manager::TimeEvaluation;
use crate::Time;

#[derive(Debug, Clone)]
pub(crate) enum WorkItem {
    Event(EventEvaluation, Time),
    Time(TimeEvaluation, Time),
    End,
}

pub(crate) const CAP_WORK_QUEUE: usize = 8;
pub(crate) const CAP_LOCAL_QUEUE: usize = 4096;
