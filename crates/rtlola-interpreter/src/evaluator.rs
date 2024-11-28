use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Not;
use std::rc::Rc;
use std::time::Duration;

use bit_set::BitSet;
use itertools::Itertools;
use num::traits::Inv;
use rtlola_frontend::mir::{
    ActivationCondition as Activation, InputReference, OutputKind, OutputReference, PacingLocality, PacingType,
    RtLolaMir, Stream, StreamReference, Task, TimeDrivenStream, Trigger, TriggerReference, WindowReference,
};
use uom::si::rational64::Time as UOM_Time;
use uom::si::time::nanosecond;

use crate::api::monitor::{Change, Instance};
use crate::closuregen::{CompiledExpr, Expr};
use crate::monitor::{Parameters, Tracer};
use crate::schedule::{DynamicSchedule, EvaluationTask};
use crate::storage::{
    GlobalStore, InstanceAggregationTrait, InstanceStore, Value, WindowParameterization, WindowParameterizationKind,
};
use crate::Time;

/// Enum to describe the activation condition of a stream; If the activation condition is described by a conjunction, the evaluator uses a bitset representation.
#[derive(Debug)]
pub(crate) enum ActivationConditionOp {
    TimeDriven,
    True,
    Conjunction(BitSet),
    General(Activation),
}

pub(crate) struct EvaluatorData {
    // Evaluation order of output streams
    layers: Vec<Vec<Task>>,
    // Accessed by stream index
    stream_activation_conditions: Vec<ActivationConditionOp>,
    spawn_activation_conditions: Vec<ActivationConditionOp>,
    close_activation_conditions: Vec<ActivationConditionOp>,
    stream_windows: HashMap<StreamReference, Vec<WindowReference>>,
    stream_instance_aggregations: HashMap<StreamReference, Vec<WindowReference>>,
    global_store: GlobalStore,
    fresh_inputs: BitSet,
    fresh_outputs: BitSet,
    spawned_outputs: BitSet,
    closed_outputs: BitSet,
    fresh_triggers: BitSet,
    triggers: Vec<Option<Trigger>>,
    time_driven_streams: Vec<Option<TimeDrivenStream>>,
    closing_streams: Vec<OutputReference>,
    ir: RtLolaMir,
    dyn_schedule: Rc<RefCell<DynamicSchedule>>,
}

#[allow(missing_debug_implementations)]
pub(crate) struct Evaluator {
    // Evaluation order of output streams
    layers: &'static [Vec<Task>],
    // Indexed by stream reference.
    stream_activation_conditions: &'static [ActivationConditionOp],
    // Indexed by output reference
    spawn_activation_conditions: &'static [ActivationConditionOp],
    // Indexed by output reference
    close_activation_conditions: &'static [ActivationConditionOp],
    // Accessed by stream index
    // If Value::None is returned by an expression the filter was false
    compiled_stream_exprs: Vec<CompiledExpr>,
    // Accessed by stream index
    // If Value::None is returned, the spawn condition was false.
    // If a stream has no spawn target, then an empty tuple is returned if the condition is true
    compiled_spawn_exprs: Vec<CompiledExpr>,
    // Accessed by stream index
    compiled_close_exprs: Vec<CompiledExpr>,
    stream_windows: &'static HashMap<StreamReference, Vec<WindowReference>>,
    stream_instance_aggregations: &'static HashMap<StreamReference, Vec<WindowReference>>,
    global_store: &'static mut GlobalStore,
    fresh_inputs: &'static mut BitSet,
    fresh_outputs: &'static mut BitSet,
    spawned_outputs: &'static mut BitSet,
    closed_outputs: &'static mut BitSet,
    fresh_triggers: &'static mut BitSet,
    // Indexed by output reference
    triggers: &'static [Option<Trigger>],
    // Indexed by output reference
    time_driven_streams: &'static [Option<TimeDrivenStream>],

    closing_streams: &'static [OutputReference],
    ir: &'static RtLolaMir,
    dyn_schedule: &'static RefCell<DynamicSchedule>,
    raw_data: *mut EvaluatorData,
}

pub(crate) struct EvaluationContext<'e> {
    ts: Time,
    global_store: &'e GlobalStore,
    fresh_inputs: &'e BitSet,
    fresh_outputs: &'e BitSet,
    pub(crate) parameter: Vec<Value>,
}

impl EvaluatorData {
    pub(crate) fn new(ir: RtLolaMir, dyn_schedule: Rc<RefCell<DynamicSchedule>>) -> Self {
        // Layers of event based output streams
        let layers: Vec<Vec<Task>> = ir.get_event_driven_layers();
        let closing_streams = ir
            .outputs
            .iter()
            .filter(|s| s.close.condition.is_some())
            .map(|s| s.reference.out_ix())
            .collect();
        let stream_acs = ir
            .outputs
            .iter()
            .map(|o| {
                match &o.eval.eval_pacing {
                    PacingType::GlobalPeriodic(_) | PacingType::LocalPeriodic(_) => ActivationConditionOp::TimeDriven,
                    PacingType::Event(ac) => ActivationConditionOp::new(ac, ir.inputs.len()),
                    PacingType::Constant => ActivationConditionOp::True,
                }
            })
            .collect();
        let spawn_acs = ir
            .outputs
            .iter()
            .map(|o| {
                match &o.spawn.pacing {
                    PacingType::GlobalPeriodic(_) | PacingType::LocalPeriodic(_) => ActivationConditionOp::TimeDriven,
                    PacingType::Event(ac) => ActivationConditionOp::new(ac, ir.inputs.len()),
                    PacingType::Constant => ActivationConditionOp::True,
                }
            })
            .collect();
        let close_acs = ir
            .outputs
            .iter()
            .map(|o| {
                match &o.close.pacing {
                    PacingType::GlobalPeriodic(_) | PacingType::LocalPeriodic(_) => ActivationConditionOp::TimeDriven,
                    PacingType::Event(ac) => ActivationConditionOp::new(ac, ir.inputs.len()),
                    PacingType::Constant => ActivationConditionOp::True,
                }
            })
            .collect();

        let global_store = GlobalStore::new(&ir);
        let fresh_inputs = BitSet::with_capacity(ir.inputs.len());
        let fresh_outputs = BitSet::with_capacity(ir.outputs.len());
        let spawned_outputs = BitSet::with_capacity(ir.outputs.len());
        let closed_outputs = BitSet::with_capacity(ir.outputs.len());
        let fresh_triggers = BitSet::with_capacity(ir.outputs.len());
        let mut triggers = vec![None; ir.outputs.len()];

        let stream_windows = ir
            .sliding_windows
            .iter()
            .map(|w| (w.caller, w.reference))
            .chain(ir.discrete_windows.iter().map(|w| (w.caller, w.reference)))
            .into_group_map();

        let stream_instance_aggregations = ir
            .instance_aggregations
            .iter()
            .map(|ia| (ia.target, ia.reference))
            .into_group_map();

        for t in &ir.triggers {
            triggers[t.output_reference.out_ix()] = Some(*t);
        }
        let mut time_driven_streams = vec![None; ir.outputs.len()];
        for t in &ir.time_driven {
            time_driven_streams[t.reference.out_ix()] = Some(*t);
        }
        EvaluatorData {
            layers,
            stream_activation_conditions: stream_acs,
            spawn_activation_conditions: spawn_acs,
            close_activation_conditions: close_acs,
            stream_windows,
            stream_instance_aggregations,
            global_store,
            fresh_inputs,
            fresh_outputs,
            spawned_outputs,
            closed_outputs,
            fresh_triggers,
            triggers,
            time_driven_streams,
            closing_streams,
            ir,
            dyn_schedule,
        }
    }

    pub(crate) fn into_evaluator(self) -> Evaluator {
        let mut on_heap = Box::new(self);
        // Store pointer to data so we can delete it in implementation of Drop trait.
        // This is necessary since we leak the evaluator data.
        let heap_ptr: *mut EvaluatorData = &mut *on_heap;
        let leaked_data: &'static mut EvaluatorData = Box::leak(on_heap);

        //Compile expressions
        let compiled_stream_exprs = leaked_data
            .ir
            .outputs
            .iter()
            .map(|o| {
                let clauses = o
                    .eval
                    .clauses
                    .iter()
                    .map(|clause| {
                        let exp = match &clause.condition {
                        None => clause.expression.clone().compile(),
                        Some(filter_exp) => CompiledExpr::create_filter(
                            filter_exp.clone().compile(),
                            clause.expression.clone().compile(),
                        ),
                    };
                    if clause.pacing != o.eval.eval_pacing {
                        if let PacingType::Event(ac) = &clause.pacing {
                        CompiledExpr::create_activation(exp, ActivationConditionOp::new(ac, leaked_data.ir.inputs.len()))
                        } else {
                            unreachable!("different pacing types of multiple eval clauses are only supported for event-driven streams. This is ensured by the frontend.")
                        }
                    } else {
                        exp
                    }
                })
                    .collect::<Vec<_>>();
                if clauses.len() == 1 {
                    clauses.into_iter().next().expect("has exactly one element")
                } else {
                    CompiledExpr::create_clauses(clauses)
                }
            })
            .collect();

        let compiled_spawn_exprs = leaked_data
            .ir
            .outputs
            .iter()
            .map(|o| {
                match (o.spawn.expression.as_ref(), o.spawn.condition.as_ref()) {
                    (None, None) => CompiledExpr::new(|_| Value::Tuple(vec![].into_boxed_slice())),
                    (Some(target), None) => target.clone().compile(),
                    (None, Some(condition)) => {
                        CompiledExpr::create_filter(
                            condition.clone().compile(),
                            CompiledExpr::new(|_| Value::Tuple(vec![].into_boxed_slice())),
                        )
                    },
                    (Some(target), Some(condition)) => {
                        CompiledExpr::create_filter(condition.clone().compile(), target.clone().compile())
                    },
                }
            })
            .collect();

        let compiled_close_exprs = leaked_data
            .ir
            .outputs
            .iter()
            .map(|o| {
                o.close
                    .condition
                    .as_ref()
                    .map_or(CompiledExpr::new(|_| Value::None), |e| e.clone().compile())
            })
            .collect();

        Evaluator {
            layers: &leaked_data.layers,
            stream_activation_conditions: &leaked_data.stream_activation_conditions,
            spawn_activation_conditions: &leaked_data.spawn_activation_conditions,
            close_activation_conditions: &leaked_data.close_activation_conditions,
            compiled_stream_exprs,
            compiled_spawn_exprs,
            compiled_close_exprs,
            stream_windows: &leaked_data.stream_windows,
            stream_instance_aggregations: &leaked_data.stream_instance_aggregations,
            global_store: &mut leaked_data.global_store,
            fresh_inputs: &mut leaked_data.fresh_inputs,
            fresh_outputs: &mut leaked_data.fresh_outputs,
            spawned_outputs: &mut leaked_data.spawned_outputs,
            closed_outputs: &mut leaked_data.closed_outputs,
            fresh_triggers: &mut leaked_data.fresh_triggers,
            triggers: &leaked_data.triggers,
            time_driven_streams: &leaked_data.time_driven_streams,
            closing_streams: &leaked_data.closing_streams,
            ir: &leaked_data.ir,
            dyn_schedule: &leaked_data.dyn_schedule,
            raw_data: heap_ptr,
        }
    }
}

impl Drop for Evaluator {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        drop(unsafe { Box::from_raw(self.raw_data) });
    }
}

impl Evaluator {
    /// Values of event are expected in the order of the input streams
    /// Time should be relative to the starting time of the monitor
    pub(crate) fn eval_event(&mut self, event: &[Value], ts: Time, tracer: &mut impl Tracer) {
        self.new_cycle(ts);
        self.accept_inputs(event, ts);
        self.eval_event_driven(ts, tracer);
    }

    /// NOT for external use because the values are volatile
    pub(crate) fn peek_fresh_outputs(&self) -> Vec<(OutputReference, Vec<Change>)> {
        self.ir
            .outputs
            .iter()
            .filter_map(|o| {
                let stream = o.reference;
                let out_ix = o.reference.out_ix();
                let changes = if o.is_parameterized() {
                    let instances = self.global_store.get_out_instance_collection(out_ix);
                    instances
                        .spawned()
                        .map(|p| Change::Spawn(p.clone()))
                        .chain(instances.fresh().map(|p| {
                            Change::Value(Some(p.clone()), self.peek_value(stream, p, 0).expect("Marked as fresh"))
                        }))
                        .chain(instances.closed().map(|p| Change::Close(p.clone())))
                        .collect()
                } else if o.is_spawned() {
                    let mut res = Vec::new();
                    if self.spawned_outputs.contains(out_ix) {
                        res.push(Change::Spawn(vec![]));
                    }
                    if self.fresh_outputs.contains(out_ix) {
                        res.push(Change::Value(
                            Some(vec![]),
                            self.peek_value(stream, &[], 0).expect("Marked as fresh"),
                        ));
                    }
                    if self.closed_outputs.contains(out_ix) {
                        res.push(Change::Close(vec![]));
                    }
                    res
                } else if self.fresh_outputs.contains(out_ix) {
                    vec![Change::Value(
                        None,
                        self.peek_value(stream, &[], 0).expect("Marked as fresh"),
                    )]
                } else {
                    vec![]
                };
                changes.is_empty().not().then(|| (o.reference.out_ix(), changes))
            })
            .collect()
    }

    /// NOT for external use because the values are volatile
    pub(crate) fn peek_violated_triggers_messages(&self) -> Vec<(OutputReference, Parameters, String)> {
        self.peek_fresh_outputs()
            .into_iter()
            .filter(|(o_ref, _)| matches!(self.ir.outputs[*o_ref].kind, OutputKind::Trigger(_)))
            .flat_map(|(o_ref, changes)| {
                changes.into_iter().filter_map(move |change| {
                    match change {
                        Change::Value(parameters, Value::Str(msg)) => Some((o_ref, parameters, msg.into())),
                        Change::Value(_, _) => unreachable!("trigger values are strings; checked by the frontend"),
                        _ => None,
                    }
                })
            })
            .collect()
    }

    /// NOT for external use because the values are volatile
    pub(crate) fn peek_violated_triggers(&self) -> Vec<TriggerReference> {
        self.fresh_triggers.iter().collect()
    }

    /// NOT for external use because the values are volatile
    pub(crate) fn peek_fresh_input(&self) -> Vec<(InputReference, Value)> {
        self.fresh_inputs
            .iter()
            .map(|i| {
                (
                    i,
                    self.peek_value(StreamReference::In(i), &[], 0)
                        .expect("Marked as fresh"),
                )
            })
            .collect()
    }

    /// NOT for external use because the values are volatile
    pub(crate) fn peek_inputs(&self) -> Vec<Option<Value>> {
        self.ir
            .inputs
            .iter()
            .map(|elem| self.peek_value(elem.reference, &[], 0))
            .collect()
    }

    /// NOT for external use because the values are volatile
    pub(crate) fn peek_outputs(&self) -> Vec<Vec<Instance>> {
        self.ir
            .outputs
            .iter()
            .map(|elem| {
                if elem.is_parameterized() {
                    let ix = elem.reference.out_ix();
                    let values: Vec<Instance> = self
                        .global_store
                        .get_out_instance_collection(ix)
                        .all_parameter()
                        .map(|para| (Some(para.clone()), self.peek_value(elem.reference, para.as_ref(), 0)))
                        .collect();
                    values
                } else if elem.is_spawned() {
                    vec![(Some(vec![]), self.peek_value(elem.reference, &[], 0))]
                } else {
                    vec![(None, self.peek_value(elem.reference, &[], 0))]
                }
            })
            .collect()
    }

    fn accept_inputs(&mut self, event: &[Value], ts: Time) {
        for (ix, v) in event.iter().enumerate() {
            match v {
                Value::None => {},
                v => self.accept_input(ix, v.clone(), ts),
            }
        }
    }

    fn accept_input(&mut self, input: InputReference, v: Value, ts: Time) {
        self.global_store.get_in_instance_mut(input).push_value(v.clone());
        self.fresh_inputs.insert(input);
        let extended = &self.ir.inputs[input];
        for (_sr, win) in &extended.aggregated_by {
            self.extend_window(&[], *win, v.clone(), ts);
        }
    }

    fn eval_event_driven(&mut self, ts: Time, tracer: &mut impl Tracer) {
        self.prepare_evaluation(ts);
        for layer in self.layers {
            self.eval_event_driven_layer(layer, ts, tracer);
        }
        for close in self.closing_streams {
            let ac = &self.close_activation_conditions[*close];
            if ac.is_eventdriven() && ac.eval(self.fresh_inputs) {
                if self.ir.output(StreamReference::Out(*close)).is_parameterized() {
                    let stream_instances: Vec<Vec<Value>> = self
                        .global_store
                        .get_out_instance_collection(*close)
                        .all_parameter()
                        .cloned()
                        .collect();
                    for instance in stream_instances {
                        tracer.close_start(*close, instance.as_slice());
                        self.eval_close(*close, instance.as_slice(), ts);
                        tracer.close_end(*close, instance.as_slice());
                    }
                } else if self.global_store.get_out_instance(*close).is_active() {
                    tracer.close_start(*close, &[]);
                    self.eval_close(*close, &[], ts);
                    tracer.close_end(*close, &[]);
                }
            }
        }
    }

    fn eval_event_driven_layer(&mut self, tasks: &[Task], ts: Time, tracer: &mut impl Tracer) {
        for task in tasks {
            match task {
                Task::Evaluate(idx) => self.eval_event_driven_output(*idx, ts, tracer),
                Task::Spawn(idx) => self.eval_event_driven_spawn(*idx, ts, tracer),
                Task::Close(_) => unreachable!("closes are not included in evaluation layer"),
            }
        }
    }

