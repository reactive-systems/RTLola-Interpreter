//! The [QueuedMonitor] is the multi-threaded version of the API.
//! Deadlines are evaluated immediately and the resulting verdicts are returned through a queue retrieved using the [output_queue](QueuedMonitor::output_queue) method.
//! This API should be used in an online monitoring setting.
//!
//! The [QueuedMonitor] is parameterized over its input and output method.
//! The preferred method to create an API is using the [ConfigBuilder](crate::ConfigBuilder) and the [queued_monitor](crate::ConfigBuilder::queued_monitor) method.
//!
//! # Input Method
//! An input method has to implement the [Input] trait. Out of the box two different methods are provided:
//! * [EventInput](crate::monitor::EventInput): Provides a basic input method for anything that already is an [Event](crate::monitor::Event) or that can be transformed into one using `Into<Event>`.
//! * [RecordInput](crate::monitor::RecordInput): Is a more elaborate input method. It allows to provide a custom data structure to the monitor as an input, as long as it implements the [Record](crate::monitor::Record) trait.
//!     If implemented this traits provides functionality to generate a new value for any input stream from the data structure.
//!
//! # Output Method
//! The [QueuedMonitor] can provide output with a varying level of detail captured by the [VerdictRepresentation](crate::monitor::VerdictRepresentation) trait. The different output formats are:
//! * [Incremental]: For each processed event a condensed list of monitor state changes is provided.
//! * [Total](crate::monitor::Total): For each event a complete snapshot of the current monitor state is returned
//! * [TotalIncremental](crate::monitor::TotalIncremental): For each processed event a complete list of monitor state changes is provided
//! * [TriggerMessages](crate::monitor::TriggerMessages): For each event a list of violated triggers with their description is produced.
//! * [TriggersWithInfoValues](crate::monitor::TriggersWithInfoValues): For each event a list of violated triggers with their specified corresponding values is returned.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Debug;
use std::ops::Not;
use std::rc::Rc;
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use crossbeam_channel::{bounded, unbounded, Sender, TrySendError};
pub use crossbeam_channel::{Receiver, RecvError, RecvTimeoutError, TryRecvError};
use rtlola_frontend::mir::{InputReference, OutputReference, RtLolaMir, Type};
#[cfg(feature = "serde")]
use serde::Serialize;

use crate::config::{Config, ExecutionMode};
use crate::configuration::time::{init_start_time, OutputTimeRepresentation, RelativeFloat, TimeRepresentation};
use crate::evaluator::{Evaluator, EvaluatorData};
use crate::monitor::{Incremental, Input, RawVerdict, Tracer, VerdictRepresentation, Verdicts};
use crate::schedule::schedule_manager::ScheduleManager;
use crate::schedule::DynamicSchedule;
use crate::Monitor;

/// Represents the kind of the verdict. I.e. whether the evaluation was triggered by an event, or by a deadline.
#[cfg_attr(feature = "serde", derive(Serialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictKind {
    /// The verdict resulted from a deadline evaluation.
    Timed,
    /// The verdict resulted from the evaluation of an event.
    Event,
}

/// Represents the length of a queue used for communication.
/// Bounding its length can be useful in resource constraint environments.
#[derive(Debug, Clone, Copy)]
pub enum QueueLength {
    /// There is no bound on the queue.
    Unbounded,
    /// The queue is bounded to keep at most this many elements.
    Bounded(usize),
}

impl QueueLength {
    fn to_queue<T>(self) -> (Sender<T>, Receiver<T>) {
        match self {
            QueueLength::Unbounded => unbounded(),
            QueueLength::Bounded(cap) => bounded(cap),
        }
    }
}

/// The verdict of the queued monitor. It is either triggered by a deadline or an event described by the `kind` field.
/// The time when the verdict occurred is given by `ts`. `verdict` finally describes the changes to input and output streams
/// as defined by the [VerdictRepresentation].
#[cfg_attr(feature = "serde", derive(Serialize))]
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
It can compute streams based on new events through [accept_event](QueuedMonitor::accept_event) once the [start](QueuedMonitor::start) function was invoked.
Timed streams are evaluated automatically at their deadline. The resulting verdicts of events and deadlines are returned through a [Receiver] which can be obtained through [output_queue](QueuedMonitor::output_queue).
Note that the [start](QueuedMonitor::start) function *has* to be invoked before any event can be evaluated.
Finally, a calling [end](QueuedMonitor::end) will block until all events have been evaluated.

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
    worker: JoinHandle<()>,

    input: Sender<WorkItem<Source, SourceTime>>,
    output: Receiver<QueuedVerdict<Verdict, VerdictTime>>,
}

