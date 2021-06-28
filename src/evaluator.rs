use crate::basics::{EvalConfig, ExecutionMode, OutputHandler, Time};
use crate::closuregen::{CompiledExpr, Expr};
use crate::coordination::{DynamicSchedule, EvaluationTask};
use crate::storage::{GlobalStore, Value};
use bit_set::BitSet;
use rtlola_frontend::mir::{
    ActivationCondition as Activation, InputReference, OutputReference, OutputStream, PacingType, RtLolaMir, Stream,
    StreamReference, Task, Trigger, WindowReference,
};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;
use string_template::Template;

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
    global_store: GlobalStore,
    start_time: Instant,
    first_event: Option<Time>,
    fresh_inputs: BitSet,
    fresh_outputs: BitSet,
    fresh_triggers: BitSet,
    triggers: Vec<Option<Trigger>>,
    trigger_templates: Vec<Option<Template>>,
    ir: RtLolaMir,
    handler: Arc<OutputHandler>,
    config: EvalConfig,
    dyn_schedule: Arc<(Mutex<DynamicSchedule>, Condvar)>,
}

#[allow(missing_debug_implementations)]
pub(crate) struct Evaluator {
    // Evaluation order of output streams
    layers: &'static [Vec<Task>],
    // Indexed by stream reference.
    stream_activation_conditions: &'static [ActivationConditionOp],
    spawn_activation_conditions: &'static [ActivationConditionOp],
    // Accessed by stream index
    // If Value::None is returned by an expression the filter was false
    compiled_stream_exprs: Vec<CompiledExpr>,
    // Accessed by stream index
    // If Value::None is returned, the spawn condition was false.
    // If a stream has no spawn target, then an empty tuple is returned if the condition is true
    compiled_spawn_exprs: Vec<CompiledExpr>,
    // Accessed by stream index
    compiled_close_exprs: Vec<CompiledExpr>,
    global_store: &'static mut GlobalStore,
    start_time: &'static Instant,
    first_event: &'static mut Option<Time>,
    fresh_inputs: &'static mut BitSet,
    fresh_outputs: &'static mut BitSet,
    fresh_triggers: &'static mut BitSet,
    triggers: &'static [Option<Trigger>],
    trigger_templates: &'static [Option<Template>],
    ir: &'static RtLolaMir,
    handler: &'static OutputHandler,
    config: &'static EvalConfig,
    dyn_schedule: &'static (Mutex<DynamicSchedule>, Condvar),
    raw_data: *mut EvaluatorData,
}

pub(crate) struct EvaluationContext<'e> {
    ts: Time,
    pub(crate) global_store: &'e GlobalStore,
    pub(crate) fresh_inputs: &'e BitSet,
    pub(crate) fresh_outputs: &'e BitSet,
}