    fn eval_spawn(&mut self, output: OutputReference, ts: Time) {
        let stream = self.ir.output(StreamReference::Out(output));
        debug_assert!(stream.is_spawned(), "tried to spawn stream that should not be spawned");

        let expr = self.compiled_spawn_exprs[output].clone();
        let ctx = self.as_EvaluationContext(vec![], ts);
        let res = expr.execute(&ctx);

        let parameter_values = match res {
            Value::None => return, // spawn condition evaluated to false
            Value::Tuple(paras) => paras.to_vec(),
            x => vec![x],
        };

        if stream.is_parameterized() {
            debug_assert!(!parameter_values.is_empty());
            let instances = self.global_store.get_out_instance_collection_mut(output);
            if instances.contains(parameter_values.as_slice()) {
                // instance already exists -> nothing to do
                return;
            }
            instances.create_instance(parameter_values.as_slice());
        } else {
            debug_assert!(parameter_values.is_empty());
            let inst = self.global_store.get_out_instance_mut(output);
            if inst.is_active() {
                // instance already exists -> nothing to do
                return;
            }
            inst.activate();
        }

        self.spawn_windows(output, parameter_values.as_slice(), ts);

        // Schedule instance evaluation if stream is periodic
        if let Some(tds) = self.time_driven_streams[output] {
            let mut schedule = (*self.dyn_schedule).borrow_mut();

            // Schedule eval if it has local pacing
            if tds.locality == PacingLocality::Local {
                schedule.schedule_evaluation(output, parameter_values.as_slice(), ts, tds.period_in_duration());
            }

            // Schedule close if it has local pacing
            if let PacingType::LocalPeriodic(f) = stream.close.pacing {
                let period = Duration::from_nanos(
                    UOM_Time::new::<uom::si::time::second>(f.get::<uom::si::frequency::hertz>().inv())
                        .get::<nanosecond>()
                        .to_integer()
                        .try_into()
                        .expect("Period [ns] too large for u64!"),
                );
                schedule.schedule_close(output, parameter_values.as_slice(), ts, period);
            }
        }

        self.spawned_outputs.insert(output);
    }

    fn spawn_windows(&mut self, stream: OutputReference, parameter_values: &[Value], ts: Time) {
        let stream = &self.ir.outputs[stream];
        let own_windows: Vec<WindowReference> = self
            .stream_windows
            .get(&stream.reference)
            .map(|windows| windows.to_vec())
            .unwrap_or_default();

        //activate windows of this stream
        for win_ref in own_windows {
            let WindowParameterization { kind, global } = self.window_parameterization(win_ref);
            let target = self.ir.window(win_ref).target();
            // Self is the caller of the window
            match (kind, global) {
                (WindowParameterizationKind::None | WindowParameterizationKind::Caller, true) => {
                    // Only a single window with a global clock exists. nothing to do...
                },
                (WindowParameterizationKind::None, false) => {
                    // Caller is spawned but not parameterized
                    // activate single window now
                    let (inst, fresh) = match target {
                        StreamReference::In(ix) => {
                            (self.global_store.get_in_instance(ix), self.fresh_inputs.contains(ix))
                        },
                        StreamReference::Out(ix) => {
                            (self.global_store.get_out_instance(ix), self.fresh_outputs.contains(ix))
                        },
                    };
                    let target_value = fresh.then(|| inst.get_value(0).unwrap());
                    let window = self.global_store.get_window_mut(win_ref);

                    if !window.is_active() {
                        window.activate(ts);
                        if let Some(val) = target_value {
                            window.accept_value(val, ts);
                        }
                    }
                },
                (WindowParameterizationKind::Target, false) => {
                    // Caller is spawned and Target is parameterized; window evaluates at local clock
                    // activate all windows registered now
                    let fresh_values = self
                        .global_store
                        .get_out_instance_collection(target.out_ix())
                        .fresh_values();
                    self.global_store
                        .get_window_collection_mut(win_ref)
                        .activate_all(fresh_values, ts, ts);
                },
                (WindowParameterizationKind::Caller, false) => {
                    // Caller is parameterized and windows are evaluated at local clock
                    // Create and activate window instance of the spawned instance
                    let (inst, fresh) = match target {
                        StreamReference::In(ix) => {
                            (self.global_store.get_in_instance(ix), self.fresh_inputs.contains(ix))
                        },
                        StreamReference::Out(ix) => {
                            (self.global_store.get_out_instance(ix), self.fresh_outputs.contains(ix))
                        },
                    };
                    let target_value = fresh.then(|| inst.get_value(0).unwrap());
                    let windows = self.global_store.get_window_collection_mut(win_ref);
                    let window = windows.get_or_create(parameter_values, ts);
                    // Check if target of window produced a value in this iteration and add the value
                    if !window.is_active() {
                        window.activate(ts);
                        if let Some(val) = target_value {
                            window.accept_value(val, ts);
                        }
                    }
                },
                (WindowParameterizationKind::Both, false) => {
                    // target and caller are parameterized and windows are evaluated at local clock
                    // create and activate a new set of sliding windows over all available target instances
                    let fresh_values = self
                        .global_store
                        .get_out_instance_collection(target.out_ix())
                        .fresh_values();
                    let windows = self.global_store.get_two_layer_window_collection_mut(win_ref);
                    windows.spawn_caller_instance(fresh_values, parameter_values, ts, ts);
                },
                (WindowParameterizationKind::Both | WindowParameterizationKind::Target, true) => {
                    // target and caller are parameterized and windows are evaluated at global clock.
                    // nothing to do?
                },
            }
        }

        //create window that aggregate over this stream
        for (_, win_ref) in &stream.aggregated_by {
            let WindowParameterization { kind, global } = self.window_parameterization(*win_ref);
            // Self is target of the window
            match (kind, global) {
                (WindowParameterizationKind::Caller | WindowParameterizationKind::None, _) => {},
                (WindowParameterizationKind::Both | WindowParameterizationKind::Target, true) => {
                    let windows = self.global_store.get_window_collection_mut(*win_ref);
                    let window = windows.get_or_create(parameter_values, ts);
                    // Window is not activated by caller so we assume it to have existed since the beginning.
                    window.activate(Time::default());
                },
                (WindowParameterizationKind::Target, false) => {
                    let windows = self.global_store.get_window_collection_mut(*win_ref);
                    windows.create_window(parameter_values, ts);
                    // Window is activated by caller at spawn
                },
                (WindowParameterizationKind::Both, false) => {
                    let windows = self.global_store.get_two_layer_window_collection_mut(*win_ref);
                    windows.spawn_target_instance(parameter_values);
                    // Windows are activated by caller at spawn
                },
            }
        }
    }

    fn eval_close(&mut self, output: OutputReference, parameter: &[Value], ts: Time) {
        let stream = self.ir.output(StreamReference::Out(output));

        let expr = self.compiled_close_exprs[output].clone();
        let ctx = self.as_EvaluationContext(parameter.to_vec(), ts);
        let res = expr.execute(&ctx);
        if !res.as_bool() {
            return;
        }

        let own_windows: Vec<WindowReference> = self
            .stream_windows
            .get(&stream.reference)
            .map(|windows| windows.to_vec())
            .unwrap_or_default();
        if stream.is_parameterized() {
            // mark instance for closing
            self.global_store
                .get_out_instance_collection_mut(output)
                .mark_for_deletion(parameter);

            for win_ref in own_windows {
                // Self is caller of the window
                match self.window_parameterization(win_ref).kind {
                    WindowParameterizationKind::None | WindowParameterizationKind::Target => unreachable!(),
                    WindowParameterizationKind::Caller => {
                        self.global_store
                            .get_window_collection_mut(win_ref)
                            .delete_window(parameter);
                    },
                    WindowParameterizationKind::Both => {
                        self.global_store
                            .get_two_layer_window_collection_mut(win_ref)
                            .close_caller_instance(parameter);
                    },
                }
            }

            // close all windows referencing this instance
            for (_, win_ref) in &stream.aggregated_by {
                // Self is target of the window
                match self.window_parameterization(*win_ref).kind {
                    WindowParameterizationKind::None | WindowParameterizationKind::Caller => unreachable!(),
                    WindowParameterizationKind::Target => {
                        self.global_store
                            .get_window_collection_mut(*win_ref)
                            .schedule_deletion(parameter, ts);
                    },
                    WindowParameterizationKind::Both => {
                        self.global_store
                            .get_two_layer_window_collection_mut(*win_ref)
                            .close_target_instance(parameter, ts);
                    },
                }
            }
        } else {
            for win_ref in own_windows {
                // Self is caller of the window
                match self.window_parameterization(win_ref).kind {
                    WindowParameterizationKind::None => {
                        self.global_store.get_window_mut(win_ref).deactivate();
                    },
                    WindowParameterizationKind::Target => {
                        self.global_store.get_window_collection_mut(win_ref).deactivate_all();
                    },
                    WindowParameterizationKind::Caller | WindowParameterizationKind::Both => {
                        unreachable!("Parameters are empty")
                    },
                }
            }
        }
        self.closed_outputs.insert(output);

        // Remove instance evaluation from schedule if stream is periodic
        if let Some(tds) = self.time_driven_streams[output] {
            let mut schedule = (*self.dyn_schedule).borrow_mut();
            schedule.remove_evaluation(output, parameter, tds.period_in_duration());

            // Remove close from schedule if it depends on current instance
            if let PacingType::LocalPeriodic(f) = stream.close.pacing {
                let period = Duration::from_nanos(
                    UOM_Time::new::<uom::si::time::second>(f.get::<uom::si::frequency::hertz>().inv())
                        .get::<nanosecond>()
                        .to_integer()
                        .try_into()
                        .expect("Period [ns] too large for u64!"),
                );
                schedule.remove_close(output, parameter, period);
            }
        }
    }

    /// Closes all streams marked for deletion
    fn close_streams(&mut self) {
        for o in self.closed_outputs.iter() {
            if self.ir.output(StreamReference::Out(o)).is_parameterized() {
                let vals = self.global_store.get_out_instance_collection_mut(o).delete_instances();
                if let Some(wrefs) = self.stream_instance_aggregations.get(&StreamReference::Out(o)) {
                    wrefs.iter().for_each(|aggr| {
                        let inst = self.global_store.get_instance_aggregation_mut(*aggr);
                        vals.iter().for_each(|v| {
                            inst.remove_value(v.clone());
                        })
                    })
                }
            } else {
                self.global_store.get_out_instance_mut(o).deactivate();
            }
        }
    }

    fn eval_event_driven_spawn(&mut self, output: OutputReference, ts: Time, tracer: &mut impl Tracer) {
        if self.spawn_activation_conditions[output].eval(self.fresh_inputs) {
            tracer.spawn_start(output);
            self.eval_spawn(output, ts);
            tracer.spawn_end(output);
        }
    }

    fn eval_stream_instances(&mut self, output: OutputReference, ts: Time, tracer: &mut impl Tracer) {
        if self.ir.output(StreamReference::Out(output)).is_parameterized() {
            let parameter: Vec<Vec<Value>> = self
                .global_store
                .get_out_instance_collection(output)
                .all_parameter()
                .cloned()
                .collect();
            for instance in parameter {
                tracer.instance_eval_start(output, instance.as_slice());
                self.eval_stream_instance(output, instance.as_slice(), ts);
                tracer.instance_eval_end(output, instance.as_slice());
            }
        } else if self.global_store.get_out_instance(output).is_active() {
            tracer.instance_eval_start(output, &[]);
            self.eval_stream_instance(output, &[], ts);
            tracer.instance_eval_end(output, &[]);
        }
    }

    fn eval_event_driven_output(&mut self, output: OutputReference, ts: Time, tracer: &mut impl Tracer) {
        if self.stream_activation_conditions[output].eval(self.fresh_inputs) {
            self.eval_stream_instances(output, ts, tracer)
        }
    }

    /// Time is expected to be relative to the start of the monitor
    pub(crate) fn eval_time_driven_tasks(&mut self, tasks: Vec<EvaluationTask>, ts: Time, tracer: &mut impl Tracer) {
        if tasks.is_empty() {
            return;
        }
        self.new_cycle(ts);
        self.prepare_evaluation(ts);
        for task in tasks {
            match task {
                EvaluationTask::Evaluate(idx, parameter) => {
                    tracer.instance_eval_start(idx, parameter.as_slice());
                    self.eval_stream_instance(idx, parameter.as_slice(), ts);
                    tracer.instance_eval_end(idx, parameter.as_slice());
                },
                EvaluationTask::EvaluateInstances(idx) => {
                    self.eval_stream_instances(idx, ts, tracer);
                },
                EvaluationTask::Spawn(idx) => {
                    tracer.spawn_start(idx);
                    self.eval_spawn(idx, ts);
                    tracer.spawn_end(idx);
                },
                EvaluationTask::Close(idx, parameter) => {
                    tracer.close_start(idx, parameter.as_slice());
                    self.eval_close(idx, parameter.as_slice(), ts);
                    tracer.close_end(idx, parameter.as_slice());
                },
            }
        }
    }

    fn prepare_evaluation(&mut self, ts: Time) {
        // We need to copy the references first because updating needs exclusive access to `self`.
        let windows = &self.ir.sliding_windows;
        for win in windows {
            let WindowParameterization { kind, .. } = self.window_parameterization(win.reference);
            match kind {
                WindowParameterizationKind::None => {
                    let window = self.global_store.get_window_mut(win.reference);
                    if window.is_active() {
                        window.update(ts);
                    }
                },
                WindowParameterizationKind::Caller | WindowParameterizationKind::Target => {
                    self.global_store
                        .get_window_collection_mut(win.reference)
                        .update_all(ts);
                },
                WindowParameterizationKind::Both => {
                    self.global_store
                        .get_two_layer_window_collection_mut(win.reference)
                        .update_all(ts);
                },
            }
        }
    }

    fn eval_stream_instance(&mut self, output: OutputReference, parameter: &[Value], ts: Time) {
        let ix = output;

        let expr = self.compiled_stream_exprs[ix].clone();
        let ctx = self.as_EvaluationContext(parameter.to_vec(), ts);
        let res = expr.execute(&ctx);

        // Filter evaluated to false
        if let Value::None = res {
            return;
        }

        let is_parameterized = self.ir.outputs[ix].is_parameterized();
        // Register value in global store.
        let instance = if is_parameterized {
            self.global_store
                .get_out_instance_collection_mut(output)
                .instance_mut(parameter)
                .expect("tried to eval non existing instance")
        } else {
            self.global_store.get_out_instance_mut(output)
        };
        let old_value = instance.get_value(0);
        instance.push_value(res.clone());
        self.fresh_outputs.insert(ix);

        if let Some(trigger) = self.is_trigger(output) {
            self.fresh_triggers.insert(trigger.trigger_reference);
        }

        // Update Instance aggregations
        if let Some(aggrs) = self.stream_instance_aggregations.get(&StreamReference::Out(output)) {
            aggrs.iter().for_each(|w| {
                let aggr = self.global_store.get_instance_aggregation_mut(*w);
                if let Some(old) = old_value.clone() {
                    aggr.remove_value(old);
                }
                aggr.accept_value(res.clone());
            });
        }

        // Check linked windows and inform them.
        let extended = &self.ir.outputs[ix];
        for (_sr, win) in &extended.aggregated_by {
            self.extend_window(parameter, *win, res.clone(), ts);
        }
    }

    fn window_parameterization(&self, win: WindowReference) -> WindowParameterization {
        self.global_store.window_parameterization(win)
    }

    fn extend_window(&mut self, own_parameter: &[Value], win: WindowReference, value: Value, ts: Time) {
        match self.window_parameterization(win).kind {
            WindowParameterizationKind::None => self.global_store.get_window_mut(win).accept_value(value, ts),
            WindowParameterizationKind::Caller => {
                self.global_store
                    .get_window_collection_mut(win)
                    .accept_value_all(value, ts)
            },
            WindowParameterizationKind::Target => {
                self.global_store
                    .get_window_collection_mut(win)
                    .window_mut(own_parameter)
                    .expect("tried to extend non existing window")
                    .accept_value(value, ts)
            },
            WindowParameterizationKind::Both => {
                self.global_store
                    .get_two_layer_window_collection_mut(win)
                    .accept_value(own_parameter, value, ts)
            },
        }
    }

    /// Marks a new evaluation cycle
    fn new_cycle(&mut self, ts: Time) {
        self.close_streams();
        self.fresh_inputs.clear();
        self.fresh_outputs.clear();
        self.fresh_triggers.clear();

        self.spawned_outputs.clear();
        self.closed_outputs.clear();
        self.global_store.new_cycle(ts);
    }

    fn is_trigger(&self, ix: OutputReference) -> Option<&Trigger> {
        self.triggers[ix].as_ref()
    }

    fn peek_value(&self, sr: StreamReference, args: &[Value], offset: i16) -> Option<Value> {
        match sr {
            StreamReference::In(ix) => {
                assert!(args.is_empty());
                self.global_store.get_in_instance(ix).get_value(offset)
            },
            StreamReference::Out(ix) => {
                if self.ir.stream(sr).is_parameterized() {
                    assert!(!args.is_empty());
                    self.global_store
                        .get_out_instance_collection(ix)
                        .instance(args)
                        .and_then(|i| i.get_value(offset))
                } else {
                    self.global_store.get_out_instance(ix).get_value(offset)
                }
            },
        }
    }

    #[allow(non_snake_case)]
    fn as_EvaluationContext(&mut self, parameter: Vec<Value>, ts: Time) -> EvaluationContext {
        EvaluationContext {
            ts,
            global_store: self.global_store,
            fresh_inputs: self.fresh_inputs,
            fresh_outputs: self.fresh_outputs,
            parameter,
        }
    }
}

