//! The [QueuedMonitor] is the multi-threaded version of the API.
//! Deadlines are evaluated immediately and the resulting verdicts are returned through a queue retrieved using the [output_queue] method.
//! This API should be used in an online monitoring setting.
//!
//! The [QueuedMonitor] is parameterized over its input and output method.
//! The preferred method to create an API is using the [ConfigBuilder](crate::ConfigBuilder) and the [queued_monitor](crate::ConfigBuilder::queued_monitor) method.
//!
//! # Input Method
//! An input method has to implement the [Input] trait. Out of the box two different methods are provided:
//! * [EventInput]: Provides a basic input method for anything that already is an [Event] or that can be transformed into one using `Into<Event>`.
//! * [RecordInput]: Is a more elaborate input method. It allows to provide a custom data structure to the monitor as an input, as long as it implements the [Record] trait.
//!     If implemented this traits provides functionality to generate a new value for any input stream from the data structure.
//!
//! # Output Method
//! The [Monitor] can provide output with a varying level of detail captured by the [VerdictRepresentation] trait. The different output formats are:
//! * [Incremental]: For each processed event a condensed list of monitor state changes is provided.
//! * [Total]: For each event a complete snapshot of the current monitor state is returned
//! * [TriggerMessages]: For each event a list of violated triggers with their description is produced.
//! * [TriggersWithInfoValues]: For each event a list of violated triggers with their specified corresponding values is returned.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Debug;
use std::ops::Not;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

use crossbeam_channel::{unbounded, Sender, TrySendError};
pub use crossbeam_channel::{Receiver, RecvError, RecvTimeoutError, TryRecvError};
use rtlola_frontend::mir::{InputReference, OutputReference, RtLolaMir, Type};

use crate::config::{Config, ExecutionMode};
use crate::configuration::time::{init_start_time, OutputTimeRepresentation, RelativeFloat, TimeRepresentation};
use crate::evaluator::EvaluatorData;
use crate::monitor::{Incremental, Input, RawVerdict, VerdictRepresentation};
use crate::schedule::schedule::ScheduleManager;
use crate::schedule::DynamicSchedule;
use crate::Monitor;

enum WorkItem<Source: Input, SourceTime: TimeRepresentation> {
    Start,
    Event(Source::Record, SourceTime::InnerTime),
}

/// Represents the kind of the verdict. I.e. whether the evaluation was triggered by an event, or by a deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictKind {
    /// The verdict resulted from a deadline evaluation.
    Timed,
    /// The verdict resulted from the evaluation of an event.
    Event,
}

/// The verdict of the queued monitor. It is either triggered by a deadline or an event described by the `kind` field.
/// The time when the [Verdict] occurred ist given by `ts`. `verdict` finally describes the changes to input and output streams
/// as defined by the [VerdictRepresentation].
#[derive(Debug, Clone)]
pub struct QueuedVerdict<Verdict: VerdictRepresentation, VerdictTime: OutputTimeRepresentation> {
    /// The kind of the verdict. I.e. what triggered the evaluation it resulted from.
    pub kind: VerdictKind,
    /// The time when the verdict occurred.
    pub ts: VerdictTime::InnerTime,
    /// The changes of input and output streams as defined by the [VerdictRepresentation]
    pub verdict: Verdict,
}

/**
The QueuedMonitor is a threaded version of the Api allowing deadlines to be evaluated immediately.

The [QueuedMonitor] accepts new events and computes streams.
It can compute streams based on new events through `accept_event` once the `start` function was invoked.
Timed streams are evaluated automatically at their deadline. The resulting verdicts are returned through a [Receiver] returned by `start`.
Note that the `start` function *has* to be invoked before any event can be evaluated.

The generic argument `Source` implements the [Input] trait describing the input source of the API.
The generic argument `SourceTime` implements the [TimeRepresentation] trait defining the input time format.
The generic argument `Verdict` implements the [VerdictRepresentation] trait describing the output format of the API that is by default [Incremental].
The generic argument `VerdictTime` implements the [TimeRepresentation] trait defining the output time format. It defaults to [RelativeFloat]
 */