impl EvaluatorData {
    pub(crate) fn new(
        ir: RtLolaMir,
        config: EvalConfig,
        handler: Arc<OutputHandler>,
        start_time: Option<Instant>,
        dyn_schedule: Arc<(Mutex<DynamicSchedule>, Condvar)>,
    ) -> Self {
        // Layers of event based output streams
        let mut layers: Vec<Vec<Task>> = ir
            .get_event_driven_layers()
            .into_iter()
            .map(|layer| layer.into_iter().map(Task::Evaluate).collect())
            .collect();
        let spawned_streams: Vec<&OutputStream> = ir.outputs.iter().filter(|o| o.is_spawned()).collect();
        for ss in spawned_streams {
            let spawn_layer = ss.spawn_layer().inner();
            layers[spawn_layer].push(Task::Spawn(ss.reference.out_ix()));
        }
        handler.debug(|| format!("Evaluation layers: {:?}", layers));
        let stream_acs = ir
            .outputs
            .iter()
            .map(|o| {
                if let Some(ac) = ir.get_ac(o.reference) {
                    ActivationConditionOp::new(ac, ir.inputs.len())
                } else {
                    ActivationConditionOp::TimeDriven
                }
            })
            .collect();
        let spawn_acs = ir
            .outputs
            .iter()
            .map(|o| match &o.instance_template.spawn.pacing {
                PacingType::Periodic(_) => ActivationConditionOp::TimeDriven,
                PacingType::Event(ac) => ActivationConditionOp::new(ac, ir.inputs.len()),
                PacingType::Constant => ActivationConditionOp::True,
            })
            .collect();

        let global_store = GlobalStore::new(&ir, Time::default());
        let fresh_inputs = BitSet::with_capacity(ir.inputs.len());
        let fresh_outputs = BitSet::with_capacity(ir.outputs.len());
        let fresh_triggers = BitSet::with_capacity(ir.outputs.len()); //trigger use their outputreferences
        let mut triggers = vec![None; ir.outputs.len()];
        for t in &ir.triggers {
            triggers[t.reference.out_ix()] = Some(t.clone());
        }
        let trigger_templates = triggers.iter().map(|t| t.as_ref().map(|t| Template::new(&t.message))).collect();
        let start_time = start_time.unwrap_or_else(Instant::now);
        EvaluatorData {
            layers,
            stream_activation_conditions: stream_acs,
            spawn_activation_conditions: spawn_acs,
            global_store,
            start_time,
            first_event: None,
            fresh_inputs,
            fresh_outputs,
            fresh_triggers,
            triggers,
            trigger_templates,
            ir,
            handler,
            config,
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
            .map(|o| match o.instance_template.filter.as_ref() {
                None => o.expr.clone().compile(),
                Some(filter_exp) => CompiledExpr::create_filter(filter_exp.clone().compile(), o.expr.clone().compile()),
            })
            .collect();

        let compiled_spawn_exprs = leaked_data
            .ir
            .outputs
            .iter()
            .map(|o| match (o.instance_template.spawn.target.as_ref(), o.instance_template.spawn.condition.as_ref()) {
                (None, None) => CompiledExpr::new(|_| Value::None),
                (Some(target), None) => target.clone().compile(),
                (None, Some(condition)) => CompiledExpr::create_filter(
                    condition.clone().compile(),
                    CompiledExpr::new(|_| Value::Tuple(vec![].into_boxed_slice())),
                ),
                (Some(target), Some(condition)) => {
                    CompiledExpr::create_filter(condition.clone().compile(), target.clone().compile())
                }
            })
            .collect();

        let compiled_close_exprs = leaked_data
            .ir
            .outputs
            .iter()
            .map(|o| {
                o.instance_template.close.as_ref().map_or(CompiledExpr::new(|_| Value::None), |e| e.clone().compile())
            })
            .collect();

        Evaluator {
            layers: &leaked_data.layers,
            stream_activation_conditions: &leaked_data.stream_activation_conditions,
            spawn_activation_conditions: &leaked_data.spawn_activation_conditions,
            compiled_stream_exprs,
            compiled_spawn_exprs,
            compiled_close_exprs,
            global_store: &mut leaked_data.global_store,
            start_time: &leaked_data.start_time,
            first_event: &mut leaked_data.first_event,
            fresh_inputs: &mut leaked_data.fresh_inputs,
            fresh_outputs: &mut leaked_data.fresh_outputs,
            fresh_triggers: &mut leaked_data.fresh_triggers,
            triggers: &leaked_data.triggers,
            trigger_templates: &leaked_data.trigger_templates,
            ir: &leaked_data.ir,
            handler: &leaked_data.handler,
            config: &leaked_data.config,
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
    pub(crate) fn eval_event(&mut self, event: &[Value], ts: Time) {
        if self.first_event.is_none() {
            *self.first_event = Some(ts);
        }
        let relative_ts = self.relative_time(ts);
        self.clear_freshness();
        self.accept_inputs(event, relative_ts);
        self.eval_event_driven(relative_ts);
    }

    /// NOT for external use because the values are volatile
    pub(crate) fn peek_fresh(&self) -> Vec<(OutputReference, Value)> {
        self.fresh_outputs
            .iter()
            .map(|elem| (elem, self.peek_value(StreamReference::Out(elem), &[], 0).expect("Marked as fresh.")))
            .chain(self.fresh_triggers.iter().map(|ix| (ix, Value::Bool(true))))
            .collect()
    }

    /// NOT for external use because the values are volatile
    pub(crate) fn peek_violated_triggers(&self) -> Vec<OutputReference> {
        self.fresh_triggers.iter().collect()
    }

    /// NOT for external use because the values are volatile
    pub(crate) fn peek_inputs(&self) -> Vec<Option<Value>> {
        self.ir.inputs.iter().map(|elem| self.peek_value(elem.reference, &[], 0)).collect()
    }

    /// NOT for external use because the values are volatile
    pub(crate) fn peek_outputs(&self) -> Vec<Option<Value>> {
        self.ir.outputs.iter().map(|elem| self.peek_value(elem.reference, &[], 0)).collect()
    }

    fn relative_time(&self, ts: Time) -> Time {
        if self.is_online() {
            self.start_time.elapsed()
        } else {
            ts - self.first_event.expect("time can only be computed after receiving the first event")
        }
    }

    fn is_online(&self) -> bool {
        self.config.mode == ExecutionMode::Online
    }

    fn accept_inputs(&mut self, event: &[Value], ts: Time) {
        for (ix, v) in event.iter().enumerate() {
            match v {
                Value::None => {}
                v => self.accept_input(ix, v.clone(), ts),
            }
        }
    }

    fn accept_input(&mut self, input: InputReference, v: Value, ts: Time) {
        self.global_store.get_in_instance_mut(input).push_value(v.clone());
        self.fresh_inputs.insert(input);
        self.handler.debug(|| format!("InputStream[{}] := {:?}.", input, v.clone()));
        let extended = &self.ir.inputs[input];
        for (_sr, win) in &extended.aggregated_by {
            self.global_store.get_window_mut(*win).accept_value(v.clone(), ts)
        }
    }

    fn eval_event_driven(&mut self, ts: Time) {
        self.prepare_evaluation(ts);
        for layer in self.layers {
            self.eval_event_driven_layer(layer, ts);
        }
        //Todo: Eval all event driven close conditions
    }

    fn eval_event_driven_layer(&mut self, tasks: &[Task], ts: Time) {
        for task in tasks {
            match task {
                Task::Evaluate(idx) => self.eval_event_driven_output(*idx, ts),
                Task::Spawn(_) => unimplemented!("Spawn not yet implemented"),
                Task::Close(_) => unreachable!("Closes are not included in evaluation layer."),
            }
        }
    }

    fn eval_event_driven_output(&mut self, output: OutputReference, ts: Time) {
        if self.stream_activation_conditions[output].eval(self.fresh_inputs) {
            self.eval_stream(output, ts);
        }
    }

    pub(crate) fn eval_time_driven_tasks(&mut self, tasks: Vec<EvaluationTask>, ts: Time) {
        // Todo: handle empty vec of tasks
        let relative_ts = self.relative_time(ts);
        self.clear_freshness();
        self.prepare_evaluation(relative_ts);
        for task in tasks {
            match task {
                EvaluationTask::Evaluate(idx, _) => self.eval_stream(idx, relative_ts),
                EvaluationTask::Spawn(_) => unimplemented!("Time driven spawn not yet implemented"),
                EvaluationTask::Close(_, _) => unimplemented!("Time driven close not yet implemented"),
            }
        }
    }

    fn prepare_evaluation(&mut self, ts: Time) {
        // We need to copy the references first because updating needs exclusive access to `self`.
        let windows = &self.ir.sliding_windows;
        for win in windows {
            self.global_store.get_window_mut(win.reference).update(ts);
        }
    }

    /// Creates the current trigger message by substituting the format placeholders with he current values of the info streams.
    /// NOT for external use because the values are volatile
    pub(crate) fn format_trigger_message(&self, trigger_ref: OutputReference) -> String {
        let trigger = self.is_trigger(trigger_ref).expect("Output reference must refer to a trigger");
        let values: Vec<String> = trigger
            .info_streams
            .iter()
            .map(|sr| self.peek_value(*sr, &[], 0).map_or("None".to_string(), |v| v.to_string()))
            .collect();
        let args: Vec<&str> = values.iter().map(|s| s.as_str()).collect();
        self.trigger_templates[trigger_ref]
            .as_ref()
            .expect("Output reference must refer to a trigger")
            .render_positional(args.as_slice())
    }

    /// Return the current values of the info streams
    /// NOT for external use because the values are volatile
    pub(crate) fn peek_info_stream_values(&self, trigger_ref: OutputReference) -> Vec<Option<Value>> {
        let trigger = self.is_trigger(trigger_ref).expect("Output reference must refer to a trigger");
        trigger.info_streams.iter().map(|sr| self.peek_value(*sr, &[], 0)).collect()
    }

    fn eval_stream(&mut self, output: OutputReference, ts: Time) {
        let ix = output;
        self.handler.debug(|| format!("Evaluating stream {}: {}.", ix, self.ir.output(StreamReference::Out(ix)).name));

        let ctx = self.as_EvaluationContext(ts);
        let res = self.compiled_stream_exprs[ix].execute(&ctx);

        // Filter evaluated to false
        if let Value::None = res {
            return;
        }

        match self.is_trigger(output) {
            None => {
                // Register value in global store.
                self.global_store.get_out_instance_mut(output).push_value(res.clone()); // TODO: unsafe unwrap.
                self.fresh_outputs.insert(ix);
                self.handler.output(|| format!("OutputStream[{}] := {:?}.", ix, res.clone()));
            }

            Some(trig) => {
                // Check if we have to emit a warning.
                if let Value::Bool(true) = res {
                    let msg = self.format_trigger_message(output);
                    self.handler.trigger(|| format!("Trigger: {}", msg), trig.trigger_reference, ts);
                    self.fresh_triggers.insert(ix);
                }
            }
        }

        // Check linked streams and inform them.
        let extended = &self.ir.outputs[ix];
        for (_sr, win) in &extended.aggregated_by {
            self.global_store.get_window_mut(*win).accept_value(res.clone(), ts)
        }
    }

    fn clear_freshness(&mut self) {
        self.fresh_inputs.clear();
        self.fresh_outputs.clear();
        self.fresh_triggers.clear();
    }

    fn is_trigger(&self, ix: OutputReference) -> Option<&Trigger> {
        self.triggers[ix].as_ref()
    }

    fn peek_value(&self, sr: StreamReference, args: &[Value], offset: i16) -> Option<Value> {
        match sr {
            StreamReference::In(ix) => {
                assert!(args.is_empty());
                self.global_store.get_in_instance(ix).get_value(offset)
            }
            StreamReference::Out(ix) => {
                let inst = ix;
                self.global_store.get_out_instance(inst).get_value(offset)
            }
        }
    }

    #[allow(non_snake_case)]
    fn as_EvaluationContext(&self, ts: Time) -> EvaluationContext {
        EvaluationContext {
            ts,
            global_store: &self.global_store,
            fresh_inputs: &self.fresh_inputs,
            fresh_outputs: &self.fresh_outputs,
        }
    }
}

impl<'e> EvaluationContext<'e> {
    pub(crate) fn lookup_latest(&self, stream_ref: StreamReference) -> Value {
        let inst = match stream_ref {
            StreamReference::In(ix) => self.global_store.get_in_instance(ix),
            StreamReference::Out(ix) => self.global_store.get_out_instance(ix),
        };
        inst.get_value(0).unwrap_or(Value::None)
    }

    pub(crate) fn lookup_latest_check(&self, stream_ref: StreamReference) -> Value {
        let inst = match stream_ref {
            StreamReference::In(ix) => {
                debug_assert!(self.fresh_inputs.contains(ix), "ix={}", ix);
                self.global_store.get_in_instance(ix)
            }
            StreamReference::Out(ix) => {
                debug_assert!(self.fresh_outputs.contains(ix), "ix={}", ix);
                self.global_store.get_out_instance(ix)
            }
        };
        inst.get_value(0).unwrap_or(Value::None)
    }

    pub(crate) fn lookup_with_offset(&self, stream_ref: StreamReference, offset: i16) -> Value {
        let (inst, fresh) = match stream_ref {
            StreamReference::In(ix) => (self.global_store.get_in_instance(ix), self.fresh_inputs.contains(ix)),
            StreamReference::Out(ix) => (self.global_store.get_out_instance(ix), self.fresh_outputs.contains(ix)),
        };
        if fresh {
            inst.get_value(offset).unwrap_or(Value::None)
        } else {
            inst.get_value(offset + 1).unwrap_or(Value::None)
        }
    }

    pub(crate) fn lookup_window(&self, window_ref: WindowReference) -> Value {
        self.global_store.get_window(window_ref).get_value(self.ts)
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
                .flat_map(|ac| if let Activation::Stream(var) = ac { Some(var.in_ix()) } else { None })
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
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::storage::Value::*;
    use ordered_float::NotNan;
    use rtlola_frontend::mir::RtLolaMir;
    use rtlola_frontend::{FrontEndErr, ParserConfig};
    use std::time::{Duration, Instant};

    fn parse(spec: &str) -> Result<RtLolaMir, FrontEndErr> {
        rtlola_frontend::parse(ParserConfig::for_string(spec.to_string()))
    }

    fn setup(spec: &str) -> (RtLolaMir, EvaluatorData, Instant) {
        let ir = parse(spec).unwrap_or_else(|e| panic!("spec is invalid: {:?}", e));
        let mut config = EvalConfig::default();
        config.verbosity = crate::basics::Verbosity::WarningsOnly;
        let handler = Arc::new(OutputHandler::new(&config, ir.triggers.len()));
        let cond = Condvar::new();
        let dyn_schedule = Arc::new((Mutex::new(DynamicSchedule::new()), cond));
        let now = Instant::now();
        let eval = EvaluatorData::new(ir.clone(), config, handler, Some(now), dyn_schedule);
        (ir, eval, now)
    }

    fn setup_time(spec: &str) -> (RtLolaMir, EvaluatorData, Time) {
        let (ir, eval, _) = setup(spec);
        (ir, eval, Time::default())
    }

    macro_rules! eval_stream {
        ($eval:expr, $start:expr, $ix:expr) => {
            $eval.eval_stream($ix, $start.elapsed());
        };
    }

    macro_rules! eval_stream_timed {
        ($eval:expr, $ix:expr, $time:expr) => {
            $eval.eval_stream($ix, $time);
        };
    }

    macro_rules! accept_input {
        ($eval:expr, $start:expr, $str_ref:expr, $v:expr) => {
            $eval.accept_input($str_ref.in_ix(), $v.clone(), $start.elapsed());
        };
    }

    macro_rules! accept_input_timed {
        ($eval:expr, $str_ref:expr, $v:expr, $time:expr) => {
            $eval.accept_input($str_ref.in_ix(), $v.clone(), $time);
        };
    }

    macro_rules! peek_assert_eq {
        ($eval:expr, $start:expr, $ix:expr, $value:expr) => {
            eval_stream!($eval, $start, $ix);
            assert_eq!($eval.peek_value(StreamReference::Out($ix), &Vec::new(), 0).unwrap(), $value);
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
        accept_input!(eval, start, sr, v.clone());
        peek_assert_eq!(eval, start, 0, Bool(true));
        peek_assert_eq!(eval, start, 1, Unsigned(3));
        peek_assert_eq!(eval, start, 2, Signed(-5));
        peek_assert_eq!(eval, start, 3, Value::new_float(-123.456));
        peek_assert_eq!(eval, start, 4, Str("foobar".into()));
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
        accept_input!(eval, start, sr, v.clone());
        peek_assert_eq!(eval, start, 0, Bool(!false));
        peek_assert_eq!(eval, start, 1, Bool(!true));
        peek_assert_eq!(eval, start, 2, Unsigned(8 + 3));
        peek_assert_eq!(eval, start, 3, Unsigned(8 - 3));
        peek_assert_eq!(eval, start, 4, Unsigned(8 * 3));
        peek_assert_eq!(eval, start, 5, Unsigned(8 / 3));
        peek_assert_eq!(eval, start, 6, Unsigned(8 % 3));
        peek_assert_eq!(eval, start, 7, Unsigned(8 * 8 * 8));
        peek_assert_eq!(eval, start, 8, Bool(false || false));
        peek_assert_eq!(eval, start, 9, Bool(false || true));
        peek_assert_eq!(eval, start, 10, Bool(true || false));
        peek_assert_eq!(eval, start, 11, Bool(true || true));
        peek_assert_eq!(eval, start, 12, Bool(false && false));
        peek_assert_eq!(eval, start, 13, Bool(false && true));
        peek_assert_eq!(eval, start, 14, Bool(true && false));
        peek_assert_eq!(eval, start, 15, Bool(true && true));
        peek_assert_eq!(eval, start, 16, Bool(0 < 1));
        peek_assert_eq!(eval, start, 17, Bool(0 < 0));
        peek_assert_eq!(eval, start, 18, Bool(1 < 0));
        peek_assert_eq!(eval, start, 19, Bool(0 <= 1));
        peek_assert_eq!(eval, start, 20, Bool(0 <= 0));
        peek_assert_eq!(eval, start, 21, Bool(1 <= 0));
        peek_assert_eq!(eval, start, 22, Bool(0 >= 1));
        peek_assert_eq!(eval, start, 23, Bool(0 >= 0));
        peek_assert_eq!(eval, start, 24, Bool(1 >= 0));
        peek_assert_eq!(eval, start, 25, Bool(0 > 1));
        peek_assert_eq!(eval, start, 26, Bool(0 > 0));
        peek_assert_eq!(eval, start, 27, Bool(1 > 0));
        peek_assert_eq!(eval, start, 28, Bool(0 == 0));
        peek_assert_eq!(eval, start, 29, Bool(0 == 1));
        peek_assert_eq!(eval, start, 30, Bool(0 != 0));
        peek_assert_eq!(eval, start, 31, Bool(0 != 1));
    }

    #[test]
    fn test_input_only() {
        let (_, eval, start) = setup("input a: UInt8");
        let mut eval = eval.into_evaluator();
        let sr = StreamReference::In(0);
        let v = Unsigned(3);
        accept_input!(eval, start, sr, v.clone());
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
        accept_input!(eval, start, in_ref, v.clone());
        eval_stream!(eval, start, 0);
        eval_stream!(eval, start, 1);
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
        eval_stream!(eval, start, 0);
        eval_stream!(eval, start, 1);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), Unsigned(3));
    }

    #[test]
    fn test_output_lookup() {
        let (_, eval, start) = setup(
            "input a: UInt8\noutput mirror: UInt8 := a\noutput mirror_offset := mirror.offset(by: -1).defaults(to: 5)\noutput c: UInt8 @5Hz := mirror.hold().defaults(to: 8)\noutput d: UInt8 @5Hz := mirror_offset.hold().defaults(to: 3)",
        );
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(2);
        let in_ref = StreamReference::In(0);
        let v1 = Unsigned(1);
        let v2 = Unsigned(2);
        accept_input!(eval, start, in_ref, v1.clone());
        eval_stream!(eval, start, 0);
        eval_stream!(eval, start, 1);
        accept_input!(eval, start, in_ref, v2);
        eval_stream!(eval, start, 0);
        eval_stream!(eval, start, 1);
        eval_stream!(eval, start, 2);
        assert_eq!(eval.peek_value(StreamReference::Out(0), &Vec::new(), 0).unwrap(), v2);
        assert_eq!(eval.peek_value(StreamReference::Out(1), &Vec::new(), 0).unwrap(), v1);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), v2);
    }

    #[test]
    fn test_conversion_if() {
        let (_, eval, start) =
            setup("input a: UInt8\noutput b: UInt16 := widen<UInt16>(if true then a else a[-1].defaults(to: 0))");
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let in_ref = StreamReference::In(0);
        let v1 = Unsigned(1);
        accept_input!(eval, start, in_ref, v1.clone());
        eval_stream!(eval, start, 0);
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
        eval_stream!(eval, start, 0);
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
        accept_input!(eval, start, a, v1.clone());
        accept_input!(eval, start, b, v2.clone());
        eval_stream!(eval, start, 0);
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
        accept_input!(eval, start, a, v1.clone());
        accept_input!(eval, start, b, v2.clone());
        eval_stream!(eval, start, 0);
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
        accept_input!(eval, start, a, v1.clone());
        accept_input!(eval, start, b, v2.clone());
        eval_stream!(eval, start, 0);
        eval_stream!(eval, start, 1);
        eval_stream!(eval, start, 2);
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
        accept_input!(eval, start, in_ref, v2.clone());
        accept_input!(eval, start, in_ref, v3);
        eval_stream!(eval, start, 0);
        eval_stream!(eval, start, 1);
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
        eval_stream!(eval, start, 0);
        eval_stream!(eval, start, 1);
        eval_stream!(eval, start, 2);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), Unsigned(3));
        assert_eq!(eval.peek_value(trig_ref, &Vec::new(), 0).unwrap(), Bool(false));
        accept_input!(eval, start, in_ref, v1.clone());
        eval_stream!(eval, start, 0);
        eval_stream!(eval, start, 1);
        eval_stream!(eval, start, 2);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), Unsigned(3));
        assert_eq!(eval.peek_value(trig_ref, &Vec::new(), 0).unwrap(), Bool(false));
        accept_input!(eval, start, in_ref, Unsigned(17));
        eval_stream!(eval, start, 0);
        eval_stream!(eval, start, 1);
        eval_stream!(eval, start, 2);
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
        eval_stream_timed!(eval, 0, time);
        let expected = Signed((n * n + n) / 2);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_count_window() {
        let (_, eval, mut time) =
            setup_time("input a: UInt16\noutput b: UInt16 @0.25Hz := a.aggregate(over: 40s, using: #)");
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
        eval_stream_timed!(eval, 0, time);
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
        eval_stream_timed!(eval, 0, time);
        let expected = Value::new_float(-3.0);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);

        let n = 25;
        for v in 1..=n {
            accept_input_timed!(eval, in_ref, Value::new_float(v as f64), time);
            time += Duration::from_secs(1);
        }
        time += Duration::from_secs(1);

        // 71 secs have passed. All values should be within the window.
        eval_stream_timed!(eval, 0, time);
        let n = n as f64;
        let expected = Value::new_float(((n * n + n) / 2.0) / 25.0);
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

        eval_stream_timed!(eval, 0, time);

        let expected = Float(NotNan::new(-106.5).unwrap());
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_window_type_count() {
        let (_, eval, start) = setup("input a: Int32\noutput b @ 10Hz := a.aggregate(over: 0.1s, using: count)");
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let _a = StreamReference::In(0);
        let expected = Unsigned(0);
        eval_stream!(eval, start, 0);
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_sum_window_discrete() {
        let (_, eval, mut time) =
            setup_time("input a: Int16\noutput b: Int16 @1Hz:= a.aggregate(over_discrete: 6, using: sum)");
        let mut eval: Evaluator = eval.into_evaluator();
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
        eval_stream_timed!(eval, 0, time);
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
        let mut eval: Evaluator = eval.into_evaluator();
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
        eval_stream_timed!(eval, 0, time);
        let expected = Float(NotNan::new(25.0).unwrap());
        assert_eq!(eval.peek_value(in_ref, &Vec::new(), 0).unwrap(), Float(NotNan::new(25.0).unwrap()));
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_last_window_signed() {
        let (_, eval, mut time) =
            setup_time("input a: Int32\noutput b: Int32 @1Hz:= a.aggregate(over: 20s, using: last).defaults(to:0)");
        let mut eval: Evaluator = eval.into_evaluator();
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
        eval_stream_timed!(eval, 0, time);
        let expected = Signed(25);
        assert_eq!(eval.peek_value(in_ref, &Vec::new(), 0).unwrap(), Signed(25));
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_last_window_unsigned() {
        let (_, eval, mut time) =
            setup_time("input a: UInt32\noutput b: UInt32 @1Hz:= a.aggregate(over: 20s, using: last).defaults(to:0)");
        let mut eval: Evaluator = eval.into_evaluator();
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
        eval_stream_timed!(eval, 0, time);
        let expected = Unsigned(25);
        assert_eq!(eval.peek_value(in_ref, &Vec::new(), 0).unwrap(), Unsigned(25));
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_percentile_float() {
        for (pctl, exp) in &[
            ("pctl25", Value::new_float(13.0)),
            ("pctl75", Value::new_float(18.0)),
            ("pctl10", Value::new_float(11.5)),
            ("pctl5", Value::new_float(11.0)),
            ("pctl90", Value::new_float(19.5)),
            ("med", Value::new_float(15.5)),
        ] {
            let (_, eval, mut time) = setup_time(&format!(
                "input a: Float32\noutput b: Float32 @1Hz:= a.aggregate(over: 10s, using: {}).defaults(to:0.0)",
                pctl
            ));
            let mut eval: Evaluator = eval.into_evaluator();
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
            eval_stream_timed!(eval, 0, time);
            assert_eq!(eval.peek_value(in_ref, &Vec::new(), 0).unwrap(), Float(NotNan::new(20.0).unwrap()));
            assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), exp.clone());
        }
    }

    #[test]
    fn test_percentile_float_unordered_input() {
        for (pctl, exp) in &[
            ("pctl25", Value::new_float(13.0)),
            ("pctl75", Value::new_float(18.0)),
            ("pctl10", Value::new_float(11.5)),
            ("pctl5", Value::new_float(11.0)),
            ("pctl90", Value::new_float(19.5)),
            ("med", Value::new_float(15.5)),
        ] {
            let (_, eval, mut time) = setup_time(&format!(
                "input a: Float32\noutput b: Float32 @1Hz:= a.aggregate(over: 10s, using: {}).defaults(to:0.0)",
                pctl
            ));
            let mut eval: Evaluator = eval.into_evaluator();
            time += Duration::from_secs(45);
            let out_ref = StreamReference::Out(0);
            let in_ref = StreamReference::In(0);
            let n = 20;
            let input_val = [1, 9, 8, 5, 4, 3, 7, 2, 10, 6, 20, 11, 19, 12, 18, 13, 17, 14, 16, 15];
            for v in 0..n {
                accept_input_timed!(eval, in_ref, Float(NotNan::new(input_val[v] as f64).unwrap()), time);
                time += Duration::from_secs(1);
            }
            time += Duration::from_secs(1);
            // 66 secs have passed. All values should be within the window.
            eval_stream_timed!(eval, 0, time);
            assert_eq!(eval.peek_value(in_ref, &Vec::new(), 0).unwrap(), Float(NotNan::new(15.0).unwrap()));
            assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), exp.clone());
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
            let mut eval: Evaluator = eval.into_evaluator();
            time += Duration::from_secs(45);
            let out_ref = StreamReference::Out(0);
            let in_ref = StreamReference::In(0);
            let n = 20;
            for v in 1..=n {
                accept_input_timed!(eval, in_ref, Signed(v), time);
                time += Duration::from_secs(1);
            }
            time += Duration::from_secs(1);
            // 66 secs have passed. All values should be within the window.
            eval_stream_timed!(eval, 0, time);
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
            let mut eval: Evaluator = eval.into_evaluator();
            time += Duration::from_secs(45);
            let out_ref = StreamReference::Out(0);
            let in_ref = StreamReference::In(0);
            let n = 20;
            for v in 1..=n {
                accept_input_timed!(eval, in_ref, Unsigned(v), time);
                time += Duration::from_secs(1);
            }
            time += Duration::from_secs(1);
            // 66 secs have passed. All values should be within the window.
            eval_stream_timed!(eval, 0, time);
            assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), exp.clone());
        }
    }

    #[test]
    fn test_percentile_discrete_float_unordered_input() {
        for (pctl, exp) in &[
            ("pctl25", Value::new_float(13.0)),
            ("pctl75", Value::new_float(18.0)),
            ("pctl10", Value::new_float(11.5)),
            ("pctl5", Value::new_float(11.0)),
            ("pctl90", Value::new_float(19.5)),
            ("med", Value::new_float(15.5)),
        ] {
            let (_, eval, mut time) = setup_time(&format!(
                "input a: Float32\noutput b: Float32 @1Hz:= a.aggregate(over_discrete: 10, using: {}).defaults(to:0.0)",
                pctl
            ));
            let mut eval: Evaluator = eval.into_evaluator();
            time += Duration::from_secs(45);
            let out_ref = StreamReference::Out(0);
            let in_ref = StreamReference::In(0);
            let n = 20;
            let input_val = [1, 9, 8, 5, 4, 3, 7, 2, 10, 6, 20, 11, 19, 12, 18, 13, 17, 14, 16, 15];
            for v in 0..n {
                accept_input_timed!(eval, in_ref, Float(NotNan::new(input_val[v] as f64).unwrap()), time);
                time += Duration::from_secs(1);
            }
            time += Duration::from_secs(1);
            // 66 secs have passed. All values should be within the window.
            eval_stream_timed!(eval, 0, time);
            assert_eq!(eval.peek_value(in_ref, &Vec::new(), 0).unwrap(), Float(NotNan::new(15.0).unwrap()));
            assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), exp.clone());
        }
    }

    #[test]
    fn test_var_equal_input() {
        let (_, eval, mut time) =
            setup_time("input a: Float32\noutput b: Float32 @1Hz:= a.aggregate(over: 5s, using: var).defaults(to:0.0)");
        let mut eval: Evaluator = eval.into_evaluator();
        time += Duration::from_secs(45);
        let out_ref = StreamReference::Out(0);
        let in_ref = StreamReference::In(0);
        let n = 25;
        for _ in 1..=n {
            accept_input_timed!(eval, in_ref, Float(NotNan::new(10 as f64).unwrap()), time);
            time += Duration::from_secs(1);
        }
        time += Duration::from_secs(1);
        // 71 secs have passed. All values should be within the window.
        eval_stream_timed!(eval, 0, time);
        let expected = Float(NotNan::new(0.0).unwrap());
        assert_eq!(eval.peek_value(in_ref, &Vec::new(), 0).unwrap(), Float(NotNan::new(10.0).unwrap()));
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_var_window() {
        for (duration, exp) in &[
            ("2", Value::new_float(0.25)),
            ("3", Value::new_float(2.0 / 3.0)),
            ("4", Value::new_float(1.25)),
            ("5", Value::new_float(2.0)),
            ("6", Value::new_float(17.5 / 6.0)),
            ("7", Value::new_float(4.0)),
            ("8", Value::new_float(5.25)),
            ("9", Value::new_float(60.0 / 9.0)),
            ("10", Value::new_float(8.25)),
            ("11", Value::new_float(10.0)),
        ] {
            let (_, eval, mut time) = setup_time(&format!(
                "input a: Float32\noutput b: Float32 @1Hz:= a.aggregate(over: {}s, using: var).defaults(to:0.0)",
                duration
            ));
            let mut eval: Evaluator = eval.into_evaluator();
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
            eval_stream_timed!(eval, 0, time);
            assert_eq!(eval.peek_value(in_ref, &Vec::new(), 0).unwrap(), Float(NotNan::new(20.0).unwrap()));
            assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), exp.clone());
        }
    }

    #[test]
    fn test_sd_window() {
        for (duration, exp) in &[
            ("2", Value::new_float(0.25f64.sqrt())),
            ("3", Value::new_float((2.0 / 3.0f64).sqrt())),
            ("4", Value::new_float(1.25f64.sqrt())),
            ("5", Value::new_float(2.0f64.sqrt())),
            ("6", Value::new_float((17.5 / 6.0f64).sqrt())),
            ("7", Value::new_float(4.0f64.sqrt())),
            ("8", Value::new_float(5.25f64.sqrt())),
            ("9", Value::new_float((60.0 / 9.0f64).sqrt())),
            ("10", Value::new_float(8.25f64.sqrt())),
            ("11", Value::new_float(10.0f64.sqrt())),
        ] {
            let (_, eval, mut time) = setup_time(&format!(
                "input a: Float32\noutput b: Float32 @1Hz:= a.aggregate(over: {}s, using: sd).defaults(to:0.0)",
                duration
            ));
            let mut eval: Evaluator = eval.into_evaluator();
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
            eval_stream_timed!(eval, 0, time);
            assert_eq!(eval.peek_value(in_ref, &Vec::new(), 0).unwrap(), Float(NotNan::new(20.0).unwrap()));
            assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), exp.clone());
        }
    }

    #[test]
    fn test_cov() {
        let (_, eval, mut time) =
            setup_time("input in: Float32\n input in2: Float32\noutput t@in&in2:= (in,in2)\n output out: Float32 @1Hz := t.aggregate(over: 6s, using: cov).defaults(to: 1337.0)");
        let mut eval: Evaluator = eval.into_evaluator();
        time += Duration::from_secs(45);
        let out_ref = StreamReference::Out(1);
        let in_ref = StreamReference::In(0);
        let in_ref_2 = StreamReference::In(1);
        let n = 20;
        for v in 1..=n {
            accept_input_timed!(eval, in_ref, Value::new_float(v as f64), time);
            accept_input_timed!(eval, in_ref_2, Value::new_float(v as f64), time);
            eval_stream_timed!(eval, 0, time);
            time += Duration::from_secs(1);
        }
        time += Duration::from_secs(1);
        // 66 secs have passed. All values should be within the window.
        eval_stream_timed!(eval, 1, time);
        let expected = Float(NotNan::new(17.5 / 6.0).unwrap());
        assert_eq!(eval.peek_value(in_ref, &Vec::new(), 0).unwrap(), Float(NotNan::new(20.0).unwrap()));
        assert_eq!(eval.peek_value(in_ref_2, &Vec::new(), 0).unwrap(), Float(NotNan::new(20.0).unwrap()));
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_cov_2() {
        let (_, eval, mut time) =
            setup_time("input in: Float32\n input in2: Float32\noutput t@in&in2:= (in,in2)\n output out: Float32 @1Hz := t.aggregate(over: 5s, using: cov).defaults(to: 1337.0)");
        let mut eval: Evaluator = eval.into_evaluator();
        time += Duration::from_secs(45);
        let out_ref = StreamReference::Out(1);
        let in_ref = StreamReference::In(0);
        let in_ref_2 = StreamReference::In(1);
        let n = 20;
        for v in 1..=n {
            accept_input_timed!(eval, in_ref, Value::new_float(v as f64), time);
            accept_input_timed!(eval, in_ref_2, Value::new_float(16.0), time);
            eval_stream_timed!(eval, 0, time);
            time += Duration::from_secs(1);
        }
        time += Duration::from_secs(1);
        // 66 secs have passed. All values should be within the window.
        eval_stream_timed!(eval, 1, time);
        let expected = Float(NotNan::new(0.0).unwrap());
        assert_eq!(eval.peek_value(in_ref, &Vec::new(), 0).unwrap(), Float(NotNan::new(20.0).unwrap()));
        assert_eq!(eval.peek_value(in_ref_2, &Vec::new(), 0).unwrap(), Float(NotNan::new(16.0).unwrap()));
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

    #[test]
    fn test_var_discrete() {
        for (duration, exp) in &[
            ("2", Value::new_float(0.25)),
            ("3", Value::new_float(2.0 / 3.0)),
            ("4", Value::new_float(1.25)),
            ("5", Value::new_float(2.0)),
            ("6", Value::new_float(17.5 / 6.0)),
            ("7", Value::new_float(4.0)),
            ("8", Value::new_float(5.25)),
            ("9", Value::new_float(60.0 / 9.0)),
            ("10", Value::new_float(8.25)),
            ("11", Value::new_float(10.0)),
        ] {
            let (_, eval, mut time) = setup_time(&format!(
                "input a: Float32\noutput b: Float32 @1Hz:= a.aggregate(over_discrete: {}, using: var).defaults(to:0.0)",
                duration
            ));
            let mut eval: Evaluator = eval.into_evaluator();
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
            eval_stream_timed!(eval, 0, time);
            assert_eq!(eval.peek_value(in_ref, &Vec::new(), 0).unwrap(), Float(NotNan::new(20.0).unwrap()));
            assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), exp.clone());
        }
    }

    #[test]
    fn test_sd_discrete() {
        for (duration, exp) in &[
            ("2", Value::new_float(0.25f64.sqrt())),
            ("3", Value::new_float((2.0 / 3.0f64).sqrt())),
            ("4", Value::new_float(1.25f64.sqrt())),
            ("5", Value::new_float(2.0f64.sqrt())),
            ("6", Value::new_float((17.5 / 6.0f64).sqrt())),
            ("7", Value::new_float(4.0f64.sqrt())),
            ("8", Value::new_float(5.25f64.sqrt())),
            ("9", Value::new_float((60.0 / 9.0f64).sqrt())),
            ("10", Value::new_float(8.25f64.sqrt())),
            ("11", Value::new_float(10.0f64.sqrt())),
        ] {
            let (_, eval, mut time) = setup_time(&format!(
                "input a: Float32\noutput b: Float32 @1Hz:= a.aggregate(over_discrete: {}, using: sd).defaults(to:0.0)",
                duration
            ));
            let mut eval: Evaluator = eval.into_evaluator();
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
            eval_stream_timed!(eval, 0, time);
            assert_eq!(eval.peek_value(in_ref, &Vec::new(), 0).unwrap(), Float(NotNan::new(20.0).unwrap()));
            assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), exp.clone());
        }
    }

    #[test]
    fn test_cases_window_discrete_float() {
        for (aggr, exp, default) in &[
            ("sum", Value::new_float(115.0), false),
            ("min", Value::new_float(21.0), true),
            ("max", Value::new_float(25.0), true),
            ("avg", Value::new_float(23.0), true),
            ("integral", Value::new_float(92.0), false),
            ("last", Value::new_float(25.0), true),
            ("med", Value::new_float(23.0), true),
            ("pctl20", Value::new_float(21.5), true),
        ] {
            let mut spec = String::from("input a: Float32\noutput b @0.5Hz:= a.aggregate(over_discrete: 5, using: ");
            spec += aggr;
            spec += ")";
            if *default {
                spec += ".defaults(to:1337.0)"
            }
            let (_, eval, mut time) = setup_time(&spec);
            let mut eval: Evaluator = eval.into_evaluator();
            time += Duration::from_secs(45);
            let out_ref = StreamReference::Out(0);
            let in_ref = StreamReference::In(0);
            let n = 25;
            for v in 1..=n {
                accept_input_timed!(eval, in_ref, Value::new_float(v as f64), time);
                time += Duration::from_secs(1);
            }
            time += Duration::from_secs(1);
            // 71 secs have passed. All values should be within the window.
            eval_stream_timed!(eval, 0, time);
            let expected = exp.clone();
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
            ("integral", Value::new_float(92.0), false),
            ("last", Signed(25), true),
            ("med", Signed(23), true),
            ("pctl20", Signed(21), true),
        ] {
            let mut spec = String::from("input a: Int16\noutput b @0.5Hz:= a.aggregate(over_discrete: 5, using: ");
            spec += aggr;
            spec += ")";
            if *default {
                spec += ".defaults(to:1337)"
            }
            let (_, eval, mut time) = setup_time(&spec);
            let mut eval: Evaluator = eval.into_evaluator();
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
            eval_stream_timed!(eval, 0, time);
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
            ("integral", Value::new_float(92.0), false),
            ("last", Unsigned(25), true),
            ("med", Unsigned(23), true),
            ("pctl20", Unsigned(21), true),
        ] {
            let mut spec = String::from("input a: UInt16\noutput b @0.5Hz:= a.aggregate(over_discrete: 5, using: ");
            spec += aggr;
            spec += ")";
            if *default {
                spec += ".defaults(to:1337)"
            }
            let (_, eval, mut time) = setup_time(&spec);
            let mut eval: Evaluator = eval.into_evaluator();
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
            eval_stream_timed!(eval, 0, time);
            let expected = exp.clone();
            assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
        }
    }

    #[test]
    fn test_filter() {
        let (_, eval, start) = setup(
            "input a: Int32\n\
                   output b filter a == 42 := a + 8",
        );
        let mut eval = eval.into_evaluator();
        let out_ref = StreamReference::Out(0);
        let in_ref = StreamReference::In(0);
        accept_input!(eval, start, in_ref, Signed(15));
        eval_stream!(eval, start, out_ref.out_ix());
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0), Option::None);

        accept_input!(eval, start, in_ref, Signed(42));
        eval_stream!(eval, start, out_ref.out_ix());
        assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), Signed(50));
    }
}