impl EvaluationContext<'_> {
    pub(crate) fn lookup_latest(&self, stream_ref: StreamReference, parameter: &[Value]) -> Value {
        match stream_ref {
            StreamReference::In(ix) => {
                self.global_store
                    .get_in_instance(ix)
                    .get_value(0)
                    .unwrap_or(Value::None)
            },
            StreamReference::Out(ix) => {
                if parameter.is_empty() {
                    self.global_store
                        .get_out_instance(ix)
                        .get_value(0)
                        .unwrap_or(Value::None)
                } else {
                    self.global_store
                        .get_out_instance_collection(ix)
                        .instance(parameter)
                        .and_then(|i| i.get_value(0))
                        .unwrap_or(Value::None)
                }
            },
        }
    }

    pub(crate) fn lookup_latest_check(&self, stream_ref: StreamReference, parameter: &[Value]) -> Value {
        let inst = match stream_ref {
            StreamReference::In(ix) => {
                debug_assert!(self.fresh_inputs.contains(ix), "ix={}", ix);
                self.global_store.get_in_instance(ix)
            },
            StreamReference::Out(ix) => {
                debug_assert!(self.fresh_outputs.contains(ix), "ix={}", ix);
                if parameter.is_empty() {
                    self.global_store.get_out_instance(ix)
                } else {
                    self.global_store
                        .get_out_instance_collection(ix)
                        .instance(parameter)
                        .expect("tried to sync access non existing instance")
                }
            },
        };
        inst.get_value(0).unwrap_or(Value::None)
    }

    fn get_instance_and_fresh(
        &self,
        stream_ref: StreamReference,
        parameter: &[Value],
    ) -> (Option<&InstanceStore>, bool) {
        match stream_ref {
            StreamReference::In(ix) => {
                (
                    Some(self.global_store.get_in_instance(ix)),
                    self.fresh_inputs.contains(ix),
                )
            },
            StreamReference::Out(ix) => {
                if parameter.is_empty() {
                    (
                        Some(self.global_store.get_out_instance(ix)),
                        self.fresh_outputs.contains(ix),
                    )
                } else {
                    let collection = self.global_store.get_out_instance_collection(ix);
                    (collection.instance(parameter), collection.is_fresh(parameter))
                }
            },
        }
    }

    pub(crate) fn lookup_fresh(&self, stream_ref: StreamReference, parameter: &[Value]) -> Value {
        let (_, fresh) = self.get_instance_and_fresh(stream_ref, parameter);
        Value::Bool(fresh)
    }

    pub(crate) fn lookup_with_offset(&self, stream_ref: StreamReference, parameter: &[Value], offset: i16) -> Value {
        let (inst, fresh) = self.get_instance_and_fresh(stream_ref, parameter);
        let inst = inst.expect("target stream instance to exist for sync access");
        if fresh {
            inst.get_value(offset).unwrap_or(Value::None)
        } else {
            inst.get_value(offset + 1).unwrap_or(Value::None)
        }
    }

    pub(crate) fn lookup_current(&self, stream_ref: StreamReference, parameter: &[Value]) -> Value {
        let (inst, fresh) = self.get_instance_and_fresh(stream_ref, parameter);
        if fresh {
            inst.expect("fresh instance to exist")
                .get_value(0)
                .expect("fresh stream to have a value.")
        } else {
            Value::None
        }
    }

    pub(crate) fn lookup_instance_aggr(&self, window_reference: WindowReference) -> Value {
        self.global_store.eval_instance_aggregation(window_reference)
    }

    pub(crate) fn lookup_window(&self, window_ref: WindowReference, target_parameter: &[Value]) -> Value {
        let parameterization = self.global_store.window_parameterization(window_ref).kind;
        match parameterization {
            WindowParameterizationKind::None => self.global_store.get_window(window_ref).get_value(self.ts),
            WindowParameterizationKind::Caller => {
                self.global_store
                    .get_window_collection(window_ref)
                    .window(self.parameter.as_slice())
                    .expect("Own window to exist")
                    .get_value(self.ts)
            },
            WindowParameterizationKind::Target => {
                let window_collection = self.global_store.get_window_collection(window_ref);
                let window = window_collection.window(target_parameter);
                if let Some(w) = window {
                    w.get_value(self.ts)
                } else {
                    window_collection.default_value(self.ts)
                }
            },
            WindowParameterizationKind::Both => {
                let collection = self.global_store.get_two_layer_window_collection(window_ref);
                let window = collection.window(target_parameter, self.parameter.as_slice());
                if let Some(w) = window {
                    w.get_value(self.ts)
                } else {
                    collection.default_value(self.ts)
                }
            },
        }
    }

    pub(crate) fn is_active(&self, ac: &ActivationConditionOp) -> bool {
        ac.eval(self.fresh_inputs)
    }
}

impl ActivationConditionOp {
    fn new(ac: &Activation, n_inputs: usize) -> Self {
        use ActivationConditionOp::*;
        if let Activation::True = ac {
            // special case for constant output streams
            return True;
        }
        if let Activation::Conjunction(vec) = ac {
            assert!(!vec.is_empty());
            let ixs: Vec<usize> = vec
                .iter()
                .flat_map(|ac| {
                    if let Activation::Stream(var) = ac {
                        Some(var.in_ix())
                    } else {
                        None
                    }
                })
                .collect();
            if vec.len() == ixs.len() {
                // fast path for conjunctive activation conditions
                let mut bs = BitSet::with_capacity(n_inputs);
                for ix in ixs {
                    bs.insert(ix);
                }
                return Conjunction(bs);
            }
        }
        General(ac.clone())
    }

    pub(crate) fn eval(&self, inputs: &BitSet) -> bool {
        use ActivationConditionOp::*;
        match self {
            True => true,
            Conjunction(bs) => bs.is_subset(inputs),
            General(ac) => Self::eval_(ac, inputs),
            TimeDriven => unreachable!(),
        }
    }

    fn eval_(ac: &Activation, inputs: &BitSet) -> bool {
        use Activation::*;
        match ac {
            Stream(var) => inputs.contains(var.in_ix()),
            Conjunction(vec) => vec.iter().all(|ac| Self::eval_(ac, inputs)),
            Disjunction(vec) => vec.iter().any(|ac| Self::eval_(ac, inputs)),
            True => unreachable!(),
        }
    }

    fn is_eventdriven(&self) -> bool {
        !matches!(self, ActivationConditionOp::TimeDriven)
    }
}

#[cfg(test)]
mod tests {

    use std::time::Duration;

    use ordered_float::{Float, NotNan};
    use rtlola_frontend::ParserConfig;

    use super::*;
    use crate::monitor::NoTracer;
    use crate::schedule::dynamic_schedule::*;
    use crate::storage::Value::*;

    fn setup(spec: &str) -> (RtLolaMir, EvaluatorData, Duration) {
        let cfg = ParserConfig::for_string(spec.to_string());
        let handler = rtlola_frontend::Handler::from(&cfg);
        let ir = rtlola_frontend::parse(&cfg).unwrap_or_else(|e| {
            handler.emit_error(&e);
            panic!();
        });
        let dyn_schedule = Rc::new(RefCell::new(DynamicSchedule::new()));
        let now = Duration::ZERO;
        let eval = EvaluatorData::new(ir.clone(), dyn_schedule);
        (ir, eval, now)
    }

    fn setup_time(spec: &str) -> (RtLolaMir, EvaluatorData, Time) {
        let (ir, eval, _) = setup(spec);
        (ir, eval, Time::default())
    }

    macro_rules! assert_float_eq {
        ($left:expr, $right:expr) => {
            if let Float(left) = $left {
                if let Float(right) = $right {
                    assert!(
                        (left - right).abs() < f64::epsilon(),
                        "Assertion failed: Difference between {} and {} is greater than {}",
                        left,
                        right,
                        f64::epsilon()
                    );
                } else {
                    panic!("{:?} is not a float.", $right)
                }
            } else {
                panic!("{:?} is not a float.", $left)
            }
        };
    }

    macro_rules! eval_stream_instances {
        ($eval:expr, $start:expr, $ix:expr) => {
            $eval.eval_event_driven_output($ix.out_ix(), $start, &mut NoTracer::default());
        };
    }

    macro_rules! eval_stream_instances_timed {
        ($eval:expr, $time:expr, $ix:expr) => {
            $eval.prepare_evaluation($time);
            $eval.eval_event_driven_output($ix.out_ix(), $time, &mut NoTracer::default());
        };
    }

    macro_rules! eval_stream {
        ($eval:expr, $start:expr, $ix:expr, $parameter:expr) => {
            $eval.eval_stream_instance($ix, $parameter.as_slice(), $start);
        };
    }

    macro_rules! spawn_stream {
        ($eval:expr, $start:expr, $ix:expr) => {
            $eval.eval_event_driven_spawn($ix.out_ix(), $start, &mut NoTracer::default());
        };
    }

    macro_rules! spawn_stream_timed {
        ($eval:expr, $time:expr, $ix:expr) => {
            $eval.eval_event_driven_spawn($ix.out_ix(), $time, &mut NoTracer::default());
        };
    }

    macro_rules! eval_close {
        ($eval:expr, $start:expr, $ix:expr, $parameter:expr) => {
            $eval.eval_close($ix.out_ix(), $parameter.as_slice(), $start);
            $eval.close_streams();
        };
    }

    macro_rules! eval_close_timed {
        ($eval:expr, $time:expr, $ix:expr, $parameter:expr) => {
            $eval.eval_close($ix.out_ix(), $parameter.as_slice(), $time);
            $eval.close_streams();
        };
    }

    macro_rules! stream_has_instance {
        ($eval:expr, $ix:expr, $parameter:expr) => {
            if $parameter.is_empty() {
                $eval.global_store.get_out_instance($ix.out_ix()).is_active()
            } else {
                $eval
                    .global_store
                    .get_out_instance_collection($ix.out_ix())
                    .contains($parameter.as_slice())
            }
        };
    }

    macro_rules! eval_stream_timed {
        ($eval:expr, $ix:expr, $parameter:expr, $time:expr) => {
            $eval.prepare_evaluation($time);
            $eval.eval_stream_instance($ix, $parameter.as_slice(), $time);
        };
    }

    macro_rules! accept_input {
        ($eval:expr, $start:expr, $str_ref:expr, $v:expr) => {
            $eval.accept_input($str_ref.in_ix(), $v.clone(), $start);
        };
    }

    macro_rules! accept_input_timed {
        ($eval:expr, $str_ref:expr, $v:expr, $time:expr) => {
            $eval.accept_input($str_ref.in_ix(), $v.clone(), $time);
        };
    }

    macro_rules! peek_assert_eq {
        ($eval:expr, $start:expr, $ix:expr, $parameter:expr, $value:expr) => {
            eval_stream!($eval, $start, $ix, $parameter);
            assert_eq!(
                $eval
                    .peek_value(StreamReference::Out($ix), $parameter.as_slice(), 0)
                    .unwrap(),
                $value
            );
        };
    }

    #[test]
    fn test_const_output_literals() {
        let (_, eval, start) = setup(
            r#"
        input i_0: UInt8

        output o_0: Bool @i_0 := true
        output o_1: UInt8 @i_0 := 3
        output o_2: Int8 @i_0 := -5
        output o_3: Float32 @i_0 := -123.456
        output o_4: String @i_0 := "foobar"
        "#,
        );
        let mut eval = eval.into_evaluator();
        let sr = StreamReference::In(0);
        let v = Unsigned(3);
        accept_input!(eval, start, sr, v);
        peek_assert_eq!(eval, start, 0, vec![], Bool(true));
        peek_assert_eq!(eval, start, 1, vec![], Unsigned(3));
        peek_assert_eq!(eval, start, 2, vec![], Signed(-5));
        peek_assert_eq!(eval, start, 3, vec![], Value::try_from(-123.456).unwrap());
        peek_assert_eq!(eval, start, 4, vec![], Str("foobar".into()));
    }

    #[test]
    fn test_const_output_arithlog() {
        let (_, eval, start) = setup(
            r#"
        input i_0: Int8

        output o_0:   Bool @i_0 := !false
        output o_1:   Bool @i_0 := !true
        output o_2:  UInt8 @i_0 := 8 + 3
        output o_3:  UInt8 @i_0 := 8 - 3
        output o_4:  UInt8 @i_0 := 8 * 3
        output o_5:  UInt8 @i_0 := 8 / 3
        output o_6:  UInt8 @i_0 := 8 % 3
        output o_7:  UInt8 @i_0 := 8 ** 3
        output o_8:   Bool @i_0 := false || false
        output o_9:   Bool @i_0 := false || true
        output o_10:  Bool @i_0 := true  || false
        output o_11:  Bool @i_0 := true  || true
        output o_12:  Bool @i_0 := false && false
        output o_13:  Bool @i_0 := false && true
        output o_14:  Bool @i_0 := true  && false
        output o_15:  Bool @i_0 := true  && true
        output o_16:  Bool @i_0 := 0 < 1
        output o_17:  Bool @i_0 := 0 < 0
        output o_18:  Bool @i_0 := 1 < 0
        output o_19:  Bool @i_0 := 0 <= 1
        output o_20:  Bool @i_0 := 0 <= 0
        output o_21:  Bool @i_0 := 1 <= 0
        output o_22:  Bool @i_0 := 0 >= 1
        output o_23:  Bool @i_0 := 0 >= 0
        output o_24:  Bool @i_0 := 1 >= 0
        output o_25:  Bool @i_0 := 0 > 1
        output o_26:  Bool @i_0 := 0 > 0
        output o_27:  Bool @i_0 := 1 > 0
        output o_28:  Bool @i_0 := 0 == 0
        output o_29:  Bool @i_0 := 0 == 1
        output o_30:  Bool @i_0 := 0 != 0
        output o_31:  Bool @i_0 := 0 != 1
        "#,
        );
        let mut eval = eval.into_evaluator();
        let sr = StreamReference::In(0);
        let v = Unsigned(3);
        accept_input!(eval, start, sr, v);
        peek_assert_eq!(eval, start, 0, vec![], Bool(!false));
        peek_assert_eq!(eval, start, 1, vec![], Bool(!true));
        peek_assert_eq!(eval, start, 2, vec![], Unsigned(8 + 3));
        peek_assert_eq!(eval, start, 3, vec![], Unsigned(8 - 3));
        peek_assert_eq!(eval, start, 4, vec![], Unsigned(8 * 3));
        peek_assert_eq!(eval, start, 5, vec![], Unsigned(8 / 3));
        peek_assert_eq!(eval, start, 6, vec![], Unsigned(8 % 3));
        peek_assert_eq!(eval, start, 7, vec![], Unsigned(8 * 8 * 8));
        peek_assert_eq!(eval, start, 8, vec![], Bool(false || false));
        peek_assert_eq!(eval, start, 9, vec![], Bool(false || true));
        peek_assert_eq!(eval, start, 10, vec![], Bool(true || false));
        peek_assert_eq!(eval, start, 11, vec![], Bool(true || true));
        peek_assert_eq!(eval, start, 12, vec![], Bool(false && false));
        peek_assert_eq!(eval, start, 13, vec![], Bool(false && true));
        peek_assert_eq!(eval, start, 14, vec![], Bool(true && false));
        peek_assert_eq!(eval, start, 15, vec![], Bool(true && true));
        peek_assert_eq!(eval, start, 16, vec![], Bool(0 < 1));
        peek_assert_eq!(eval, start, 17, vec![], Bool(0 < 0));
        peek_assert_eq!(eval, start, 18, vec![], Bool(1 < 0));
        peek_assert_eq!(eval, start, 19, vec![], Bool(0 <= 1));
        peek_assert_eq!(eval, start, 20, vec![], Bool(0 <= 0));
        peek_assert_eq!(eval, start, 21, vec![], Bool(1 <= 0));
        peek_assert_eq!(eval, start, 22, vec![], Bool(0 >= 1));
        peek_assert_eq!(eval, start, 23, vec![], Bool(0 >= 0));
        peek_assert_eq!(eval, start, 24, vec![], Bool(1 >= 0));
        peek_assert_eq!(eval, start, 25, vec![], Bool(0 > 1));
        peek_assert_eq!(eval, start, 26, vec![], Bool(0 > 0));
        peek_assert_eq!(eval, start, 27, vec![], Bool(1 > 0));
        peek_assert_eq!(eval, start, 28, vec![], Bool(0 == 0));
        peek_assert_eq!(eval, start, 29, vec![], Bool(0 == 1));
        peek_assert_eq!(eval, start, 30, vec![], Bool(0 != 0));
        peek_assert_eq!(eval, start, 31, vec![], Bool(0 != 1));
    }

    #[test]
    fn test_input_only() {
        let (_, eval, start) = setup("input a: UInt8");
        let mut eval = eval.into_evaluator();
        let sr = StreamReference::In(0);
        let v = Unsigned(3);
        accept_input!(eval, start, sr, v);
        assert_eq!(eval.peek_value(sr, &Vec::new(), 0).unwrap(), v)
    }

    #[test]
    fn test_sync_lookup() {
        let (_, eval, start) = setup("input a: UInt8 output b: UInt8 := a output c: UInt8 := b");
        let mut eval = eval.into_evaluator();
        let out_ref_0 = StreamReference::Out(0);
        let out_ref_1 = StreamReference::Out(1);
        let in_ref = StreamReference::In(0);
        let v = Unsigned(9);
        accept_input!(eval, start, in_ref, v);
        eval_stream!(eval, start, 0, vec![]);
        eval_stream!(eval, start, 1, vec![]);
        assert_eq!(eval.peek_value(out_ref_0, &Vec::new(), 0).unwrap(), v);
        assert_eq!(eval.peek_value(out_ref_1, &Vec::new(), 0).unwrap(), v)
    }