#[allow(missing_debug_implementations)]
pub struct QueuedMonitor<Source, SourceTime, Verdict = Incremental, VerdictTime = RelativeFloat>
where
    Source: Input,
    SourceTime: TimeRepresentation,
    Verdict: VerdictRepresentation,
    VerdictTime: OutputTimeRepresentation + 'static,
{
    ir: RtLolaMir,

    input: Sender<WorkItem<Source, SourceTime>>,
    output: Receiver<QueuedVerdict<Verdict, VerdictTime>>,
}

/// Crate-public interface
impl<Source, SourceTime, Verdict, VerdictTime> QueuedMonitor<Source, SourceTime, Verdict, VerdictTime>
where
    Source: Input + 'static,
    SourceTime: TimeRepresentation,
    Verdict: VerdictRepresentation,
    VerdictTime: OutputTimeRepresentation,
{
    ///setup
    pub(crate) fn setup(
        config: Config<SourceTime, VerdictTime>,
        setup_data: Source::CreationData,
    ) -> QueuedMonitor<Source, SourceTime, Verdict, VerdictTime> {
        let config_clone = config.clone();

        let input_map = config
            .ir
            .inputs
            .iter()
            .map(|i| (i.name.clone(), i.reference.in_ix()))
            .collect();

        let (input_send, input_rcv) = unbounded();
        let (output_send, output_rcv) = unbounded();

        match config.mode {
            ExecutionMode::Offline => thread::spawn(move || Self::offline_worker(config_clone, setup_data, input_rcv, output_send)),
            ExecutionMode::Online => thread::spawn(move || Self::online_worker(config_clone, input_map, setup_data,input_rcv, output_send)),
        };

        QueuedMonitor {
            ir: config.ir,

            input: input_send,
            output: output_rcv,
        }
    }

    fn try_send(output: &Sender<QueuedVerdict<Verdict, VerdictTime>>, verdict: Option<QueuedVerdict<Verdict, VerdictTime>>) {
        if let Some(verdict) = verdict {
            if let Err(e) = output.try_send(verdict) {
                match e {
                    TrySendError::Full(_) => println!("Output queue overloaded! Verdict lost..."),
                    TrySendError::Disconnected(_) => println!("Output queue disconnected! Verdict lost..."),
                }
            }
        }
    }

    fn online_worker(
        config: Config<SourceTime, VerdictTime>,
        input_names: HashMap<String, InputReference>,
        setup_data: Source::CreationData,
        input: Receiver<WorkItem<Source, SourceTime>>,
        output: Sender<QueuedVerdict<Verdict, VerdictTime>>,
    ) -> () {
        // setup monitor
        let mut source_time = config.input_time_representation;
        let source = Source::new(input_names, setup_data);

        // Setup evaluator
        let dyn_schedule = Rc::new(RefCell::new(DynamicSchedule::new()));
        let eval_data = EvaluatorData::new(config.ir.clone(), dyn_schedule.clone());
        let mut schedule_manager = ScheduleManager::setup(config.ir.clone(), dyn_schedule)
            .expect("Error computing schedule for time-driven streams");
        let mut eval = eval_data.into_evaluator();

        // Wait for Start command
        loop {
            match input.recv() {
                Ok(WorkItem::Start) => break,
                Ok(WorkItem::Event(_, _)) => panic!("Received Event before 'start' was called"),
                Err(_) => return,
            }
        }

        init_start_time::<SourceTime>(config.start_time);
        let output_time = VerdictTime::default();

        loop {
            let next_deadline = schedule_manager.get_next_due();
            let item = if let Some(due) = next_deadline {
                input.recv_timeout(due)
            } else {
                input.recv().map_err(|_| RecvTimeoutError::Disconnected)
            };
            let verdict = match item {
                Ok(WorkItem::Event(e, ts)) => {
                    // Received Event before deadline
                    let e = source.get_event(e);
                    let ts = source_time.convert_from(ts);

                    eval.eval_event(&e, ts);
                    let verdict = Verdict::create(RawVerdict::from(&eval));
                    verdict.is_empty().not().then_some(
                    QueuedVerdict {
                        kind: VerdictKind::Event,
                        ts: output_time.convert_into(ts),
                        verdict,
                    })
                },
                Err(RecvTimeoutError::Timeout) => {
                    // Deadline occurred before event
                    let due = next_deadline.expect("timeout to only happen for a deadline.");
                    let deadline = schedule_manager.get_next_deadline(due);
                    schedule_manager.eval_deadline(&mut eval, deadline, due);

                    let verdict = Verdict::create(RawVerdict::from(&eval));
                    verdict.is_empty().not().then_some(
                    QueuedVerdict {
                        kind: VerdictKind::Timed,
                        ts: output_time.convert_into(due),
                        verdict,
                    })
                },
                Err(RecvTimeoutError::Disconnected) => {
                    // Channel closed, we are done here
                    return;
                },
                Ok(WorkItem::Start) => {
                    // Received second start command -> abort
                    panic!("Received second start command.")
                },
            };

            Self::try_send(&output, verdict);

        }
    }

    fn offline_worker(
        config: Config<SourceTime, VerdictTime>,
        setup_data: Source::CreationData,
        input: Receiver<WorkItem<Source, SourceTime>>,
        output: Sender<QueuedVerdict<Verdict, VerdictTime>>,
    ) -> () {
        // Wait for Start command
        loop {
            match input.recv() {
                Ok(WorkItem::Start) => break,
                Ok(WorkItem::Event(_, _)) => panic!("Received Event before 'start' was called"),
                Err(_) => return,
            }
        }

        // Setup evaluator
        let mut monitor: Monitor<Source, SourceTime, Verdict, VerdictTime> =
            Monitor::setup(config, setup_data);

        loop {
            let (verdict, ts) = match input.recv() {
                Ok(WorkItem::Event(e, ts)) => {
                    // Received Event
                    let v = monitor.accept_event(e, ts);
                    let ts = monitor.last_event().expect("the event to be recorded");
                    (v, ts)
                },
                Err(_) => {
                    // Channel closed, we are done here
                    return;
                },
                Ok(WorkItem::Start) => {
                    // Received second start command -> abort
                    panic!("Received second start command.")
                },
            };
            for (ts, v) in verdict.timed {
                let verdict = QueuedVerdict {
                    kind: VerdictKind::Timed,
                    ts,
                    verdict: v,
                };
                Self::try_send(&output, Some(verdict));
            }
            if !verdict.event.is_empty() {
                let ts = monitor.verdict_time().convert_into(ts);
                let verdict = QueuedVerdict {
                    kind: VerdictKind::Event,
                    ts,
                    verdict: verdict.event,
                };
                Self::try_send(&output, Some(verdict));
            }

        }
    }
}