impl<Source, SourceTime, Verdict, VerdictTime> QueuedMonitor<Source, SourceTime, Verdict, VerdictTime>
where
    Source: Input + 'static,
    SourceTime: TimeRepresentation,
    Verdict: VerdictRepresentation,
    VerdictTime: OutputTimeRepresentation,
{
    /// setup the api, while providing bounds for the queues.
    pub fn bounded_setup(
        config: Config<SourceTime, VerdictTime>,
        setup_data: Source::CreationData,
        input_queue_bound: QueueLength,
        output_queue_bound: QueueLength,
    ) -> QueuedMonitor<Source, SourceTime, Verdict, VerdictTime> {
        let config_clone = config.clone();

        let input_map = config
            .ir
            .inputs
            .iter()
            .map(|i| (i.name.clone(), i.reference.in_ix()))
            .collect();

        let (input_send, input_rcv) = input_queue_bound.to_queue();
        let (output_send, output_rcv) = output_queue_bound.to_queue();

        let worker = match config.mode {
            ExecutionMode::Offline => {
                thread::spawn(move || {
                    Self::runner::<OfflineWorker<Source, SourceTime, Verdict, VerdictTime>>(
                        config_clone,
                        input_map,
                        setup_data,
                        input_rcv,
                        output_send,
                    )
                })
            },
            ExecutionMode::Online => {
                thread::spawn(move || {
                    Self::runner::<OnlineWorker<Source, SourceTime, Verdict, VerdictTime>>(
                        config_clone,
                        input_map,
                        setup_data,
                        input_rcv,
                        output_send,
                    )
                })
            },
        };

        QueuedMonitor {
            ir: config.ir,
            worker,

            input: input_send,
            output: output_rcv,
        }
    }

    /// setup the api
    pub fn setup(
        config: Config<SourceTime, VerdictTime>,
        setup_data: Source::CreationData,
    ) -> QueuedMonitor<Source, SourceTime, Verdict, VerdictTime> {
        Self::bounded_setup(config, setup_data, QueueLength::Unbounded, QueueLength::Unbounded)
    }

    fn runner<W: Worker<Source, SourceTime, Verdict, VerdictTime>>(
        config: Config<SourceTime, VerdictTime>,
        input_names: HashMap<String, InputReference>,
        setup_data: Source::CreationData,
        input: Receiver<WorkItem<Source, SourceTime>>,
        output: Sender<QueuedVerdict<Verdict, VerdictTime>>,
    ) {
        let mut worker = W::setup(config, input_names, setup_data, input.clone(), output);
        worker.wait_for_start(&input);
        drop(input);
        worker.init();
        worker.process();
    }

    /// Starts the evaluation process. This method has to be called before any event is accepted.
    pub fn start(&self) {
        self.input.send(WorkItem::Start).expect("Worker thread hung up!");
    }

    /// This method returns the queue through which the verdicts can be received.
    pub fn output_queue(&self) -> Receiver<QueuedVerdict<Verdict, VerdictTime>> {
        self.output.clone()
    }

    /**
    Schedules a new event for evaluation. The verdict can be received through the Queue return by the [QueuedMonitor::output_queue].
    */
    pub fn accept_event(&mut self, ev: Source::Record, ts: SourceTime::InnerTime) {
        self.input
            .send(WorkItem::Event(ev, ts))
            .expect("Worker thread hung up!");
    }

    /// Ends the evaluation process and blocks until all events are processed.
    pub fn end(self) {
        let QueuedMonitor { worker, input, .. } = self;
        // Drop the sender of the input queue
        drop(input);
        // wait for worker to finish processing all events left in input queue
        worker.join().expect("Failed to join on worker thread.");
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

enum WorkItem<Source: Input, SourceTime: TimeRepresentation> {
    Start,
    Event(Source::Record, SourceTime::InnerTime),
}

trait Worker<Source, SourceTime, Verdict, VerdictTime>
where
    Source: Input,
    SourceTime: TimeRepresentation,
    Verdict: VerdictRepresentation,
    VerdictTime: OutputTimeRepresentation + 'static,
{
    fn setup(
        config: Config<SourceTime, VerdictTime>,
        input_names: HashMap<String, InputReference>,
        setup_data: Source::CreationData,
        input: Receiver<WorkItem<Source, SourceTime>>,
        output: Sender<QueuedVerdict<Verdict, VerdictTime>>,
    ) -> Self;

    fn wait_for_start(&mut self, input: &Receiver<WorkItem<Source, SourceTime>>) {
        // Wait for Start command
        loop {
            match input.recv() {
                Ok(WorkItem::Start) => break,
                Ok(WorkItem::Event(_, _)) => panic!("Received Event before 'start' was called"),
                Err(_) => return,
            }
        }
    }

    fn init(&mut self);

    fn process(&mut self);

    fn try_send(
        output: &Sender<QueuedVerdict<Verdict, VerdictTime>>,
        verdict: Option<QueuedVerdict<Verdict, VerdictTime>>,
    ) {
        if let Some(verdict) = verdict {
            if let Err(e) = output.try_send(verdict) {
                match e {
                    TrySendError::Full(_) => println!("Output queue overloaded! Verdict lost..."),
                    TrySendError::Disconnected(_) => println!("Output queue disconnected! Verdict lost..."),
                }
            }
        }
    }
}

struct OnlineWorker<Source, SourceTime, Verdict, VerdictTime>
where
    Source: Input,
    SourceTime: TimeRepresentation,
    Verdict: VerdictRepresentation,
    VerdictTime: OutputTimeRepresentation + 'static,
{
    source: Source,
    source_time: SourceTime,
    output_time: Option<VerdictTime>,
    start_time: Option<SystemTime>,

    schedule_manager: ScheduleManager,
    evaluator: Evaluator,
    input: Receiver<WorkItem<Source, SourceTime>>,
    output: Sender<QueuedVerdict<Verdict, VerdictTime>>,
}

impl<
        Source: Input,
        SourceTime: TimeRepresentation,
        Verdict: VerdictRepresentation,
        VerdictTime: OutputTimeRepresentation,
    > Worker<Source, SourceTime, Verdict, VerdictTime> for OnlineWorker<Source, SourceTime, Verdict, VerdictTime>
{
    fn setup(
        config: Config<SourceTime, VerdictTime>,
        input_names: HashMap<String, InputReference>,
        setup_data: Source::CreationData,
        input: Receiver<WorkItem<Source, SourceTime>>,
        output: Sender<QueuedVerdict<Verdict, VerdictTime>>,
    ) -> Self {
        // setup monitor
        let source_time = config.input_time_representation;
        let source = Source::new(input_names, setup_data);

        // Setup evaluator
        let dyn_schedule = Rc::new(RefCell::new(DynamicSchedule::new()));
        let eval_data = EvaluatorData::new(config.ir.clone(), dyn_schedule.clone());
        let schedule_manager = ScheduleManager::setup(config.ir.clone(), dyn_schedule)
            .expect("Error computing schedule for time-driven streams");
        let evaluator = eval_data.into_evaluator();

        OnlineWorker {
            source,
            source_time,
            output_time: None,
            start_time: config.start_time,
            schedule_manager,
            evaluator,
            input,
            output,
        }
    }

    fn init(&mut self) {
        init_start_time::<SourceTime>(self.start_time);
        self.output_time.replace(VerdictTime::default());
    }

    fn process(&mut self) {
        let output_time = self.output_time.as_mut().expect("Init to be executed before process");
        loop {
            let next_deadline = self.schedule_manager.get_next_due();
            let item = if let Some(due) = next_deadline {
                self.input.recv_timeout(due)
            } else {
                self.input.recv().map_err(|_| RecvTimeoutError::Disconnected)
            };
            let verdict = match item {
                Ok(WorkItem::Event(e, ts)) => {
                    // Received Event before deadline
                    let e = self.source.get_event(e);
                    let ts = self.source_time.convert_from(ts);

                    let mut tracer = Verdict::Tracing::default();
                    tracer.eval_start();
                    self.evaluator.eval_event(&e, ts, &mut tracer);
                    tracer.eval_end();

                    let verdict = Verdict::create_with_trace(RawVerdict::from(&self.evaluator), tracer);
                    verdict.is_empty().not().then_some(QueuedVerdict {
                        kind: VerdictKind::Event,
                        ts: output_time.convert_into(ts),
                        verdict,
                    })
                },
                Err(RecvTimeoutError::Timeout) => {
                    // Deadline occurred before event
                    let mut tracer = Verdict::Tracing::default();
                    tracer.eval_start();
                    let due = next_deadline.expect("timeout to only happen for a deadline.");

                    let deadline = self.schedule_manager.get_next_deadline(due);
                    self.evaluator.eval_time_driven_tasks(deadline, due, &mut tracer);
                    tracer.eval_end();

                    let verdict = Verdict::create_with_trace(RawVerdict::from(&self.evaluator), tracer);
                    verdict.is_empty().not().then_some(QueuedVerdict {
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

            Self::try_send(&self.output, verdict);
        }
    }
}

struct OfflineWorker<Source, SourceTime, Verdict, VerdictTime>
where
    Source: Input,
    SourceTime: TimeRepresentation,
    Verdict: VerdictRepresentation,
    VerdictTime: OutputTimeRepresentation + 'static,
{
    config: Config<SourceTime, VerdictTime>,
    setup_data: Source::CreationData,

    monitor: Option<Monitor<Source, SourceTime, Verdict, VerdictTime>>,
    input: Receiver<WorkItem<Source, SourceTime>>,
    output: Sender<QueuedVerdict<Verdict, VerdictTime>>,
}

impl<
        Source: Input,
        SourceTime: TimeRepresentation,
        Verdict: VerdictRepresentation,
        VerdictTime: OutputTimeRepresentation,
    > Worker<Source, SourceTime, Verdict, VerdictTime> for OfflineWorker<Source, SourceTime, Verdict, VerdictTime>
{
    fn setup(
        config: Config<SourceTime, VerdictTime>,
        _input_names: HashMap<String, InputReference>,
        setup_data: Source::CreationData,
        input: Receiver<WorkItem<Source, SourceTime>>,
        output: Sender<QueuedVerdict<Verdict, VerdictTime>>,
    ) -> Self {
        OfflineWorker {
            config,
            setup_data,
            monitor: None,
            input,
            output,
        }
    }

    fn init(&mut self) {
        // Setup evaluator
        let monitor: Monitor<Source, SourceTime, Verdict, VerdictTime> =
            Monitor::setup(self.config.clone(), self.setup_data.clone());
        self.monitor.replace(monitor);
    }

    fn process(&mut self) {
        let monitor = self.monitor.as_mut().expect("Init to be called before process");
        let mut last_event = None;
        let mut done = false;
        while !done {
            let timed = match self.input.recv() {
                Ok(WorkItem::Event(e, ts)) => {
                    // Received Event
                    last_event.replace(ts.clone());
                    let Verdicts { timed, event } = monitor.accept_event(e, ts);
                    let ts = monitor.last_event().expect("the event to be recorded");

                    if !event.is_empty() {
                        let verdict = QueuedVerdict {
                            kind: VerdictKind::Event,
                            ts: ts.clone(),
                            verdict: event,
                        };
                        Self::try_send(&self.output, Some(verdict));
                    }

                    timed
                },
                Err(_) => {
                    // Channel closed, we are done here
                    done = true;
                    if let Some(last_event) = last_event.as_ref() {
                        monitor.accept_time(last_event.clone())
                    } else {
                        return;
                    }
                },
                Ok(WorkItem::Start) => {
                    // Received second start command -> abort
                    panic!("Received second start command.")
                },
            };

            for (ts, v) in timed {
                let verdict = QueuedVerdict {
                    kind: VerdictKind::Timed,
                    ts,
                    verdict: v,
                };
                Self::try_send(&self.output, Some(verdict));
            }
        }
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

        let res: Vec<_> = (0..11).map(|_| output.recv_timeout(timeout).unwrap()).collect();
        assert!(output.is_empty());

        assert!(res.iter().all(|v| v.kind == VerdictKind::Timed));
        assert!(res.iter().all(|QueuedVerdict { ts, verdict, .. }| {
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