    #[test]
    fn test_oob_lookup() {
        let (_, eval, start) =
            setup("input a: UInt8\noutput b := a.offset(by: -1).defaults(to: 3)\noutput x: UInt8 @5Hz := b.hold().defaults(to: 3)");
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(1);
        let in_ref = StreamReference::In(0);
        let v1 = Unsigned(1);
        accept_input!(eval, start, in_ref, v1);
        eval_stream!(eval, start, 0, vec![]);
        eval_stream!(eval, start, 1, vec![]);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), Unsigned(3));
    }

    #[test]
    fn test_output_lookup() {
        let (_, eval, start) = setup(
            "input a: UInt8\n\
                    output mirror: UInt8 := a\n\
                    output mirror_offset := mirror.offset(by: -1).defaults(to: 5)\n\
                    output c: UInt8 @5Hz := mirror.hold().defaults(to: 8)\n\
                    output d: UInt8 @5Hz := mirror_offset.hold().defaults(to: 3)",
        );
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(2);
        let in_ref = StreamReference::In(0);
        let v1 = Unsigned(1);
        let v2 = Unsigned(2);
        accept_input!(eval, start, in_ref, v1);
        eval_stream!(eval, start, 0, vec![]);
        eval_stream!(eval, start, 1, vec![]);
        accept_input!(eval, start, in_ref, v2);
        eval_stream!(eval, start, 0, vec![]);
        eval_stream!(eval, start, 1, vec![]);
        eval_stream!(eval, start, 2, vec![]);
        assert_eq!(eval.peek_value(StreamReference::Out(0), &Vec::new(), 0).unwrap(), v2);
        assert_eq!(eval.peek_value(StreamReference::Out(1), &Vec::new(), 0).unwrap(), v1);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), v2);
    }

    #[test]
    fn test_get_fresh_lookup() {
        let (_, eval, start) = setup(
            "input a: UInt8\n\
                    input b: UInt8\n\
                    output mirror: UInt8 := a\n\
                    output mirror_parameter (p)\n
                        spawn with a\n\
                        eval with a
                    output g1: UInt8 @a := mirror.get().defaults(to: 8)\n\
                    output g2: UInt8 @a := mirror_parameter(a).get().defaults(to: 8)\n\
                    output f1: Bool @a := mirror.is_fresh()\n\
                    output f2: Bool @a := mirror_parameter(a).is_fresh()",
        );
        let mut eval = eval.into_evaluator();
        let a = StreamReference::In(0);
        let mirror_p = StreamReference::Out(1);
        let g1 = StreamReference::Out(2);
        let g2 = StreamReference::Out(3);
        let f1 = StreamReference::Out(4);
        let f2 = StreamReference::Out(5);

        let v1 = Unsigned(1);
        let t = Bool(true);

        accept_input!(eval, start, a, v1);
        eval_stream!(eval, start, 0, vec![]);
        spawn_stream!(eval, start, mirror_p);
        eval_stream_instances!(eval, start, mirror_p);

        eval_stream!(eval, start, 2, vec![]);
        eval_stream!(eval, start, 3, vec![]);
        eval_stream!(eval, start, 4, vec![]);
        eval_stream!(eval, start, 5, vec![]);

        assert_eq!(eval.peek_value(g1, &Vec::new(), 0).unwrap(), v1);
        assert_eq!(eval.peek_value(g2, &Vec::new(), 0).unwrap(), v1);

        assert_eq!(eval.peek_value(f1, &Vec::new(), 0).unwrap(), t);
        assert_eq!(eval.peek_value(f2, &Vec::new(), 0).unwrap(), t);
    }

    #[test]
    fn test_get_fresh_lookup_fail() {
        let (_, eval, start) = setup(
            "input a: UInt8\n\
                    input b: UInt8\n\
                    output mirror: UInt8 := a\n\
                    output mirror_parameter (p)\n
                        spawn with a\n\
                        eval with a
                    output g1: UInt8 @b := mirror.get().defaults(to: 8)\n\
                    output g2: UInt8 @b := mirror_parameter(b).get().defaults(to: 8)\n\
                    output f1: Bool @b := mirror.is_fresh()\n\
                    output f2: Bool @b := mirror_parameter(b).is_fresh()",
        );
        let mut eval = eval.into_evaluator();
        let b = StreamReference::In(1);
        let g1 = StreamReference::Out(2);
        let g2 = StreamReference::Out(3);
        let f1 = StreamReference::Out(4);
        let f2 = StreamReference::Out(5);

        let v2 = Unsigned(2);
        let d = Unsigned(8);
        let f = Bool(false);

        accept_input!(eval, start, b, v2);

        eval_stream!(eval, start, 2, vec![]);
        eval_stream!(eval, start, 3, vec![]);
        eval_stream!(eval, start, 4, vec![]);
        eval_stream!(eval, start, 5, vec![]);

        assert_eq!(eval.peek_value(g1, &Vec::new(), 0).unwrap(), d);
        assert_eq!(eval.peek_value(g2, &Vec::new(), 0).unwrap(), d);

        assert_eq!(eval.peek_value(f1, &Vec::new(), 0).unwrap(), f);
        assert_eq!(eval.peek_value(f2, &Vec::new(), 0).unwrap(), f);
    }

    #[test]
    fn test_conversion_if() {
        let (_, eval, start) =
            setup("input a: UInt8\noutput b: UInt16 := widen<UInt16>(if true then a else a[-1].defaults(to: 0))");
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let in_ref = StreamReference::In(0);
        let v1 = Unsigned(1);
        accept_input!(eval, start, in_ref, v1);
        eval_stream!(eval, start, 0, vec![]);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), v1);
    }

    #[test]
    #[ignore] // See issue #32 in LolaParser.
    fn test_conversion_lookup() {
        let (_, eval, start) = setup("input a: UInt8\noutput b: UInt32 := a + 100000");
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let in_ref = StreamReference::In(0);
        let expected = Unsigned(7 + 100000);
        let v1 = Unsigned(7);
        accept_input!(eval, start, in_ref, v1);
        eval_stream!(eval, start, 0, vec![]);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_bin_op() {
        let (_, eval, start) = setup("input a: UInt16\n input b: UInt16\noutput c: UInt16 := a + b");
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let a = StreamReference::In(0);
        let b = StreamReference::In(1);
        let v1 = Unsigned(1);
        let v2 = Unsigned(2);
        let expected = Unsigned(1 + 2);
        accept_input!(eval, start, a, v1);
        accept_input!(eval, start, b, v2);
        eval_stream!(eval, start, 0, vec![]);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_bin_op_float() {
        let (_, eval, start) = setup("input a: Float64\n input b: Float64\noutput c: Float64 := a + b");
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let a = StreamReference::In(0);
        let b = StreamReference::In(1);
        let v1 = Float(NotNan::new(3.5f64).unwrap());
        let v2 = Float(NotNan::new(39.347568f64).unwrap());
        let expected = Float(NotNan::new(3.5f64 + 39.347568f64).unwrap());
        accept_input!(eval, start, a, v1);
        accept_input!(eval, start, b, v2);
        eval_stream!(eval, start, 0, vec![]);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_bin_tuple() {
        let (_, eval, start) =
            setup("input a: Int32\n input b: Bool\noutput c := (a, b) output d := c.0 output e := c.1");
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let out_ref0 = StreamReference::Out(1);
        let out_ref1 = StreamReference::Out(2);
        let a = StreamReference::In(0);
        let b = StreamReference::In(1);
        let v1 = Signed(1);
        let v2 = Bool(true);
        let expected = Tuple(Box::new([v1.clone(), v2.clone()]));
        accept_input!(eval, start, a, v1);
        accept_input!(eval, start, b, v2);
        eval_stream!(eval, start, 0, vec![]);
        eval_stream!(eval, start, 1, vec![]);
        eval_stream!(eval, start, 2, vec![]);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
        assert_eq!(eval.peek_value(out_ref0, &Vec::new(), 0).unwrap(), v1);
        assert_eq!(eval.peek_value(out_ref1, &Vec::new(), 0).unwrap(), v2);
    }

    #[test]
    fn test_regular_lookup() {
        let (_, eval, start) =
            setup("input a: UInt8 output b := a.offset(by: -1).defaults(to: 5) output x: UInt8 @5Hz := b.hold().defaults(to: 3)");
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(1);
        let in_ref = StreamReference::In(0);
        let v1 = Unsigned(1);
        let v2 = Unsigned(2);
        let v3 = Unsigned(3);
        accept_input!(eval, start, in_ref, v1);
        accept_input!(eval, start, in_ref, v2);
        accept_input!(eval, start, in_ref, v3);
        eval_stream!(eval, start, 0, vec![]);
        eval_stream!(eval, start, 1, vec![]);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), v2)
    }

    #[ignore] // triggers no longer store values
    #[test]
    fn test_trigger() {
        let (_, eval, start) =
            setup("input a: UInt8 output b := a.offset(by: -1) output x: UInt8 @5Hz := b.hold().defaults(to: 3)\n trigger x > 4");
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(1);
        let trig_ref = StreamReference::Out(2);
        let in_ref = StreamReference::In(0);
        let v1 = Unsigned(8);
        eval_stream!(eval, start, 0, vec![]);
        eval_stream!(eval, start, 1, vec![]);
        eval_stream!(eval, start, 2, vec![]);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), Unsigned(3));
        assert_eq!(eval.peek_value(trig_ref, &Vec::new(), 0).unwrap(), Bool(false));
        accept_input!(eval, start, in_ref, v1);
        eval_stream!(eval, start, 0, vec![]);
        eval_stream!(eval, start, 1, vec![]);
        eval_stream!(eval, start, 2, vec![]);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), Unsigned(3));
        assert_eq!(eval.peek_value(trig_ref, &Vec::new(), 0).unwrap(), Bool(false));
        accept_input!(eval, start, in_ref, Unsigned(17));
        eval_stream!(eval, start, 0, vec![]);
        eval_stream!(eval, start, 1, vec![]);
        eval_stream!(eval, start, 2, vec![]);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), v1);
        assert_eq!(eval.peek_value(trig_ref, &Vec::new(), 0).unwrap(), Bool(true));
    }

    #[test]
    fn test_sum_window() {
        let (_, eval, mut time) =
            setup_time("input a: Int16\noutput b: Int16 @0.25Hz := a.aggregate(over: 40s, using: sum)");
        let mut eval = eval.into_evaluator();
        time += Duration::from_secs(45);
        let out_ref = StreamReference::Out(0);
        let in_ref = StreamReference::In(0);
        let n = 25;
        for v in 1..=n {
            accept_input_timed!(eval, in_ref, Signed(v), time);
            time += Duration::from_secs(1);
        }
        time += Duration::from_secs(1);
        // 71 secs have passed. All values should be within the window.
        eval_stream_timed!(eval, 0, vec![], time);
        let expected = Signed((n * n + n) / 2);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_count_window() {
        let (_, eval, mut time) =
            setup_time("input a: UInt16\noutput b: UInt16 @0.25Hz := a.aggregate(over: 40s, using: count)");
        let mut eval = eval.into_evaluator();
        time += Duration::from_secs(45);
        let out_ref = StreamReference::Out(0);
        let in_ref = StreamReference::In(0);
        let n = 25;
        for v in 1..=n {
            accept_input_timed!(eval, in_ref, Unsigned(v), time);
            time += Duration::from_secs(1);
        }
        time += Duration::from_secs(1);
        // 71 secs have passed. All values should be within the window.
        eval_stream_timed!(eval, 0, vec![], time);
        let expected = Unsigned(n);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_average_window() {
        let (_, eval, mut time) = setup_time(
            "input a: Float32\noutput b @0.25Hz := a.aggregate(over: 40s, using: average).defaults(to: -3.0)",
        );
        let mut eval = eval.into_evaluator();
        time += Duration::from_secs(45);
        let out_ref = StreamReference::Out(0);
        let in_ref = StreamReference::In(0);

        // No time has passed. No values should be within the window. We should see the default value.
        eval_stream_timed!(eval, 0, vec![], time);
        let expected = Value::try_from(-3.0).unwrap();
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);

        let n = 25;
        for v in 1..=n {
            accept_input_timed!(eval, in_ref, Value::try_from(v as f64).unwrap(), time);
            time += Duration::from_secs(1);
        }
        time += Duration::from_secs(1);

        // 71 secs have passed. All values should be within the window.
        eval_stream_timed!(eval, 0, vec![], time);
        let n = n as f64;
        let expected = Value::try_from(((n * n + n) / 2.0) / 25.0).unwrap();
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_window_correct_bucketing() {
        let (_, eval, mut time) = setup_time("input a: Float32\noutput b @2Hz := a.aggregate(over: 3s, using: sum)");
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let in_ref = StreamReference::In(0);

        accept_input_timed!(eval, in_ref, Value::try_from(0 as f64).unwrap(), time);

        time += Duration::from_millis(500);
        eval_stream_timed!(eval, 0, vec![], time);
        let expected = Value::try_from(0.0).unwrap();
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);

        time += Duration::from_millis(500);
        eval_stream_timed!(eval, 0, vec![], time);
        let expected = Value::try_from(0.0).unwrap();
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);

        //1s

        time += Duration::from_millis(100);
        accept_input_timed!(eval, in_ref, Value::try_from(1 as f64).unwrap(), time);

        time += Duration::from_millis(400);
        eval_stream_timed!(eval, 0, vec![], time);
        let expected = Value::try_from(1.0).unwrap();
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);

        time += Duration::from_millis(500);
        eval_stream_timed!(eval, 0, vec![], time);
        let expected = Value::try_from(1.0).unwrap();
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);

        //2s

        time += Duration::from_millis(500);
        eval_stream_timed!(eval, 0, vec![], time);
        let expected = Value::try_from(1.0).unwrap();
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);

        time += Duration::from_millis(500);
        eval_stream_timed!(eval, 0, vec![], time);
        let expected = Value::try_from(1.0).unwrap();
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);

        //3s

        time += Duration::from_millis(40);
        accept_input_timed!(eval, in_ref, Value::try_from(2 as f64).unwrap(), time);

        time += Duration::from_millis(460);
        eval_stream_timed!(eval, 0, vec![], time);
        let expected = Value::try_from(3.0).unwrap();
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);

        time += Duration::from_millis(500);
        eval_stream_timed!(eval, 0, vec![], time);
        let expected = Value::try_from(3.0).unwrap();
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);

        //4s

        time += Duration::from_millis(500);
        eval_stream_timed!(eval, 0, vec![], time);
        let expected = Value::try_from(2.0).unwrap();
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);

        time += Duration::from_millis(500);
        eval_stream_timed!(eval, 0, vec![], time);
        let expected = Value::try_from(2.0).unwrap();
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);

        //5s

        time += Duration::from_millis(110);
        accept_input_timed!(eval, in_ref, Value::try_from(3 as f64).unwrap(), time);

        time += Duration::from_millis(390);
        eval_stream_timed!(eval, 0, vec![], time);
        let expected = Value::try_from(5.0).unwrap();
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);

        time += Duration::from_millis(500);
        eval_stream_timed!(eval, 0, vec![], time);
        let expected = Value::try_from(5.0).unwrap();
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_integral_window() {
        let (_, eval, mut time) = setup_time(
            "input a: Float64\noutput b: Float64 @0.25Hz := a.aggregate(over_exactly: 40s, using: integral).defaults(to: -3.0)",
        );
        let mut eval = eval.into_evaluator();
        time += Duration::from_secs(45);
        let out_ref = StreamReference::Out(0);
        let in_ref = StreamReference::In(0);

        fn mv(f: f64) -> Value {
            Float(NotNan::new(f).unwrap())
        }

        accept_input_timed!(eval, in_ref, mv(1f64), time);
        time += Duration::from_secs(2);
        accept_input_timed!(eval, in_ref, mv(5f64), time);
        // Value so far: (1+5) / 2 * 2 = 6
        time += Duration::from_secs(5);
        accept_input_timed!(eval, in_ref, mv(25f64), time);
        // Value so far: 6 + (5+25) / 2 * 5 = 6 + 75 = 81
        time += Duration::from_secs(1);
        accept_input_timed!(eval, in_ref, mv(0f64), time);
        // Value so far: 81 + (25+0) / 2 * 1 = 81 + 12.5 = 93.5
        time += Duration::from_secs(10);
        accept_input_timed!(eval, in_ref, mv(-40f64), time);
        // Value so far: 93.5 + (0+(-40)) / 2 * 10 = 93.5 - 200 = -106.5
        // Time passed: 2 + 5 + 1 + 10 = 18.

        eval_stream_timed!(eval, 0, vec![], time);

        let expected = Float(NotNan::new(-106.5).unwrap());
        assert_eq!(eval.peek_value(out_ref, vec![].as_slice(), 0).unwrap(), expected);
    }

    #[test]
    fn test_integral_window2() {
        fn mv(f: f64) -> Value {
            Float(NotNan::new(f).unwrap())
        }

        let (_, eval, mut time) = setup_time("input a : Int64\noutput b@1Hz := a.aggregate(over: 5s, using: integral)");
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let in_ref = StreamReference::In(0);

        accept_input_timed!(eval, in_ref, mv(0f64), time);

        time += Duration::from_secs(1);
        accept_input_timed!(eval, in_ref, mv(8f64), time);
        eval_stream_timed!(eval, 0, vec![], time);
        let expected = Float(NotNan::new(4.0).unwrap());
        assert_eq!(eval.peek_value(out_ref, vec![].as_slice(), 0).unwrap(), expected);
    }

    #[test]
    fn test_window_type_count() {
        let (_, eval, start) = setup("input a: Int32\noutput b @ 10Hz := a.aggregate(over: 0.1s, using: count)");
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let _a = StreamReference::In(0);
        let expected = Unsigned(0);
        eval_stream!(eval, start, 0, vec![]);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_sum_window_discrete() {
        let (_, eval, mut time) =
            setup_time("input a: Int16\noutput b: Int16 := a.aggregate(over_discrete: 6, using: sum)");
        let mut eval = eval.into_evaluator();
        time += Duration::from_secs(45);
        let out_ref = StreamReference::Out(0);
        let in_ref = StreamReference::In(0);
        let n = 25;
        for v in 1..=n {
            accept_input_timed!(eval, in_ref, Signed(v), time);
            time += Duration::from_secs(1);
        }
        time += Duration::from_secs(1);
        // 71 secs have passed. All values should be within the window.
        eval_stream_timed!(eval, 0, vec![], time);
        let expected = Signed(135);
        //assert_eq!(eval.peek_value(in_ref, &Vec::new(), -1).unwrap(), Signed(24));
        assert_eq!(eval.peek_value(in_ref, &Vec::new(), 0).unwrap(), Signed(25));
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    // New windows tests
    #[test]
    fn test_last_window_float() {
        let (_, eval, mut time) = setup_time(
            "input a: Float32\noutput b: Float32 @1Hz:= a.aggregate(over: 5s, using: last).defaults(to:0.0)",
        );
        let mut eval = eval.into_evaluator();
        time += Duration::from_secs(45);
        let out_ref = StreamReference::Out(0);
        let in_ref = StreamReference::In(0);
        let n = 25;
        for v in 1..=n {
            accept_input_timed!(eval, in_ref, Float(NotNan::new(v as f64).unwrap()), time);
            time += Duration::from_secs(1);
        }
        time += Duration::from_secs(1);
        // 71 secs have passed. All values should be within the window.
        eval_stream_timed!(eval, 0, vec![], time);
        let expected = Float(NotNan::new(25.0).unwrap());
        assert_eq!(
            eval.peek_value(in_ref, &Vec::new(), 0).unwrap(),
            Float(NotNan::new(25.0).unwrap())
        );
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_last_window_signed() {
        let (_, eval, mut time) =
            setup_time("input a: Int32\noutput b: Int32 @1Hz:= a.aggregate(over: 20s, using: last).defaults(to:0)");
        let mut eval = eval.into_evaluator();
        time += Duration::from_secs(45);
        let out_ref = StreamReference::Out(0);
        let in_ref = StreamReference::In(0);
        let n = 25;
        for v in 1..=n {
            accept_input_timed!(eval, in_ref, Signed(v), time);
            time += Duration::from_secs(1);
        }
        time += Duration::from_secs(1);
        // 71 secs have passed. All values should be within the window.
        eval_stream_timed!(eval, 0, vec![], time);
        let expected = Signed(25);
        assert_eq!(eval.peek_value(in_ref, &Vec::new(), 0).unwrap(), Signed(25));
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_last_window_unsigned() {
        let (_, eval, mut time) =
            setup_time("input a: UInt32\noutput b: UInt32 @1Hz:= a.aggregate(over: 20s, using: last).defaults(to:0)");
        let mut eval = eval.into_evaluator();
        time += Duration::from_secs(45);
        let out_ref = StreamReference::Out(0);
        let in_ref = StreamReference::In(0);
        let n = 25;
        for v in 1..=n {
            accept_input_timed!(eval, in_ref, Unsigned(v), time);
            time += Duration::from_secs(1);
        }
        time += Duration::from_secs(1);
        // 71 secs have passed. All values should be within the window.
        eval_stream_timed!(eval, 0, vec![], time);
        let expected = Unsigned(25);
        assert_eq!(eval.peek_value(in_ref, &Vec::new(), 0).unwrap(), Unsigned(25));
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_percentile_float() {
        for (pctl, exp) in &[
            ("pctl25", Value::try_from(13.0)),
            ("pctl75", Value::try_from(18.0)),
            ("pctl10", Value::try_from(11.5)),
            ("pctl5", Value::try_from(11.0)),
            ("pctl90", Value::try_from(19.5)),
            ("med", Value::try_from(15.5)),
        ] {
            let (_, eval, mut time) = setup_time(&format!(
                "input a: Float32\noutput b: Float32 @1Hz:= a.aggregate(over: 10s, using: {}).defaults(to:0.0)",
                pctl
            ));
            let mut eval = eval.into_evaluator();
            time += Duration::from_secs(45);
            let out_ref = StreamReference::Out(0);
            let in_ref = StreamReference::In(0);
            let n = 20;
            for v in 1..=n {
                time += Duration::from_secs(1);
                accept_input_timed!(eval, in_ref, Float(NotNan::new(v as f64).unwrap()), time);
            }
            // 66 secs have passed. All values should be within the window.
            eval_stream_timed!(eval, 0, vec![], time);
            assert_eq!(
                eval.peek_value(in_ref, &Vec::new(), 0).unwrap(),
                Float(NotNan::new(20.0).unwrap())
            );
            assert_eq!(
                eval.peek_value(out_ref, &Vec::new(), 0).unwrap(),
                exp.as_ref().unwrap().clone()
            );
        }
    }

    #[test]
    fn test_percentile_float_unordered_input() {
        for (pctl, exp) in &[
            ("pctl25", Value::try_from(13.0)),
            ("pctl75", Value::try_from(18.0)),
            ("pctl10", Value::try_from(11.5)),
            ("pctl5", Value::try_from(11.0)),
            ("pctl90", Value::try_from(19.5)),
            ("med", Value::try_from(15.5)),
        ] {
            let (_, eval, mut time) = setup_time(&format!(
                "input a: Float32\noutput b: Float32 @1Hz:= a.aggregate(over: 10s, using: {}).defaults(to:0.0)",
                pctl
            ));
            let mut eval = eval.into_evaluator();
            time += Duration::from_secs(45);
            let out_ref = StreamReference::Out(0);
            let in_ref = StreamReference::In(0);
            let n = 20;
            let input_val = [1, 9, 8, 5, 4, 3, 7, 2, 10, 6, 20, 11, 19, 12, 18, 13, 17, 14, 16, 15];
            for v in 0..n {
                time += Duration::from_secs(1);
                accept_input_timed!(eval, in_ref, Float(NotNan::new(input_val[v] as f64).unwrap()), time);
            }
            // 66 secs have passed. All values should be within the window.
            eval_stream_timed!(eval, 0, vec![], time);
            assert_eq!(
                eval.peek_value(in_ref, &Vec::new(), 0).unwrap(),
                Float(NotNan::new(15.0).unwrap())
            );
            assert_eq!(
                eval.peek_value(out_ref, &Vec::new(), 0).unwrap(),
                exp.as_ref().unwrap().clone()
            );
        }
    }

    #[test]
    fn test_percentile_signed() {
        for (pctl, exp) in &[
            ("pctl25", Signed(13)),
            ("pctl75", Signed(18)),
            ("pctl10", Signed(11)),
            ("pctl5", Signed(11)),
            ("pctl90", Signed(19)),
            ("med", Signed(15)),
        ] {
            let (_, eval, mut time) = setup_time(&format!(
                "input a: Int32\noutput b: Int32 @1Hz:= a.aggregate(over: 10s, using: {}).defaults(to:0)",
                pctl
            ));
            let mut eval = eval.into_evaluator();
            time += Duration::from_secs(45);
            let out_ref = StreamReference::Out(0);
            let in_ref = StreamReference::In(0);
            let n = 20;
            for v in 1..=n {
                time += Duration::from_secs(1);
                accept_input_timed!(eval, in_ref, Signed(v), time);
            }
            // 66 secs have passed. All values should be within the window.
            eval_stream_timed!(eval, 0, vec![], time);
            assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), exp.clone());
        }
    }

    #[test]
    fn test_percentile_unsigned() {
        for (pctl, exp) in &[
            ("pctl25", Unsigned(13)),
            ("pctl75", Unsigned(18)),
            ("pctl10", Unsigned(11)),
            ("pctl5", Unsigned(11)),
            ("pctl90", Unsigned(19)),
            ("med", Unsigned(15)),
        ] {
            let (_, eval, mut time) = setup_time(&format!(
                "input a: UInt32\noutput b: UInt32 @1Hz:= a.aggregate(over: 10s, using: {}).defaults(to:0)",
                pctl
            ));
            let mut eval = eval.into_evaluator();
            time += Duration::from_secs(45);
            let out_ref = StreamReference::Out(0);
            let in_ref = StreamReference::In(0);
            let n = 20;
            for v in 1..=n {
                time += Duration::from_secs(1);
                accept_input_timed!(eval, in_ref, Unsigned(v), time);
            }
            // 66 secs have passed. All values should be within the window.
            eval_stream_timed!(eval, 0, vec![], time);
            assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), exp.clone());
        }
    }

    #[test]
    fn test_percentile_discrete_float_unordered_input() {
        for (pctl, exp) in &[
            ("pctl25", Value::try_from(13.0)),
            ("pctl75", Value::try_from(18.0)),
            ("pctl10", Value::try_from(11.5)),
            ("pctl5", Value::try_from(11.0)),
            ("pctl90", Value::try_from(19.5)),
            ("med", Value::try_from(15.5)),
        ] {
            let (_, eval, mut time) = setup_time(&format!(
                "input a: Float32\noutput b: Float32 := a.aggregate(over_discrete: 10, using: {}).defaults(to:0.0)",
                pctl
            ));
            let mut eval = eval.into_evaluator();
            time += Duration::from_secs(45);
            let out_ref = StreamReference::Out(0);
            let in_ref = StreamReference::In(0);
            let n = 20;
            let input_val = [1, 9, 8, 5, 4, 3, 7, 2, 10, 6, 20, 11, 19, 12, 18, 13, 17, 14, 16, 15];
            for v in 0..n {
                time += Duration::from_secs(1);
                accept_input_timed!(eval, in_ref, Float(NotNan::new(input_val[v] as f64).unwrap()), time);
            }
            // 66 secs have passed. All values should be within the window.
            eval_stream_timed!(eval, 0, vec![], time);
            assert_eq!(
                eval.peek_value(in_ref, &Vec::new(), 0).unwrap(),
                Float(NotNan::new(15.0).unwrap())
            );
            assert_eq!(
                eval.peek_value(out_ref, &Vec::new(), 0).unwrap(),
                exp.as_ref().unwrap().clone()
            );
        }
    }

    #[test]
    fn test_var_equal_input() {
        let (_, eval, mut time) =
            setup_time("input a: Float32\noutput b: Float32 @1Hz:= a.aggregate(over: 5s, using: var).defaults(to:0.0)");
        let mut eval = eval.into_evaluator();
        time += Duration::from_secs(45);
        let out_ref = StreamReference::Out(0);
        let in_ref = StreamReference::In(0);
        let n = 25;
        for _ in 1..=n {
            accept_input_timed!(eval, in_ref, Float(NotNan::new(10_f64).unwrap()), time);
            time += Duration::from_secs(1);
        }
        time += Duration::from_secs(1);
        // 71 secs have passed. All values should be within the window.
        eval_stream_timed!(eval, 0, vec![], time);
        let expected = Float(NotNan::new(0.0).unwrap());
        assert_eq!(
            eval.peek_value(in_ref, &Vec::new(), 0).unwrap(),
            Float(NotNan::new(10.0).unwrap())
        );
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_var_window() {
        for (duration, exp) in &[
            ("2", Value::try_from(0.25)),
            ("3", Value::try_from(2.0 / 3.0)),
            ("4", Value::try_from(1.25)),
            ("5", Value::try_from(2.0)),
            ("6", Value::try_from(17.5 / 6.0)),
            ("7", Value::try_from(4.0)),
            ("8", Value::try_from(5.25)),
            ("9", Value::try_from(60.0 / 9.0)),
            ("10", Value::try_from(8.25)),
            ("11", Value::try_from(10.0)),
        ] {
            let (_, eval, mut time) = setup_time(&format!(
                "input a: Float32\noutput b: Float32 @1Hz:= a.aggregate(over: {}s, using: var).defaults(to:0.0)",
                duration
            ));
            let mut eval = eval.into_evaluator();
            time += Duration::from_secs(45);
            let out_ref = StreamReference::Out(0);
            let in_ref = StreamReference::In(0);
            let n = 20;
            for v in 1..=n {
                time += Duration::from_secs(1);
                accept_input_timed!(eval, in_ref, Float(NotNan::new(v as f64).unwrap()), time);
            }
            // 66 secs have passed. All values should be within the window.
            eval_stream_timed!(eval, 0, vec![], time);
            assert_eq!(
                eval.peek_value(in_ref, &Vec::new(), 0).unwrap(),
                Float(NotNan::new(20.0).unwrap())
            );
            assert_float_eq!(
                eval.peek_value(out_ref, &Vec::new(), 0).unwrap(),
                exp.as_ref().unwrap().clone()
            );
        }
    }

    #[test]
    fn test_sd_window() {
        for (duration, exp) in &[
            ("2", Value::try_from(0.25f64.sqrt())),
            ("3", Value::try_from((2.0 / 3.0f64).sqrt())),
            ("4", Value::try_from(1.25f64.sqrt())),
            ("5", Value::try_from(2.0f64.sqrt())),
            ("6", Value::try_from((17.5 / 6.0f64).sqrt())),
            ("7", Value::try_from(4.0f64.sqrt())),
            ("8", Value::try_from(5.25f64.sqrt())),
            ("9", Value::try_from((60.0 / 9.0f64).sqrt())),
            ("10", Value::try_from(8.25f64.sqrt())),
            ("11", Value::try_from(10.0f64.sqrt())),
        ] {
            let (_, eval, mut time) = setup_time(&format!(
                "input a: Float32\noutput b: Float32 @1Hz:= a.aggregate(over: {}s, using: sd).defaults(to:0.0)",
                duration
            ));
            let mut eval = eval.into_evaluator();
            time += Duration::from_secs(45);
            let out_ref = StreamReference::Out(0);
            let in_ref = StreamReference::In(0);
            let n = 20;
            for v in 1..=n {
                time += Duration::from_secs(1);
                accept_input_timed!(eval, in_ref, Float(NotNan::new(v as f64).unwrap()), time);
            }
            // 66 secs have passed. All values should be within the window.
            eval_stream_timed!(eval, 0, vec![], time);
            assert_float_eq!(
                eval.peek_value(in_ref, &Vec::new(), 0).unwrap(),
                Float(NotNan::new(20.0).unwrap())
            );
            assert_float_eq!(
                eval.peek_value(out_ref, &Vec::new(), 0).unwrap(),
                exp.as_ref().unwrap().clone()
            );
        }
    }

    #[test]
    fn test_cov() {
        let (_, eval, mut time) =
            setup_time("input in: Float32\n input in2: Float32\noutput t@in&in2:= (in,in2)\n output out: Float32 @1Hz := t.aggregate(over: 6s, using: cov).defaults(to: 1337.0)");
        let mut eval = eval.into_evaluator();
        time += Duration::from_secs(45);
        let out_ref = StreamReference::Out(1);
        let in_ref = StreamReference::In(0);
        let in_ref_2 = StreamReference::In(1);
        let n = 20;
        for v in 1..=n {
            time += Duration::from_secs(1);
            accept_input_timed!(eval, in_ref, Value::try_from(v as f64).unwrap(), time);
            accept_input_timed!(eval, in_ref_2, Value::try_from(v as f64).unwrap(), time);
            eval_stream_timed!(eval, 0, vec![], time);
        }
        // 66 secs have passed. All values should be within the window.
        eval_stream_timed!(eval, 1, vec![], time);
        let expected = Float(NotNan::new(17.5 / 6.0).unwrap());
        assert_float_eq!(
            eval.peek_value(in_ref, &Vec::new(), 0).unwrap(),
            Float(NotNan::new(20.0).unwrap())
        );
        assert_float_eq!(
            eval.peek_value(in_ref_2, &Vec::new(), 0).unwrap(),
            Float(NotNan::new(20.0).unwrap())
        );
        assert_float_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_cov_2() {
        let (_, eval, mut time) =
            setup_time("input in: Float32\n input in2: Float32\noutput t@in&in2:= (in,in2)\n output out: Float32 @1Hz := t.aggregate(over: 5s, using: cov).defaults(to: 1337.0)");
        let mut eval = eval.into_evaluator();
        time += Duration::from_secs(45);
        let out_ref = StreamReference::Out(1);
        let in_ref = StreamReference::In(0);
        let in_ref_2 = StreamReference::In(1);
        let n = 20;
        for v in 1..=n {
            accept_input_timed!(eval, in_ref, Value::try_from(v as f64).unwrap(), time);
            accept_input_timed!(eval, in_ref_2, Value::try_from(16.0).unwrap(), time);
            eval_stream_timed!(eval, 0, vec![], time);
            time += Duration::from_secs(1);
        }
        time += Duration::from_secs(1);
        // 66 secs have passed. All values should be within the window.
        eval_stream_timed!(eval, 1, vec![], time);
        let expected = Float(NotNan::new(0.0).unwrap());
        assert_eq!(
            eval.peek_value(in_ref, &Vec::new(), 0).unwrap(),
            Float(NotNan::new(20.0).unwrap())
        );
        assert_eq!(
            eval.peek_value(in_ref_2, &Vec::new(), 0).unwrap(),
            Float(NotNan::new(16.0).unwrap())
        );
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_var_discrete() {
        for (duration, exp) in &[
            ("2", Value::try_from(0.25)),
            ("3", Value::try_from(2.0 / 3.0)),
            ("4", Value::try_from(1.25)),
            ("5", Value::try_from(2.0)),
            ("6", Value::try_from(17.5 / 6.0)),
            ("7", Value::try_from(4.0)),
            ("8", Value::try_from(5.25)),
            ("9", Value::try_from(60.0 / 9.0)),
            ("10", Value::try_from(8.25)),
            ("11", Value::try_from(10.0)),
        ] {
            let (_, eval, mut time) = setup_time(&format!(
                "input a: Float32\noutput b: Float32 := a.aggregate(over_discrete: {}, using: var).defaults(to:0.0)",
                duration
            ));
            let mut eval = eval.into_evaluator();
            time += Duration::from_secs(45);
            let out_ref = StreamReference::Out(0);
            let in_ref = StreamReference::In(0);
            let n = 20;
            for v in 1..=n {
                accept_input_timed!(eval, in_ref, Float(NotNan::new(v as f64).unwrap()), time);
                time += Duration::from_secs(1);
            }
            time += Duration::from_secs(1);
            // 66 secs have passed. All values should be within the window.
            eval_stream_timed!(eval, 0, vec![], time);
            assert_eq!(
                eval.peek_value(in_ref, &Vec::new(), 0).unwrap(),
                Float(NotNan::new(20.0).unwrap())
            );
            assert_float_eq!(
                eval.peek_value(out_ref, &Vec::new(), 0).unwrap(),
                exp.as_ref().unwrap().clone()
            );
        }
    }

    #[test]
    fn test_sd_discrete() {
        for (duration, exp) in &[
            ("2", Value::try_from(0.25f64.sqrt())),
            ("3", Value::try_from((2.0 / 3.0f64).sqrt())),
            ("4", Value::try_from(1.25f64.sqrt())),
            ("5", Value::try_from(2.0f64.sqrt())),
            ("6", Value::try_from((17.5 / 6.0f64).sqrt())),
            ("7", Value::try_from(4.0f64.sqrt())),
            ("8", Value::try_from(5.25f64.sqrt())),
            ("9", Value::try_from((60.0 / 9.0f64).sqrt())),
            ("10", Value::try_from(8.25f64.sqrt())),
            ("11", Value::try_from(10.0f64.sqrt())),
        ] {
            let (_, eval, mut time) = setup_time(&format!(
                "input a: Float32\noutput b: Float32 := a.aggregate(over_discrete: {}, using: sd).defaults(to:0.0)",
                duration
            ));
            let mut eval = eval.into_evaluator();
            time += Duration::from_secs(45);
            let out_ref = StreamReference::Out(0);
            let in_ref = StreamReference::In(0);
            let n = 20;
            for v in 1..=n {
                accept_input_timed!(eval, in_ref, Float(NotNan::new(v as f64).unwrap()), time);
                time += Duration::from_secs(1);
            }
            time += Duration::from_secs(1);
            // 66 secs have passed. All values should be within the window.
            eval_stream_timed!(eval, 0, vec![], time);
            assert_eq!(
                eval.peek_value(in_ref, &Vec::new(), 0).unwrap(),
                Float(NotNan::new(20.0).unwrap())
            );
            assert_eq!(
                eval.peek_value(out_ref, &Vec::new(), 0).unwrap(),
                exp.as_ref().unwrap().clone()
            );
        }
    }

    #[test]
    fn test_cases_window_discrete_float() {
        for (aggr, exp, default) in &[
            ("sum", Value::try_from(115.0), false),
            ("min", Value::try_from(21.0), true),
            ("max", Value::try_from(25.0), true),
            ("avg", Value::try_from(23.0), true),
            ("integral", Value::try_from(92.0), false),
            ("last", Value::try_from(25.0), true),
            ("med", Value::try_from(23.0), true),
            ("pctl20", Value::try_from(21.5), true),
        ] {
            let mut spec = String::from("input a: Float32\noutput b := a.aggregate(over_discrete: 5, using: ");
            spec += aggr;
            spec += ")";
            if *default {
                spec += ".defaults(to:1337.0)"
            }
            let (_, eval, mut time) = setup_time(&spec);
            let mut eval = eval.into_evaluator();
            time += Duration::from_secs(45);
            let out_ref = StreamReference::Out(0);
            let in_ref = StreamReference::In(0);
            let n = 25;
            for v in 1..=n {
                accept_input_timed!(eval, in_ref, Value::try_from(v as f64).unwrap(), time);
                time += Duration::from_secs(1);
            }
            time += Duration::from_secs(1);
            // 71 secs have passed. All values should be within the window.
            eval_stream_timed!(eval, 0, vec![], time);
            let expected = exp.as_ref().unwrap().clone();
            assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
        }
    }

    #[test]
    fn test_cases_window_discrete_signed() {
        for (aggr, exp, default) in &[
            ("sum", Signed(115), false),
            ("count", Unsigned(5), false),
            ("min", Signed(21), true),
            ("max", Signed(25), true),
            ("avg", Signed(23), true),
            ("integral", Value::try_from(92.0).unwrap(), false),
            ("last", Signed(25), true),
            ("med", Signed(23), true),
            ("pctl20", Signed(21), true),
        ] {
            let mut spec = String::from("input a: Int16\noutput b := a.aggregate(over_discrete: 5, using: ");
            spec += aggr;
            spec += ")";
            if *default {
                spec += ".defaults(to:1337)"
            }
            let (_, eval, mut time) = setup_time(&spec);
            let mut eval = eval.into_evaluator();
            time += Duration::from_secs(45);
            let out_ref = StreamReference::Out(0);
            let in_ref = StreamReference::In(0);
            let n = 25;
            for v in 1..=n {
                accept_input_timed!(eval, in_ref, Signed(v), time);
                time += Duration::from_secs(1);
            }
            time += Duration::from_secs(1);
            // 71 secs have passed. All values should be within the window.
            eval_stream_timed!(eval, 0, vec![], time);
            let expected = exp.clone();
            assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
        }
    }

    #[test]
    fn test_cases_window_discrete_unsigned() {
        for (aggr, exp, default) in &[
            ("sum", Unsigned(115), false),
            ("count", Unsigned(5), false),
            ("min", Unsigned(21), true),
            ("max", Unsigned(25), true),
            ("avg", Unsigned(23), true),
            ("integral", Value::try_from(92.0).unwrap(), false),
            ("last", Unsigned(25), true),
            ("med", Unsigned(23), true),
            ("pctl20", Unsigned(21), true),
        ] {
            let mut spec = String::from("input a: UInt16\noutput b := a.aggregate(over_discrete: 5, using: ");
            spec += aggr;
            spec += ")";
            if *default {
                spec += ".defaults(to:1337)"
            }
            let (_, eval, mut time) = setup_time(&spec);
            let mut eval = eval.into_evaluator();
            time += Duration::from_secs(45);
            let out_ref = StreamReference::Out(0);
            let in_ref = StreamReference::In(0);
            let n = 25;
            for v in 1..=n {
                accept_input_timed!(eval, in_ref, Unsigned(v), time);
                time += Duration::from_secs(1);
            }
            time += Duration::from_secs(1);
            // 71 secs have passed. All values should be within the window.
            eval_stream_timed!(eval, 0, vec![], time);
            let expected = exp.clone();
            assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
        }
    }

    #[test]
    fn test_filter() {
        let (_, eval, start) = setup(
            "input a: Int32\n\
                   output b eval when a == 42 with a + 8",
        );
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let in_ref = StreamReference::In(0);
        accept_input!(eval, start, in_ref, Signed(15));
        eval_stream!(eval, start, out_ref.out_ix(), vec![]);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0), Option::None);

        accept_input!(eval, start, in_ref, Signed(42));
        eval_stream!(eval, start, out_ref.out_ix(), vec![]);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), Signed(50));
    }

    #[test]
    fn test_spawn_eventbased() {
        let (_, eval, start) = setup(
            "input a: Int32\n\
                  output b(x: Int32) spawn with a eval with x + a",
        );
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let in_ref = StreamReference::In(0);
        accept_input!(eval, start, in_ref, Signed(15));
        spawn_stream!(eval, start, out_ref);

        assert!(stream_has_instance!(eval, out_ref, vec![Signed(15)]));

        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &[Signed(15)], 0).unwrap(), Signed(30));
    }

    #[test]
    fn test_spawn_timedriven() {
        let (_, eval, mut time) = setup_time(
            "input a: Int32\n\
                  output b(x: Int32) spawn with a eval @1Hz with x + a.hold(or: 42)",
        );
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let in_ref = StreamReference::In(0);

        time += Duration::from_secs(5);
        accept_input_timed!(eval, in_ref, Signed(15), time);
        spawn_stream_timed!(eval, time, out_ref);
        assert!(stream_has_instance!(eval, out_ref, vec![Signed(15)]));

        time += Duration::from_secs(1);
        let mut schedule = eval.dyn_schedule.borrow_mut();
        let next_due = schedule.get_next_deadline_due().unwrap();
        let next_deadline = schedule.get_next_deadline(time).unwrap();
        assert_eq!(next_due, Duration::from_secs(6));
        assert_eq!(
            next_deadline,
            DynamicDeadline {
                due: Duration::from_secs(6),
                tasks: vec![EvaluationTask::Evaluate(out_ref.out_ix(), vec![Signed(15)])]
            }
        );

        eval_stream_timed!(eval, out_ref.out_ix(), vec![Signed(15)], time);
        assert_eq!(eval.peek_value(out_ref, &[Signed(15)], 0).unwrap(), Signed(30));
    }

    #[test]
    fn test_spawn_eventbased_unit() {
        let (_, eval, start) = setup(
            "input a: Int32\n\
                  output b spawn when a == 42 eval with a",
        );
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let in_ref = StreamReference::In(0);
        accept_input!(eval, start, in_ref, Signed(15));
        spawn_stream!(eval, start, out_ref);

        assert!(!stream_has_instance!(eval, out_ref, Vec::<Value>::new()));

        accept_input!(eval, start, in_ref, Signed(42));
        spawn_stream!(eval, start, out_ref);

        assert!(stream_has_instance!(eval, out_ref, Vec::<Value>::new()));

        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &Vec::<Value>::new(), 0).unwrap(), Signed(42));
    }

    #[test]
    fn test_spawn_timedriven_unit() {
        let (_, eval, mut time) = setup_time(
            "input a: Int32\n\
                  output b spawn when a == 42 eval @1Hz with a.hold(or: 42)",
        );
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let in_ref = StreamReference::In(0);

        time += Duration::from_secs(5);
        accept_input_timed!(eval, in_ref, Signed(15), time);
        spawn_stream_timed!(eval, time, out_ref);
        assert!(!stream_has_instance!(eval, out_ref, Vec::<Value>::new()));

        time += Duration::from_secs(5);
        accept_input_timed!(eval, in_ref, Signed(42), time);
        spawn_stream_timed!(eval, time, out_ref);
        assert!(stream_has_instance!(eval, out_ref, Vec::<Value>::new()));

        time += Duration::from_secs(1);
        let mut schedule = eval.dyn_schedule.borrow_mut();
        let next_due = schedule.get_next_deadline_due().unwrap();
        let next_deadline = schedule.get_next_deadline(time).unwrap();
        assert_eq!(next_due, Duration::from_secs(11));
        assert_eq!(
            next_deadline,
            DynamicDeadline {
                due: Duration::from_secs(11),
                tasks: vec![EvaluationTask::Evaluate(out_ref.out_ix(), vec![])]
            }
        );

        eval_stream_timed!(eval, out_ref.out_ix(), vec![], time);
        assert_eq!(eval.peek_value(out_ref, &[], 0).unwrap(), Signed(42));
    }

    // Window access a unit parameterized stream before it is spawned
    #[test]
    fn test_spawn_window_unit() {
        let (_, eval, mut time) = setup_time(
            "input a: Int32\n\
                  output b spawn when a == 42 eval with a\n\
                  output c @1Hz := b.aggregate(over: 1s, using: sum)",
        );
        let mut eval = eval.into_evaluator();
        let b_ref = StreamReference::Out(0);
        let c_ref = StreamReference::Out(1);
        let in_ref = StreamReference::In(0);

        //Stream is not spawned but c produced initial value
        time += Duration::from_secs(1);
        accept_input_timed!(eval, in_ref, Signed(15), time);
        spawn_stream_timed!(eval, time, b_ref);
        assert!(!stream_has_instance!(eval, b_ref, Vec::<Value>::new()));
        eval_stream_timed!(eval, c_ref.out_ix(), vec![], time);
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Signed(0));

        //Stream is spawned
        time += Duration::from_millis(200);
        accept_input_timed!(eval, in_ref, Signed(42), time);
        spawn_stream_timed!(eval, time, b_ref);
        assert!(stream_has_instance!(eval, b_ref, Vec::<Value>::new()));
        eval_stream_instances_timed!(eval, time, b_ref);
        assert_eq!(eval.peek_value(b_ref, &[], 0).unwrap(), Signed(42));

        //Stream gets new value
        time += Duration::from_millis(200);
        accept_input_timed!(eval, in_ref, Signed(18), time);
        eval_stream_instances_timed!(eval, time, b_ref);
        assert_eq!(eval.peek_value(b_ref, &[], 0).unwrap(), Signed(18));

        //Stream gets new value
        time += Duration::from_millis(200);
        accept_input_timed!(eval, in_ref, Signed(17), time);
        eval_stream_instances_timed!(eval, time, b_ref);
        assert_eq!(eval.peek_value(b_ref, &[], 0).unwrap(), Signed(17));

        //Stream gets new value. Window is evaluated.
        time += Duration::from_millis(400);
        accept_input_timed!(eval, in_ref, Signed(3), time);
        eval_stream_instances_timed!(eval, time, b_ref);
        assert_eq!(eval.peek_value(b_ref, &[], 0).unwrap(), Signed(3));
        eval_stream_timed!(eval, c_ref.out_ix(), vec![], time);
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Signed(80));
    }

    #[test]
    fn test_both_parameterized_window() {
        let (_, eval, mut time) = setup_time(
            "input a: Int32\n\
                  output b(p) spawn with a eval with p+a\n\
                  output c(p) spawn with a eval @1Hz with b(p).aggregate(over: 2s, using: sum)",
        );
        let mut eval = eval.into_evaluator();
        let b_ref = StreamReference::Out(0);
        let c_ref = StreamReference::Out(1);
        let in_ref = StreamReference::In(0);

        time += Duration::from_secs(1);
        accept_input_timed!(eval, in_ref, Signed(15), time);
        spawn_stream_timed!(eval, time, b_ref);
        spawn_stream_timed!(eval, time, c_ref);
        assert!(stream_has_instance!(eval, b_ref, vec![Signed(15)]));
        assert!(stream_has_instance!(eval, c_ref, vec![Signed(15)]));
        eval_stream_timed!(eval, b_ref.out_ix(), vec![Signed(15)], time);
        eval_stream_timed!(eval, c_ref.out_ix(), vec![Signed(15)], time);
        assert_eq!(eval.peek_value(b_ref, &[Signed(15)], 0).unwrap(), Signed(30));
        assert_eq!(eval.peek_value(c_ref, &[Signed(15)], 0).unwrap(), Signed(30));

        time += Duration::from_secs(1);
        accept_input_timed!(eval, in_ref, Signed(5), time);
        spawn_stream_timed!(eval, time, b_ref);
        spawn_stream_timed!(eval, time, c_ref);
        assert!(stream_has_instance!(eval, b_ref, vec![Signed(5)]));
        assert!(stream_has_instance!(eval, c_ref, vec![Signed(5)]));
        eval_stream_timed!(eval, b_ref.out_ix(), vec![Signed(5)], time);
        eval_stream_timed!(eval, c_ref.out_ix(), vec![Signed(5)], time);
        eval_stream_timed!(eval, b_ref.out_ix(), vec![Signed(15)], time);
        eval_stream_timed!(eval, c_ref.out_ix(), vec![Signed(15)], time);
        assert_eq!(eval.peek_value(b_ref, &[Signed(5)], 0).unwrap(), Signed(10));
        assert_eq!(eval.peek_value(c_ref, &[Signed(5)], 0).unwrap(), Signed(10));
        assert_eq!(eval.peek_value(b_ref, &[Signed(15)], 0).unwrap(), Signed(20));
        assert_eq!(eval.peek_value(c_ref, &[Signed(15)], 0).unwrap(), Signed(50));

        time += Duration::from_secs(1);
        accept_input_timed!(eval, in_ref, Signed(5), time);
        eval_stream_timed!(eval, b_ref.out_ix(), vec![Signed(5)], time);
        eval_stream_timed!(eval, c_ref.out_ix(), vec![Signed(5)], time);
        eval_stream_timed!(eval, b_ref.out_ix(), vec![Signed(15)], time);
        eval_stream_timed!(eval, c_ref.out_ix(), vec![Signed(15)], time);
        assert_eq!(eval.peek_value(b_ref, &[Signed(5)], 0).unwrap(), Signed(10));
        assert_eq!(eval.peek_value(c_ref, &[Signed(5)], 0).unwrap(), Signed(20));
        assert_eq!(eval.peek_value(b_ref, &[Signed(15)], 0).unwrap(), Signed(20));
        assert_eq!(eval.peek_value(c_ref, &[Signed(15)], 0).unwrap(), Signed(40));
    }

    #[test]
    fn test_spawn_window_unit3() {
        let (_, eval, mut time) = setup_time(
            "input a: Int32\n\
                  output b(p) spawn with a eval with p+a\n\
                  output c spawn when a == 5 eval @1Hz with b(15).aggregate(over: 2s, using: sum)",
        );
        let mut eval = eval.into_evaluator();
        let b_ref = StreamReference::Out(0);
        let c_ref = StreamReference::Out(1);
        let in_ref = StreamReference::In(0);
        let empty: Vec<Value> = vec![];

        time += Duration::from_secs(1);
        accept_input_timed!(eval, in_ref, Signed(15), time);
        spawn_stream_timed!(eval, time, b_ref);
        spawn_stream_timed!(eval, time, c_ref);
        assert!(stream_has_instance!(eval, b_ref, vec![Signed(15)]));
        assert!(!stream_has_instance!(eval, c_ref, empty));
        eval_stream_timed!(eval, b_ref.out_ix(), vec![Signed(15)], time);
        assert_eq!(eval.peek_value(b_ref, &[Signed(15)], 0).unwrap(), Signed(30));

        time += Duration::from_secs(1);
        eval.new_cycle(time);
        accept_input_timed!(eval, in_ref, Signed(5), time);
        spawn_stream_timed!(eval, time, b_ref);
        spawn_stream_timed!(eval, time, c_ref);
        assert!(stream_has_instance!(eval, b_ref, vec![Signed(5)]));
        assert!(stream_has_instance!(eval, c_ref, empty));
        eval_stream_timed!(eval, b_ref.out_ix(), vec![Signed(5)], time);
        eval_stream_timed!(eval, b_ref.out_ix(), vec![Signed(15)], time);
        eval_stream_timed!(eval, c_ref.out_ix(), vec![], time);
        assert_eq!(eval.peek_value(b_ref, &[Signed(5)], 0).unwrap(), Signed(10));
        assert_eq!(eval.peek_value(b_ref, &[Signed(15)], 0).unwrap(), Signed(20));
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Signed(20));

        time += Duration::from_secs(1);
        eval.new_cycle(time);
        accept_input_timed!(eval, in_ref, Signed(5), time);
        eval_stream_timed!(eval, b_ref.out_ix(), vec![Signed(5)], time);
        eval_stream_timed!(eval, b_ref.out_ix(), vec![Signed(15)], time);
        eval_stream_timed!(eval, c_ref.out_ix(), vec![], time);
        assert_eq!(eval.peek_value(b_ref, &[Signed(5)], 0).unwrap(), Signed(10));
        assert_eq!(eval.peek_value(b_ref, &[Signed(15)], 0).unwrap(), Signed(20));
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Signed(40));
    }

    // Window access a unit parameterized stream after it is spawned
    #[test]
    fn test_spawn_window_unit2() {
        let (_, eval, mut time) = setup_time(
            "input a: Int32\n\
                  output b spawn when a == 42 eval with a\n\
                  output c @1Hz := b.aggregate(over: 1s, using: sum)",
        );
        let mut eval = eval.into_evaluator();
        let b_ref = StreamReference::Out(0);
        let c_ref = StreamReference::Out(1);
        let in_ref = StreamReference::In(0);

        //Stream is spawned
        time += Duration::from_millis(200);
        accept_input_timed!(eval, in_ref, Signed(42), time);
        spawn_stream_timed!(eval, time, b_ref);
        assert!(stream_has_instance!(eval, b_ref, Vec::<Value>::new()));
        eval_stream_instances_timed!(eval, time, b_ref);
        assert_eq!(eval.peek_value(b_ref, &[], 0).unwrap(), Signed(42));

        //Stream gets new value
        time += Duration::from_millis(200);
        accept_input_timed!(eval, in_ref, Signed(18), time);
        eval_stream_instances_timed!(eval, time, b_ref);
        assert_eq!(eval.peek_value(b_ref, &[], 0).unwrap(), Signed(18));

        //Stream gets new value
        time += Duration::from_millis(200);
        accept_input_timed!(eval, in_ref, Signed(17), time);
        eval_stream_instances_timed!(eval, time, b_ref);
        assert_eq!(eval.peek_value(b_ref, &[], 0).unwrap(), Signed(17));

        //Stream gets new value. Window is evaluated.
        time += Duration::from_millis(400);
        accept_input_timed!(eval, in_ref, Signed(3), time);
        eval_stream_instances_timed!(eval, time, b_ref);
        assert_eq!(eval.peek_value(b_ref, &[], 0).unwrap(), Signed(3));
        eval_stream_timed!(eval, c_ref.out_ix(), vec![], time);
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Signed(80));
    }

    // p = true -> only gets 42 values
    // p = false -> get als remaining values
    #[test]
    fn test_spawn_window_parameterized() {
        let (_, eval, mut time) = setup_time(
            "input a: Int32\n\
                  output b(p: Bool) spawn with a == 42 eval when !p || a == 42 with a\n\
                  output c @1Hz := b(false).aggregate(over: 1s, using: sum)\n\
                  output d @1Hz := b(true).aggregate(over: 1s, using: sum)",
        );
        let mut eval = eval.into_evaluator();
        let b_ref = StreamReference::Out(0);
        let c_ref = StreamReference::Out(1);
        let d_ref = StreamReference::Out(2);
        let in_ref = StreamReference::In(0);

        //Instance b(false) is spawned
        time += Duration::from_millis(500);
        accept_input_timed!(eval, in_ref, Signed(15), time);
        spawn_stream_timed!(eval, time, b_ref);
        assert!(stream_has_instance!(eval, b_ref, vec![Bool(false)]));
        assert!(!stream_has_instance!(eval, b_ref, vec![Bool(true)]));
        eval_stream_instances_timed!(eval, time, b_ref);
        assert_eq!(eval.peek_value(b_ref, &[Bool(false)], 0).unwrap(), Signed(15));

        //Instance b(false) gets new value
        //Instance b(true) is spawned
        //Timed streams are evaluated
        time += Duration::from_millis(500);
        accept_input_timed!(eval, in_ref, Signed(42), time);
        spawn_stream_timed!(eval, time, b_ref);
        assert!(stream_has_instance!(eval, b_ref, vec![Bool(true)]));
        eval_stream_instances_timed!(eval, time, b_ref);
        assert_eq!(eval.peek_value(b_ref, &[Bool(false)], 0).unwrap(), Signed(42));
        assert_eq!(eval.peek_value(b_ref, &[Bool(true)], 0).unwrap(), Signed(42));
        eval_stream_timed!(eval, c_ref.out_ix(), vec![], time);
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Signed(57));
        eval_stream_timed!(eval, d_ref.out_ix(), vec![], time);
        assert_eq!(eval.peek_value(d_ref, &[], 0).unwrap(), Signed(42));

        //Instance b(false) gets new value
        //Instance b(true) gets new value
        //Timed streams are evaluated
        time += Duration::from_secs(1);
        accept_input_timed!(eval, in_ref, Signed(42), time);
        eval_stream_instances_timed!(eval, time, b_ref);
        assert_eq!(eval.peek_value(b_ref, &[Bool(false)], 0).unwrap(), Signed(42));
        assert_eq!(eval.peek_value(b_ref, &[Bool(true)], 0).unwrap(), Signed(42));
        eval_stream_timed!(eval, c_ref.out_ix(), vec![], time);
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Signed(42));
        eval_stream_timed!(eval, d_ref.out_ix(), vec![], time);
        assert_eq!(eval.peek_value(d_ref, &[], 0).unwrap(), Signed(42));
    }

    #[test]
    fn test_close_parameterized() {
        let (_, eval, start) = setup(
            "input a: Int32\n\
                  input b: Bool\n\
                  output c(x: Int32) spawn with a close when b && (x % 2 == 0) eval with x + a",
        );
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let a_ref = StreamReference::In(0);
        let b_ref = StreamReference::In(1);
        accept_input!(eval, start, a_ref, Signed(15));
        spawn_stream!(eval, start, out_ref);

        assert!(stream_has_instance!(eval, out_ref, vec![Signed(15)]));

        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &[Signed(15)], 0).unwrap(), Signed(30));

        accept_input!(eval, start, b_ref, Bool(false));
        accept_input!(eval, start, a_ref, Signed(8));
        spawn_stream!(eval, start, out_ref);

        assert!(stream_has_instance!(eval, out_ref, vec![Signed(15)]));
        assert!(stream_has_instance!(eval, out_ref, vec![Signed(8)]));

        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &[Signed(15)], 0).unwrap(), Signed(23));
        assert_eq!(eval.peek_value(out_ref, &[Signed(8)], 0).unwrap(), Signed(16));

        // Close has no effect, because it is false
        eval_close!(eval, start, out_ref, vec![Signed(15)]);
        eval_close!(eval, start, out_ref, vec![Signed(8)]);
        assert!(stream_has_instance!(eval, out_ref, vec![Signed(15)]));
        assert!(stream_has_instance!(eval, out_ref, vec![Signed(8)]));

        accept_input!(eval, start, b_ref, Bool(true));

        eval_close!(eval, start, out_ref, vec![Signed(15)]);
        eval_close!(eval, start, out_ref, vec![Signed(8)]);
        assert!(stream_has_instance!(eval, out_ref, vec![Signed(15)]));
        assert!(!stream_has_instance!(eval, out_ref, vec![Signed(8)]));
        assert!(eval.peek_value(out_ref, &[Signed(8)], 0).is_none());
    }

    #[test]
    fn test_close_unit() {
        let (_, eval, start) = setup(
            "input a: Int32\n\
                  input b: Bool\n\
                  output c spawn when a = 42 close when b eval with a",
        );
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let a_ref = StreamReference::In(0);
        let b_ref = StreamReference::In(1);
        accept_input!(eval, start, a_ref, Signed(42));
        spawn_stream!(eval, start, out_ref);

        assert!(stream_has_instance!(eval, out_ref, Vec::<Value>::new()));

        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &Vec::<Value>::new(), 0).unwrap(), Signed(42));

        accept_input!(eval, start, b_ref, Bool(false));
        accept_input!(eval, start, a_ref, Signed(8));

        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &Vec::<Value>::new(), 0).unwrap(), Signed(8));

        // Close has no effect, because it is false
        eval_close!(eval, start, out_ref, Vec::<Value>::new());
        assert!(stream_has_instance!(eval, out_ref, Vec::<Value>::new()));

        accept_input!(eval, start, b_ref, Bool(true));

        eval_close!(eval, start, out_ref, Vec::<Value>::new());
        assert!(!stream_has_instance!(eval, out_ref, Vec::<Value>::new()));
        assert!(eval.peek_value(out_ref, &Vec::<Value>::new(), 0).is_none());
    }

    #[test]
    fn test_close_selfref_unit() {
        let (_, eval, start) = setup(
            "input a: Int32\n\
                  output c spawn when a = 42 close when c = 1337 eval with a",
        );
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let a_ref = StreamReference::In(0);
        accept_input!(eval, start, a_ref, Signed(42));
        spawn_stream!(eval, start, out_ref);

        assert!(stream_has_instance!(eval, out_ref, Vec::<Value>::new()));

        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &Vec::<Value>::new(), 0).unwrap(), Signed(42));

        accept_input!(eval, start, a_ref, Signed(1337));

        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &Vec::<Value>::new(), 0).unwrap(), Signed(1337));

        eval_close!(eval, start, out_ref, Vec::<Value>::new());
        assert!(!stream_has_instance!(eval, out_ref, Vec::<Value>::new()));
        assert!(eval.peek_value(out_ref, &Vec::<Value>::new(), 0).is_none());
    }

    #[test]
    fn test_close_selfref_parameter() {
        let (_, eval, start) = setup(
            "input a: Int32\n\
                  output c(p: Int32) spawn with a close when c(p) = 1337 eval with p+a",
        );
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let a_ref = StreamReference::In(0);
        accept_input!(eval, start, a_ref, Signed(15));
        spawn_stream!(eval, start, out_ref);

        assert!(stream_has_instance!(eval, out_ref, vec![Signed(15)]));

        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &[Signed(15)], 0).unwrap(), Signed(30));

        accept_input!(eval, start, a_ref, Signed(1322));
        spawn_stream!(eval, start, out_ref);
        assert!(stream_has_instance!(eval, out_ref, vec![Signed(1322)]));

        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &[Signed(15)], 0).unwrap(), Signed(1337));
        assert_eq!(eval.peek_value(out_ref, &[Signed(1322)], 0).unwrap(), Signed(2644));

        eval_close!(eval, start, out_ref, vec![Signed(15)]);
        eval_close!(eval, start, out_ref, vec![Signed(1322)]);
        assert!(!stream_has_instance!(eval, out_ref, vec![Signed(15)]));
        assert!(stream_has_instance!(eval, out_ref, vec![Signed(1322)]));
        assert!(eval.peek_value(out_ref, &[Signed(15)], 0).is_none());
    }

    // p = true -> only gets 42 values
    // p = false -> get all remaining values
    #[test]
    fn test_close_window_parameterized() {
        let (_, eval, mut time) = setup_time(
            "input a: Int32\n\
                  output b(p: Bool) spawn with a == 42  close when b(p) == 1337 eval when !p || a == 42 with a\n\
                  output c @1Hz := b(false).aggregate(over: 1s, using: sum)\n\
                  output d @1Hz := b(true).aggregate(over: 1s, using: sum)",
        );
        let mut eval = eval.into_evaluator();
        let b_ref = StreamReference::Out(0);
        let c_ref = StreamReference::Out(1);
        let d_ref = StreamReference::Out(2);
        let in_ref = StreamReference::In(0);

        //Intance b(false) is spawned
        time += Duration::from_millis(500);
        eval.new_cycle(time);
        accept_input_timed!(eval, in_ref, Signed(15), time);
        spawn_stream_timed!(eval, time, b_ref);
        assert!(stream_has_instance!(eval, b_ref, vec![Bool(false)]));
        assert!(!stream_has_instance!(eval, b_ref, vec![Bool(true)]));
        eval_stream_instances_timed!(eval, time, b_ref);
        assert_eq!(eval.peek_value(b_ref, &[Bool(false)], 0).unwrap(), Signed(15));

        //Intance b(false) gets new value
        //Intance b(true) is spawned
        //Timed streams are evaluated
        time += Duration::from_millis(500);
        eval.new_cycle(time);
        accept_input_timed!(eval, in_ref, Signed(42), time);
        spawn_stream_timed!(eval, time, b_ref);
        assert!(stream_has_instance!(eval, b_ref, vec![Bool(true)]));
        eval_stream_instances_timed!(eval, time, b_ref);
        assert_eq!(eval.peek_value(b_ref, &[Bool(false)], 0).unwrap(), Signed(42));
        assert_eq!(eval.peek_value(b_ref, &[Bool(true)], 0).unwrap(), Signed(42));
        eval_stream_timed!(eval, c_ref.out_ix(), vec![], time);
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Signed(57));
        eval_stream_timed!(eval, d_ref.out_ix(), vec![], time);
        assert_eq!(eval.peek_value(d_ref, &[], 0).unwrap(), Signed(42));

        //Intance b(false) gets new value
        // Instance b(false) is closed
        time += Duration::from_millis(500);
        eval.new_cycle(time);
        accept_input_timed!(eval, in_ref, Signed(1337), time);
        eval.prepare_evaluation(time);
        eval_stream_instances_timed!(eval, time, b_ref);
        assert_eq!(eval.peek_value(b_ref, &[Bool(false)], 0).unwrap(), Signed(1337));
        assert_eq!(eval.peek_value(b_ref, &[Bool(true)], 0).unwrap(), Signed(42));
        eval_close_timed!(eval, time, b_ref, &vec![Bool(false)]);
        eval_close_timed!(eval, time, b_ref, &vec![Bool(true)]);
        assert!(!stream_has_instance!(eval, b_ref, vec![Bool(false)]));
        assert!(stream_has_instance!(eval, b_ref, vec![Bool(true)]));

        //Eval timed streams again
        time += Duration::from_millis(500);
        eval.new_cycle(time);
        eval.prepare_evaluation(time);
        eval_stream_timed!(eval, c_ref.out_ix(), vec![], time);
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Signed(1337));
        eval_stream_timed!(eval, d_ref.out_ix(), vec![], time);
        assert_eq!(eval.peek_value(d_ref, &[], 0).unwrap(), Signed(0));

        //Check window instance is closed
        time += Duration::from_millis(550);
        eval.new_cycle(time);
        eval.prepare_evaluation(time);
        let window = eval.ir.sliding_windows[0].reference;
        assert!(eval
            .global_store
            .get_window_collection_mut(window)
            .window(&[Bool(false)])
            .is_none())
    }

    #[test]
    fn test_close_window_unit() {
        let (_, eval, mut time) = setup_time(
            "input a: Int32\n\
                  output b spawn when a == 42 close when a = 1337 eval with a\n\
                  output c @1Hz := b.aggregate(over: 1s, using: sum)",
        );
        let mut eval = eval.into_evaluator();
        let b_ref = StreamReference::Out(0);
        let c_ref = StreamReference::Out(1);
        let in_ref = StreamReference::In(0);

        //Stream is spawned
        time += Duration::from_millis(200);
        accept_input_timed!(eval, in_ref, Signed(42), time);
        spawn_stream_timed!(eval, time, b_ref);
        assert!(stream_has_instance!(eval, b_ref, Vec::<Value>::new()));
        eval_stream_instances_timed!(eval, time, b_ref);
        assert_eq!(eval.peek_value(b_ref, &[], 0).unwrap(), Signed(42));

        //Stream gets new value
        time += Duration::from_millis(200);
        accept_input_timed!(eval, in_ref, Signed(18), time);
        eval_stream_instances_timed!(eval, time, b_ref);
        assert_eq!(eval.peek_value(b_ref, &[], 0).unwrap(), Signed(18));

        //Stream gets new value
        time += Duration::from_millis(200);
        accept_input_timed!(eval, in_ref, Signed(17), time);
        eval_stream_instances_timed!(eval, time, b_ref);
        assert_eq!(eval.peek_value(b_ref, &[], 0).unwrap(), Signed(17));

        //Stream gets new value. Window is evaluated.
        time += Duration::from_millis(400);
        accept_input_timed!(eval, in_ref, Signed(3), time);
        eval_stream_instances_timed!(eval, time, b_ref);
        assert_eq!(eval.peek_value(b_ref, &[], 0).unwrap(), Signed(3));
        eval_stream_timed!(eval, c_ref.out_ix(), vec![], time);
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Signed(80));

        //Stream gets new value and is closed
        time += Duration::from_millis(500);
        accept_input_timed!(eval, in_ref, Signed(1337), time);
        eval_stream_instances_timed!(eval, time, b_ref);
        assert_eq!(eval.peek_value(b_ref, &[], 0).unwrap(), Signed(1337));
        eval_close_timed!(eval, time, b_ref, &vec![]);
        assert!(!stream_has_instance!(eval, b_ref, Vec::<Value>::new()));

        //Timed streams are evaluated again
        time += Duration::from_millis(500);
        eval.prepare_evaluation(time);
        eval_stream_timed!(eval, c_ref.out_ix(), vec![], time);
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Signed(1337));
    }

    #[test]
    fn test_optional_tuple_access() {
        let (_, eval, start) = setup(
            "input a: (UInt, (Bool, Float))\n\
             output b := a.offset(by: -1).1.0.defaults(to: false)",
        );
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let a_ref = StreamReference::In(0);
        accept_input!(
            eval,
            start,
            a_ref,
            Tuple(Box::new([
                Unsigned(42),
                Tuple(Box::new([Bool(true), Float(NotNan::new(1.5).unwrap())]))
            ]))
        );

        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &[], 0).unwrap(), Bool(false));

        accept_input!(
            eval,
            start,
            a_ref,
            Tuple(Box::new([
                Unsigned(13),
                Tuple(Box::new([Bool(false), Float(NotNan::new(42.0).unwrap())]))
            ]))
        );

        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &[], 0).unwrap(), Bool(true));
    }

    #[test]
    fn test_instance_aggr_all_all() {
        let (_, eval, start) = setup(
            "input a: Int32\n\
                  output b(x: Int32) \
                    spawn with a \
                    eval @a with x % 2 = 0 \
                    close when a == 42 \
                  output c @a := b.aggregate(over_instances: all, using: forall)",
        );
        let mut eval = eval.into_evaluator();
        let b_ref = StreamReference::Out(0);
        let c_ref = StreamReference::Out(1);
        let in_ref = StreamReference::In(0);
        accept_input!(eval, start, in_ref, Signed(16));
        spawn_stream!(eval, start, b_ref);
        eval_stream_instances!(eval, start, b_ref);
        eval_stream_instances!(eval, start, c_ref);

        assert!(stream_has_instance!(eval, b_ref, vec![Signed(16)]));

        assert_eq!(eval.peek_value(b_ref, &[Signed(16)], 0).unwrap(), Bool(true));
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Bool(true));

        let mut time = start + Duration::from_secs(1);

        accept_input!(eval, time, in_ref, Signed(7));
        spawn_stream!(eval, time, b_ref);
        eval_stream_instances!(eval, time, b_ref);
        eval_stream_instances!(eval, time, c_ref);

        assert!(stream_has_instance!(eval, b_ref, vec![Signed(7)]));

        assert_eq!(eval.peek_value(b_ref, &[Signed(16)], 0).unwrap(), Bool(true));
        assert_eq!(eval.peek_value(b_ref, &[Signed(7)], 0).unwrap(), Bool(false));
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Bool(false));

        time += Duration::from_secs(1);

        accept_input!(eval, time, in_ref, Signed(42));
        spawn_stream!(eval, time, b_ref);
        eval_stream_instances!(eval, time, b_ref);
        eval_stream_instances!(eval, time, c_ref);

        assert!(stream_has_instance!(eval, b_ref, vec![Signed(42)]));

        assert_eq!(eval.peek_value(b_ref, &[Signed(16)], 0).unwrap(), Bool(true));
        assert_eq!(eval.peek_value(b_ref, &[Signed(7)], 0).unwrap(), Bool(false));
        assert_eq!(eval.peek_value(b_ref, &[Signed(42)], 0).unwrap(), Bool(true));
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Bool(false));

        eval_close!(eval, time, b_ref, vec![Signed(16)]);
        eval_close!(eval, time, b_ref, vec![Signed(7)]);
        eval_close!(eval, time, b_ref, vec![Signed(42)]);

        time += Duration::from_secs(1);

        accept_input!(eval, time, in_ref, Signed(16));
        spawn_stream!(eval, time, b_ref);
        eval_stream_instances!(eval, time, b_ref);
        eval_stream_instances!(eval, time, c_ref);

        assert!(stream_has_instance!(eval, b_ref, vec![Signed(16)]));

        assert_eq!(eval.peek_value(b_ref, &[Signed(16)], 0).unwrap(), Bool(true));
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Bool(true));
    }

    #[test]
    fn test_instance_aggr_all_any() {
        let (_, eval, start) = setup(
            "input a: Int32\n\
                  output b(x: Int32) \
                    spawn with a \
                    eval @a with x % 2 = 0 \
                    close when a == 42 \
                  output c @a := b.aggregate(over_instances: all, using: exists)",
        );
        let mut eval = eval.into_evaluator();
        let b_ref = StreamReference::Out(0);
        let c_ref = StreamReference::Out(1);
        let in_ref = StreamReference::In(0);
        accept_input!(eval, start, in_ref, Signed(17));
        spawn_stream!(eval, start, b_ref);
        eval_stream_instances!(eval, start, b_ref);
        eval_stream_instances!(eval, start, c_ref);

        assert!(stream_has_instance!(eval, b_ref, vec![Signed(17)]));

        assert_eq!(eval.peek_value(b_ref, &[Signed(17)], 0).unwrap(), Bool(false));
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Bool(false));

        let mut time = start + Duration::from_secs(1);

        accept_input!(eval, time, in_ref, Signed(6));
        spawn_stream!(eval, time, b_ref);
        eval_stream_instances!(eval, time, b_ref);
        eval_stream_instances!(eval, time, c_ref);

        assert!(stream_has_instance!(eval, b_ref, vec![Signed(6)]));

        assert_eq!(eval.peek_value(b_ref, &[Signed(17)], 0).unwrap(), Bool(false));
        assert_eq!(eval.peek_value(b_ref, &[Signed(6)], 0).unwrap(), Bool(true));
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Bool(true));

        time += Duration::from_secs(1);

        accept_input!(eval, time, in_ref, Signed(42));
        spawn_stream!(eval, time, b_ref);
        eval_stream_instances!(eval, time, b_ref);
        eval_stream_instances!(eval, time, c_ref);

        assert!(stream_has_instance!(eval, b_ref, vec![Signed(42)]));

        assert_eq!(eval.peek_value(b_ref, &[Signed(17)], 0).unwrap(), Bool(false));
        assert_eq!(eval.peek_value(b_ref, &[Signed(6)], 0).unwrap(), Bool(true));
        assert_eq!(eval.peek_value(b_ref, &[Signed(42)], 0).unwrap(), Bool(true));
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Bool(true));

        eval_close!(eval, time, b_ref, vec![Signed(17)]);
        eval_close!(eval, time, b_ref, vec![Signed(6)]);
        eval_close!(eval, time, b_ref, vec![Signed(42)]);

        time += Duration::from_secs(1);

        accept_input!(eval, time, in_ref, Signed(17));
        spawn_stream!(eval, time, b_ref);
        eval_stream_instances!(eval, time, b_ref);
        eval_stream_instances!(eval, time, c_ref);

        assert!(stream_has_instance!(eval, b_ref, vec![Signed(17)]));

        assert_eq!(eval.peek_value(b_ref, &[Signed(17)], 0).unwrap(), Bool(false));
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Bool(false));
    }

    #[test]
    fn test_instance_aggr_fresh_all() {
        let (_, eval, start) = setup(
            "input a: Int32\n\
                   input i: Int32\n\
                  output b(x: Int32)\n\
                    spawn with a\n\
                    eval when (x + i) % 2 = 0 with x > 10\n\
                    close when a == 42\n\
                  output c := b.aggregate(over_instances: fresh, using: forall)",
        );
        let mut eval = eval.into_evaluator();
        let b_ref = StreamReference::Out(0);
        let c_ref = StreamReference::Out(1);
        let a_ref = StreamReference::In(0);
        let i_ref = StreamReference::In(1);
        accept_input!(eval, start, a_ref, Signed(16));
        accept_input!(eval, start, i_ref, Signed(0));
        spawn_stream!(eval, start, b_ref);
        eval_stream_instances!(eval, start, b_ref);
        eval_stream_instances!(eval, start, c_ref);

        assert!(stream_has_instance!(eval, b_ref, vec![Signed(16)]));

        assert_eq!(eval.peek_value(b_ref, &[Signed(16)], 0).unwrap(), Bool(true));
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Bool(true));

        let mut time = start + Duration::from_secs(1);
        eval.new_cycle(time);

        accept_input!(eval, time, a_ref, Signed(6));
        accept_input!(eval, time, i_ref, Signed(0));
        spawn_stream!(eval, time, b_ref);
        eval_stream_instances!(eval, time, b_ref);
        eval_stream_instances!(eval, time, c_ref);

        assert!(stream_has_instance!(eval, b_ref, vec![Signed(6)]));

        assert_eq!(eval.peek_value(b_ref, &[Signed(16)], 0).unwrap(), Bool(true));
        assert_eq!(eval.peek_value(b_ref, &[Signed(6)], 0).unwrap(), Bool(false));
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Bool(false));

        time += Duration::from_secs(1);
        eval.new_cycle(time);

        accept_input!(eval, time, a_ref, Signed(11));
        accept_input!(eval, time, i_ref, Signed(1));
        spawn_stream!(eval, time, b_ref);
        eval_stream_instances!(eval, time, b_ref);
        eval_stream_instances!(eval, time, c_ref);

        assert!(stream_has_instance!(eval, b_ref, vec![Signed(11)]));

        assert_eq!(eval.peek_value(b_ref, &[Signed(16)], 0).unwrap(), Bool(true));
        assert_eq!(eval.peek_value(b_ref, &[Signed(6)], 0).unwrap(), Bool(false));
        assert_eq!(eval.peek_value(b_ref, &[Signed(11)], 0).unwrap(), Bool(true));
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Bool(true));

        time += Duration::from_secs(1);
        eval.new_cycle(time);

        accept_input!(eval, time, a_ref, Signed(42));
        accept_input!(eval, time, i_ref, Signed(1));
        spawn_stream!(eval, time, b_ref);
        eval_stream_instances!(eval, time, b_ref);
        eval_stream_instances!(eval, time, c_ref);

        assert!(stream_has_instance!(eval, b_ref, vec![Signed(42)]));

        assert_eq!(eval.peek_value(b_ref, &[Signed(16)], 0).unwrap(), Bool(true));
        assert_eq!(eval.peek_value(b_ref, &[Signed(6)], 0).unwrap(), Bool(false));
        assert_eq!(eval.peek_value(b_ref, &[Signed(11)], 0).unwrap(), Bool(true));
        assert!(eval.peek_value(b_ref, &[Signed(42)], 0).is_none());
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Bool(true));

        eval_close!(eval, time, b_ref, vec![Signed(16)]);
        eval_close!(eval, time, b_ref, vec![Signed(6)]);
        eval_close!(eval, time, b_ref, vec![Signed(11)]);
        eval_close!(eval, time, b_ref, vec![Signed(42)]);

        time += Duration::from_secs(1);
        eval.new_cycle(time);

        accept_input!(eval, time, a_ref, Signed(4));
        accept_input!(eval, time, i_ref, Signed(0));
        spawn_stream!(eval, time, b_ref);
        eval_stream_instances!(eval, time, b_ref);
        eval_stream_instances!(eval, time, c_ref);

        assert!(stream_has_instance!(eval, b_ref, vec![Signed(4)]));

        assert_eq!(eval.peek_value(b_ref, &[Signed(4)], 0).unwrap(), Bool(false));
        assert_eq!(eval.peek_value(c_ref, &[], 0).unwrap(), Bool(false));
    }

    #[test]
    fn test_multiple_eval_clauses() {
        let (_, eval, start) = setup(
            "input a : UInt64\n\
            output b
                eval @a when a < 10 with a + 1
                eval @a when a < 20 with a + 2
                eval @a with a + 3",
        );
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let a_ref = StreamReference::In(0);

        accept_input!(eval, start, a_ref, Unsigned(0));
        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &[], 0).unwrap(), Unsigned(1));
        accept_input!(eval, start, a_ref, Unsigned(12));
        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &[], 0).unwrap(), Unsigned(14));
        accept_input!(eval, start, a_ref, Unsigned(20));
        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &[], 0).unwrap(), Unsigned(23));
    }

    #[test]
    fn test_multiple_eval_clauses_no_filter() {
        let (_, eval, start) = setup(
            "input a : UInt64\n\
            output b
                eval @a with a + 1
                eval @a when a < 20 with a + 2",
        );
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let a_ref = StreamReference::In(0);

        accept_input!(eval, start, a_ref, Unsigned(0));
        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &[], 0).unwrap(), Unsigned(1));
        accept_input!(eval, start, a_ref, Unsigned(12));
        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &[], 0).unwrap(), Unsigned(13));
    }

    #[test]
    fn test_multiple_eval_clauses_different_ac() {
        let (_, eval, start) = setup(
            "input a : UInt64\ninput b : UInt64\n\
            output c : UInt64
                eval @a&&b with 1
                eval @a with 2",
        );
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let a_ref = StreamReference::In(0);
        let b_ref = StreamReference::In(1);

        accept_input!(eval, start, a_ref, Unsigned(0));
        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &[], 0).unwrap(), Unsigned(2));
        accept_input!(eval, start, a_ref, Unsigned(1));
        accept_input!(eval, start, b_ref, Unsigned(1));
        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &[], 0).unwrap(), Unsigned(1));
    }

    #[test]
    fn test_multiple_eval_clauses_ac_and_filter() {
        let (_, eval, start) = setup(
            "input a : UInt64\ninput b : UInt64\n\
            output c : UInt64
                eval @a&&b when b < 10 with 1
                eval @a&&b with 3
                eval @a when a < 10 with 2
                eval @a with 4",
        );
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let a_ref = StreamReference::In(0);
        let b_ref = StreamReference::In(1);

        accept_input!(eval, start, a_ref, Unsigned(11));
        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &[], 0).unwrap(), Unsigned(4));

        accept_input!(eval, start, a_ref, Unsigned(0));
        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &[], 0).unwrap(), Unsigned(2));

        accept_input!(eval, start, a_ref, Unsigned(1));
        accept_input!(eval, start, b_ref, Unsigned(11));
        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &[], 0).unwrap(), Unsigned(3));

        accept_input!(eval, start, a_ref, Unsigned(1));
        accept_input!(eval, start, b_ref, Unsigned(1));
        eval_stream_instances!(eval, start, out_ref);
        assert_eq!(eval.peek_value(out_ref, &[], 0).unwrap(), Unsigned(1));
    }

    // Corresponds to issue #18
    #[test]
    fn test_parameterized_window_bug() {
        let (_, eval, mut time) = setup(
            "input a : Int64
                output b(p1)
                    spawn with a
                    eval when a == p1 with true
                output c(p1)
                    spawn with a
                    eval @1Hz with b(p1).aggregate(over: 10s, using: count)
                ",
        );
        let mut eval = eval.into_evaluator();
        let b_ref = StreamReference::Out(0);
        let c_ref = StreamReference::Out(1);
        let mut tracer = NoTracer::default();

        time += Duration::from_millis(1000);
        eval.eval_event(&[Signed(1)], time, &mut tracer);
        assert!(stream_has_instance!(eval, b_ref, vec![Signed(1)]));
        assert!(stream_has_instance!(eval, c_ref, vec![Signed(1)]));
        assert_eq!(eval.peek_value(b_ref, &[Signed(1)], 0).unwrap(), Bool(true));

        time += Duration::from_millis(1000);
        eval.eval_time_driven_tasks(
            vec![EvaluationTask::Evaluate(c_ref.out_ix(), vec![Signed(1)])],
            time,
            &mut tracer,
        );
        eval.eval_event(&[Signed(1)], time, &mut tracer);
        assert_eq!(eval.peek_value(b_ref, &[Signed(1)], 0).unwrap(), Bool(true));
        assert_eq!(eval.peek_value(c_ref, &[Signed(1)], 0).unwrap(), Unsigned(1));

        time += Duration::from_millis(1000);
        eval.eval_time_driven_tasks(
            vec![EvaluationTask::Evaluate(c_ref.out_ix(), vec![Signed(1)])],
            time,
            &mut tracer,
        );
        assert_eq!(eval.peek_value(c_ref, &[Signed(1)], 0).unwrap(), Unsigned(2));

        // This test is correct and shows the inteded behavior. Yet it is confusing as inputs coincide with periods.
        /*
         The following is the order of events that take place:
         1s: a = 1 -> b spawns -> c spawns -> the windows is created -> b = true -> window = 1
         2s: c = 1 -> a = 1 -> b = true -> window = 2
         3s: c = 2
        !!! Remember that in case that an input coincides with a deadline, the deadline is evaluated first, hence the confusing order of events at time 2s
         */
    }

    // Corresponds to issue #19
    #[test]
    fn test_parameterized_window_spawn_bug() {
        let (_, eval, mut time) = setup(
            "input a : Int
                       output b spawn @a eval @0.5s with a.aggregate(over: 5s, using: count)",
        );
        let mut eval = eval.into_evaluator();
        let b_ref = StreamReference::Out(0);
        let mut tracer = NoTracer::default();

        // A receives a value and b is spawned
        // The windows over a is spawned as well and should already contain the current value of a
        time += Duration::from_millis(1000);
        eval.eval_event(&[Signed(1)], time, &mut tracer);
        assert!(stream_has_instance!(eval, b_ref, Vec::<Value>::new()));

        // b evaluates and the window contains one value. (The value produces by 'a' at time 1)
        time += Duration::from_millis(500);
        eval.eval_time_driven_tasks(
            vec![EvaluationTask::Evaluate(b_ref.out_ix(), vec![Signed(1)])],
            time,
            &mut tracer,
        );
        assert_eq!(eval.peek_value(b_ref, &[Signed(1)], 0).unwrap(), Unsigned(1));
    }

    // Corresponds to issue #20
    #[test]
    fn test_parameterized_window_global_freq() {
        let (_, eval, mut time) = setup(
            "input page_id: UInt
                    output page_id_visits(pid)
                      spawn with page_id
                      eval when pid == page_id

                    output visits_per_day(pid)
                      spawn with page_id
                      eval @Global(1h) with page_id_visits(pid).aggregate(over: 1h, using: count)",
        );
        let mut eval = eval.into_evaluator();
        let visits = StreamReference::Out(0);
        let avg = StreamReference::Out(1);
        let mut tracer = NoTracer::default();

        eval.eval_event(&[Signed(1)], time, &mut tracer);
        assert!(stream_has_instance!(eval, visits, vec![Signed(1)]));
        assert!(stream_has_instance!(eval, avg, vec![Signed(1)]));

        time += Duration::from_millis(1000);
        eval.eval_event(&[Signed(2)], time, &mut tracer);
        assert!(stream_has_instance!(eval, visits, vec![Signed(2)]));
        assert!(stream_has_instance!(eval, avg, vec![Signed(2)]));

        time += Duration::from_millis(1000);
        eval.eval_event(&[Signed(3)], time, &mut tracer);
        assert!(stream_has_instance!(eval, visits, vec![Signed(3)]));
        assert!(stream_has_instance!(eval, avg, vec![Signed(3)]));

        time += Duration::from_millis(1000);
        eval.eval_event(&[Signed(5)], time, &mut tracer);
        assert!(stream_has_instance!(eval, visits, vec![Signed(5)]));
        assert!(stream_has_instance!(eval, avg, vec![Signed(5)]));

        time += Duration::from_millis(1000);
        eval.eval_event(&[Signed(3)], time, &mut tracer);
        assert!(stream_has_instance!(eval, visits, vec![Signed(3)]));
        assert!(stream_has_instance!(eval, avg, vec![Signed(3)]));

        time += Duration::from_millis(1000);
        eval.eval_event(&[Signed(1)], time, &mut tracer);
        assert!(stream_has_instance!(eval, visits, vec![Signed(1)]));
        assert!(stream_has_instance!(eval, avg, vec![Signed(1)]));

        time = Duration::from_secs(3600);
        eval.eval_time_driven_tasks(
            vec![
                EvaluationTask::Evaluate(avg.out_ix(), vec![Signed(1)]),
                EvaluationTask::Evaluate(avg.out_ix(), vec![Signed(2)]),
                EvaluationTask::Evaluate(avg.out_ix(), vec![Signed(3)]),
                EvaluationTask::Evaluate(avg.out_ix(), vec![Signed(5)]),
            ],
            time,
            &mut tracer,
        );
        assert_eq!(eval.peek_value(avg, &[Signed(1)], 0).unwrap(), Unsigned(1));
        assert_eq!(eval.peek_value(avg, &[Signed(2)], 0).unwrap(), Unsigned(1));
        assert_eq!(eval.peek_value(avg, &[Signed(3)], 0).unwrap(), Unsigned(2));
        assert_eq!(eval.peek_value(avg, &[Signed(5)], 0).unwrap(), Unsigned(1));
    }
}