/// Public interface
impl<Source, SourceTime, Verdict, VerdictTime> QueuedMonitor<Source, SourceTime, Verdict, VerdictTime>
where
    Source: Input,
    SourceTime: TimeRepresentation,
    Verdict: VerdictRepresentation,
    VerdictTime: OutputTimeRepresentation,
{
    /// Starts the evaluation process. This method has to be called before any event is accepted.
    pub fn start(&self) {
        self.input.send(WorkItem::Start).expect("Worker thread hung up!");

    }

    /// This method returns the queue through which the verdicts can be received.
    pub fn output_queue(&self) -> Receiver<QueuedVerdict<Verdict, VerdictTime>> {
        self.output.clone()
    }

    /**
    Schedules a new event for evaluation. The verdict can be received through the Queue return by the [QueuedMonitor::start].
    */
    pub fn accept_event(&mut self, ev: Source::Record, ts: SourceTime::InnerTime) {
        self.input
            .send(WorkItem::Event(ev, ts))
            .expect("Worker thread hung up!");
    }

    /// Returns the underlying representation of the specification as an [RtLolaMir]
    pub fn ir(&self) -> &RtLolaMir {
        &self.ir
    }

    /**
    Get the name of an input stream based on its [InputReference].

    The reference is valid for the lifetime of the monitor.
    */
    pub fn name_for_input(&self, id: InputReference) -> &str {
        self.ir.inputs[id].name.as_str()
    }

    /**
    Get the name of an output stream based on its [OutputReference].

    The reference is valid for the lifetime of the monitor.
    */
    pub fn name_for_output(&self, id: OutputReference) -> &str {
        self.ir.outputs[id].name.as_str()
    }

    /**
    Get the message of a trigger based on its index.

    The reference is valid for the lifetime of the monitor.
    */
    pub fn trigger_message(&self, id: usize) -> &str {
        self.ir.triggers[id].message.as_str()
    }

    /**
    Get the [OutputReference] of a trigger based on its index.
    */
    pub fn trigger_stream_index(&self, id: usize) -> usize {
        self.ir.triggers[id].reference.out_ix()
    }

    /**
    Get the number of input streams.
    */
    pub fn number_of_input_streams(&self) -> usize {
        self.ir.inputs.len()
    }

    /**
    Get the number of output streams (this includes one output stream for each trigger).
    */
    pub fn number_of_output_streams(&self) -> usize {
        self.ir.outputs.len()
    }

    /**
    Get the number of triggers.
    */
    pub fn number_of_triggers(&self) -> usize {
        self.ir.triggers.len()
    }

    /**
    Get the type of an input stream based on its [InputReference].

    The reference is valid for the lifetime of the monitor.
    */
    pub fn type_of_input(&self, id: InputReference) -> &Type {
        &self.ir.inputs[id].ty
    }

    /**
    Get the type of an output stream based on its [OutputReference].

    The reference is valid for the lifetime of the monitor.
    */
    pub fn type_of_output(&self, id: OutputReference) -> &Type {
        &self.ir.outputs[id].ty
    }

    /**
    Get the extend rate of an output stream based on its [OutputReference].

    The reference is valid for the lifetime of the monitor.
    */
    pub fn extend_rate_of_output(&self, id: OutputReference) -> Option<Duration> {
        self.ir
            .time_driven
            .iter()
            .find(|time_driven_stream| time_driven_stream.reference.out_ix() == id)
            .map(|time_driven_stream| time_driven_stream.period_in_duration())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::api::monitor::Change;
    use crate::monitor::{Event, EventInput, Incremental, Total, VerdictRepresentation};
    use crate::queued::{QueuedVerdict, VerdictKind};
    use crate::time::RelativeFloat;
    use crate::{ConfigBuilder, QueuedMonitor, Value};

    fn setup<V: VerdictRepresentation>(
        spec: &str,
    ) -> (
        Instant,
        QueuedMonitor<EventInput<Event>, RelativeFloat, V, RelativeFloat>,
    ) {
        // Init Monitor API
        let monitor = ConfigBuilder::new()
            .spec_str(spec)
            .offline::<RelativeFloat>()
            .event_input::<Event>()
            .with_verdict::<V>()
            .queued_monitor();
        (Instant::now(), monitor)
    }

    fn sort_total(res: Total) -> Total {
        let Total { inputs, mut outputs } = res;
        outputs.iter_mut().for_each(|s| s.sort());
        Total { inputs, outputs }
    }

    fn sort_incremental(mut res: Incremental) -> Incremental {
        res.iter_mut().for_each(|(_, changes)| changes.sort());
        res
    }

    #[test]
    fn test_const_output_literals() {
        let (start, mut monitor) = setup::<Total>(
            r#"
        input i_0: UInt8

        output o_0: Bool @i_0 := true
        output o_1: UInt8 @i_0 := 3
        output o_2: Int8 @i_0 := -5
        output o_3: Float32 @i_0 := -123.456
        output o_4: String @i_0 := "foobar"
        "#,
        );
        let queue = monitor.output_queue();
        monitor.start();
        let v = Value::Unsigned(3);
        let timeout = Duration::from_millis(500);

        monitor.accept_event(vec![v.clone()], start.elapsed());
        let res = queue.recv_timeout(timeout).unwrap();

        assert!(res.kind == VerdictKind::Event);
        let res = res.verdict;
        assert_eq!(res.inputs[0], Some(v));
        assert_eq!(res.outputs[0][0], (None, Some(Value::Bool(true))));
        assert_eq!(res.outputs[1][0], (None, Some(Value::Unsigned(3))));
        assert_eq!(res.outputs[2][0], (None, Some(Value::Signed(-5))));
        assert_eq!(res.outputs[3][0], (None, Some(Value::new_float(-123.456))));
        assert_eq!(res.outputs[4][0], (None, Some(Value::Str("foobar".into()))));
    }

    #[test]
    fn test_count_window() {
        let (_, mut monitor) =
            setup::<Incremental>("input a: UInt16\noutput b: UInt16 @0.25Hz := a.aggregate(over: 40s, using: #)");

        let timeout = Duration::from_millis(500);
        let output = monitor.output_queue();
        monitor.start();
        let n = 25;
        let mut time = Duration::from_secs(45);
        monitor.accept_event(vec![Value::Unsigned(1)], time);

        let res: Vec<_> = (0..11).map(|_| {
            output.recv_timeout(timeout).unwrap()
        }).collect();
        assert!(output.is_empty());

        assert!(res.iter().all(|v| v.kind == VerdictKind::Timed));
        assert!(res.iter().all(|QueuedVerdict{ts, verdict, ..}| {
            ts.as_secs() % 4 == 0 && verdict[0].0 == 0 && verdict[0].1[0] == Change::Value(None, Value::Unsigned(0))
        }));
        for v in 2..=n {
            time += Duration::from_secs(1);
            monitor.accept_event(vec![Value::Unsigned(v)], time);
            if (v - 1) % 4 == 0 {
                let res = output.recv_timeout(timeout).unwrap();
                assert_eq!(res.kind, VerdictKind::Timed);
                assert_eq!(res.verdict[0].1[0], Change::Value(None, Value::Unsigned(v - 1)));
            } else {
                assert!(output.is_empty());
            }
        }
    }

    #[test]
    fn test_spawn_eventbased() {
        let (_, mut monitor) = setup::<Total>(
            "input a: Int32\n\
                  input b: Int32\n\
                  output c(x: Int32) spawn with a eval with x + a\n\
                  output d := b",
        );

        let timeout = Duration::from_millis(500);
        let output = monitor.output_queue();
        monitor.start();
        monitor.accept_event(vec![Value::Signed(15), Value::None], Duration::from_secs(1));
        let res = output.recv_timeout(timeout).unwrap();

        let expected = Total {
            inputs: vec![Some(Value::Signed(15)), None],
            outputs: vec![
                vec![(Some(vec![Value::Signed(15)]), Some(Value::Signed(30)))],
                vec![(None, None)],
            ],
        };
        assert_eq!(res.kind, VerdictKind::Event);
        assert_eq!(sort_total(res.verdict), sort_total(expected));

        monitor.accept_event(vec![Value::Signed(20), Value::Signed(7)], Duration::from_secs(2));
        let res = output.recv_timeout(timeout).unwrap();

        let expected = Total {
            inputs: vec![Some(Value::Signed(20)), Some(Value::Signed(7))],
            outputs: vec![
                vec![
                    (Some(vec![Value::Signed(15)]), Some(Value::Signed(35))),
                    (Some(vec![Value::Signed(20)]), Some(Value::Signed(40))),
                ],
                vec![(None, Some(Value::Signed(7)))],
            ],
        };
        assert_eq!(res.kind, VerdictKind::Event);
        assert_eq!(sort_total(res.verdict), sort_total(expected));

        monitor.accept_event(vec![Value::None, Value::Signed(42)], Duration::from_secs(3));
        let res = output.recv_timeout(timeout).unwrap();

        let expected = Total {
            inputs: vec![Some(Value::Signed(20)), Some(Value::Signed(42))],
            outputs: vec![
                vec![
                    (Some(vec![Value::Signed(15)]), Some(Value::Signed(35))),
                    (Some(vec![Value::Signed(20)]), Some(Value::Signed(40))),
                ],
                vec![(None, Some(Value::Signed(42)))],
            ],
        };
        assert_eq!(res.kind, VerdictKind::Event);
        assert_eq!(sort_total(res.verdict), sort_total(expected));

    }

    #[test]
    fn test_eval_close() {
        let (_, mut monitor) = setup::<Incremental>(
            "input a: Int32\n\
                  output c(x: Int32)\n\
                    spawn with a \n\
                    close @a when true\n\
                    eval with x + a",
        );

        let timeout = Duration::from_millis(500);
        let output = monitor.output_queue();
        monitor.start();
        monitor.accept_event(vec![Value::Signed(15)], Duration::from_secs(1));
        let res = output.recv_timeout(timeout).unwrap();

        let mut expected = vec![
            Change::Spawn(vec![Value::Signed(15)]),
            Change::Value(Some(vec![Value::Signed(15)]), Value::Signed(30)),
            Change::Close(vec![Value::Signed(15)]),
        ];
        expected.sort();
        assert_eq!(res.kind, VerdictKind::Event);
        assert_eq!(res.verdict[0].0, 0);

        assert_eq!(sort_incremental(res.verdict)[0].1, expected);
    }
}
