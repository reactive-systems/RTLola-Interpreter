use std::cmp::Reverse;
use std::collections::{HashMap, HashSet, VecDeque};

use bimap::BiHashMap;
use either::Either;
use priority_queue::PriorityQueue;
use rtlola_frontend::mir::{
    DiscreteWindow as MirDiscreteWindow, InputReference, MemorizationBound, Origin, OutputReference, OutputStream,
    RtLolaMir, SlidingWindow as MirSlidingWindow, Stream, StreamAccessKind, StreamReference, Type, WindowReference,
};

use super::{InstanceAggregationTrait, Value};
use crate::storage::instance_aggregations::InstanceAggregation;
use crate::storage::SlidingWindow;
use crate::Time;

/// The collection of all instances of a parameterized stream
pub(crate) struct InstanceCollection {
    /// The instances accessed by parameter values
    instances: HashMap<Vec<Value>, InstanceStore>,
    /// A set of instances that got a new value in the last evaluation cycle
    fresh: HashSet<Vec<Value>>,
    /// A set of instances that spawned in the last evaluation cycle
    spawned: HashSet<Vec<Value>>,
    /// A set of instances that closed in the last evaluation cycle
    closed: HashSet<Vec<Value>>,
    /// The value type of data that should be stored
    value_type: Type,
    /// The memorization bound of the instance
    bound: MemorizationBound,
}

impl InstanceCollection {
    /// Creates a new instance
    pub(crate) fn new(ty: &Type, bound: MemorizationBound) -> Self {
        InstanceCollection {
            instances: HashMap::new(),
            fresh: HashSet::new(),
            spawned: HashSet::new(),
            closed: HashSet::new(),
            value_type: ty.clone(),
            bound,
        }
    }

    /// Returns a reference to the instance store of the instance corresponding to the parameters
    pub(crate) fn instance(&self, parameter: &[Value]) -> Option<&InstanceStore> {
        self.instances.get(parameter)
    }

    /// Returns a mutable reference to the instance store of the instance corresponding to the parameters
    pub(crate) fn instance_mut(&mut self, parameter: &[Value]) -> Option<&mut InstanceStore> {
        self.fresh.insert(parameter.to_vec());
        self.instances.get_mut(parameter)
    }

    /// Creates a new instance if not existing and returns a reference to the *new* instance if created
    pub(crate) fn create_instance(&mut self, parameter: &[Value]) -> Option<&InstanceStore> {
        if !self.instances.contains_key(parameter) {
            self.spawned.insert(parameter.to_vec());
            self.instances.insert(
                parameter.to_vec(),
                InstanceStore::new(&self.value_type, self.bound, true),
            );
            self.instances.get(parameter)
        } else {
            None
        }
    }

    /// Deletes the instance corresponding to the parameters
    pub(crate) fn mark_for_deletion(&mut self, parameter: &[Value]) {
        debug_assert!(self.instances.contains_key(parameter));
        self.closed.insert(parameter.to_vec());
    }

    /// Deletes all instances marked for deletion, returning their last value.
    pub(crate) fn delete_instances(&mut self) -> Vec<Value> {
        self.closed
            .iter()
            .filter_map(|paras| self.instances.remove(paras).and_then(|inst| inst.get_value(0)))
            .collect()
    }

    /// Returns a vector of all parameters for which an instance exists
    pub(crate) fn all_parameter(&self) -> impl Iterator<Item = &Vec<Value>> {
        self.instances.keys()
    }

    pub(crate) fn instances(&self) -> impl Iterator<Item = &InstanceStore> {
        self.instances.values()
    }

    /// Returns an iterator over all instances that got a new value
    pub(crate) fn fresh(&self) -> impl Iterator<Item = &Vec<Value>> {
        self.fresh.iter()
    }

    /// Returns a bool representing whether the instance identified by the parameter got a fresh value.
    pub(crate) fn is_fresh(&self, parameter: &[Value]) -> bool {
        self.fresh.contains(parameter)
    }

    /// Returns an iterator over newly created instances
    pub(crate) fn spawned(&self) -> impl Iterator<Item = &Vec<Value>> {
        self.spawned.iter()
    }

    /// Returns an iterator over closed instances
    pub(crate) fn closed(&self) -> impl Iterator<Item = &Vec<Value>> {
        self.closed.iter()
    }

    /// Marks all instances as not fresh
    /// Clears spawned and closed instances
    pub(crate) fn new_cycle(&mut self) {
        self.fresh.clear();
        self.spawned.clear();
        self.closed.clear();
    }

    /// Returns true if the instance exists in the instance store
    pub(crate) fn contains(&self, parameter: &[Value]) -> bool {
        self.instances.contains_key(parameter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Represents the parameterization of the window
pub(crate) enum WindowParameterization {
    /// The window is not parameterized. Yet it might be spawned.
    None {
        /// whether the window is spawned nonetheless
        is_spawned: bool,
    },
    /// The caller of the window is parameterized.
    /// An instance of the window has to be created whenever a new instance of the caller spawns, as these windows could be on different timelines.
    Caller,
    /// The target of the window is parameterized.
    /// An instance of the windows has the be created whenever a new instance of the target spawns, as we can only determine at runtime over which instance exactly the caller aggregates.
    Target {
        /// whether the window is spawned nonetheless
        is_spawned: bool,
    },
    /// Both the target and caller of the window are parameterized.
    /// An instance has the be created for all instances of the caller and the target.
    Both,
}

impl WindowParameterization {
    pub(crate) fn is_spawned(&self) -> bool {
        match self {
            WindowParameterization::None { is_spawned } | WindowParameterization::Target { is_spawned } => *is_spawned,
            WindowParameterization::Caller | WindowParameterization::Both => true,
        }
    }
}

/// The collection of all sliding windows of a parameterized stream
pub(crate) struct SlidingWindowCollection {
    windows: HashMap<Vec<Value>, SlidingWindow>,
    mir_window: Either<MirSlidingWindow, MirDiscreteWindow>,
    default_window: SlidingWindow,
    to_be_closed: PriorityQueue<Vec<Value>, Reverse<Time>>,
}

impl SlidingWindowCollection {
    /// Creates a new Collection for real-time sliding windows
    pub(crate) fn new_for_sliding(window: &MirSlidingWindow) -> Self {
        SlidingWindowCollection {
            windows: HashMap::new(),
            mir_window: Either::Left(window.clone()),
            default_window: SlidingWindow::from_sliding(Time::default(), window, false),
            to_be_closed: PriorityQueue::new(),
        }
    }

    /// Creates a new Collection for discrete sliding windows
    pub(crate) fn new_for_discrete(window: &MirDiscreteWindow) -> Self {
        SlidingWindowCollection {
            windows: HashMap::new(),
            mir_window: Either::Right(window.clone()),
            default_window: SlidingWindow::from_discrete(
                window.duration,
                window.wait,
                window.op,
                Time::default(),
                &window.ty,
                false,
            ),
            to_be_closed: PriorityQueue::new(),
        }
    }

    pub(crate) fn default_value(&self, ts: Time) -> Value {
        self.default_window.get_value(ts)
    }

    /// Creates a new sliding window in the collection
    pub(crate) fn create_window(&mut self, parameters: &[Value], start_time: Time) -> Option<&mut SlidingWindow> {
        // remove from deletion list if it still exists
        self.to_be_closed.remove(parameters);
        if !self.windows.contains_key(parameters) {
            let window = match &self.mir_window {
                Either::Left(w) => SlidingWindow::from_sliding(start_time, w, false),
                Either::Right(w) => SlidingWindow::from_discrete(w.duration, w.wait, w.op, start_time, &w.ty, false),
            };
            self.windows.insert(parameters.to_vec(), window);
            self.windows.get_mut(parameters)
        } else {
            None
        }
    }

    /// Returns the existing sliding window for the parameters or creates an instance if not existing
    pub(crate) fn get_or_create(&mut self, parameters: &[Value], ts: Time) -> &mut SlidingWindow {
        self.delete_expired_windows(ts);
        if self.windows.contains_key(parameters) {
            self.window_mut(parameters, ts).unwrap()
        } else {
            self.create_window(parameters, ts).unwrap()
        }
    }

    /// Returns a reference to the sliding window corresponding to the parameters
    pub(crate) fn window(&mut self, parameter: &[Value], ts: Time) -> Option<&SlidingWindow> {
        self.delete_expired_windows(ts);
        self.windows.get(parameter)
    }

    /// Returns a mutable reference to the sliding window corresponding to the parameters
    pub(crate) fn window_mut(&mut self, parameter: &[Value], ts: Time) -> Option<&mut SlidingWindow> {
        self.delete_expired_windows(ts);
        self.windows.get_mut(parameter)
    }

    /// Schedules the window for deletion after their period expires.
    pub(crate) fn schedule_deletion(&mut self, parameter: &[Value], ts: Time) {
        debug_assert!(self.windows.contains_key(parameter));
        match &self.mir_window {
            Either::Left(sw) => {
                self.to_be_closed.push(parameter.to_vec(), Reverse(ts + sw.duration));
            },
            Either::Right(_) => {
                // Todo reevaluate this
                self.windows.remove(parameter);
            },
        }
    }

    fn delete_expired_windows(&mut self, ts: Time) {
        while self.to_be_closed.peek().map(|(_, due)| ts > due.0).unwrap_or(false) {
            let (params, _) = self.to_be_closed.pop().unwrap();
            self.windows.remove(params.as_slice());
        }
    }

    /// Deletes the sliding window corresponding to the parameters
    pub(crate) fn delete_window(&mut self, parameter: &[Value]) {
        debug_assert!(self.windows.contains_key(parameter));
        self.windows.remove(parameter);
    }

    /// Updates all windows in the collection with the given time
    pub(crate) fn update_all(&mut self, ts: Time) {
        self.delete_expired_windows(ts);
        self.windows.iter_mut().for_each(|(_, w)| {
            if w.is_active() {
                w.update(ts)
            }
        });
    }

    /// Activates all windows in the collection with the given time
    pub(crate) fn activate_all(&mut self, ts: Time) {
        self.delete_expired_windows(ts);
        self.windows.iter_mut().for_each(|(_, w)| {
            if !w.is_active() {
                w.activate(ts);
            }
        });
    }

    /// Deactivates all windows in the collection with the given time
    pub(crate) fn deactivate_all(&mut self, ts: Time) {
        self.delete_expired_windows(ts);
        self.windows.iter_mut().for_each(|(_, w)| {
            if w.is_active() {
                w.deactivate();
            }
        });
    }

    /// Adds a new value to all windows in the collection with the given time
    pub(crate) fn accept_value_all(&mut self, value: Value, ts: Time) {
        self.delete_expired_windows(ts);
        self.windows
            .iter_mut()
            .for_each(|(_, w)| w.accept_value(value.clone(), ts));
    }
}

/// The collection of all sliding windows of a parameterized stream
pub(crate) struct TwoLayerSlidingWindowCollection {
    /// A mapping from caller parameters to ids
    caller_parameters: BiHashMap<Vec<Value>, usize>,
    /// A mapping from target parameters to ids
    target_parameters: BiHashMap<Vec<Value>, usize>,
    /// Dummy caller window instances, from which the ones for the targets are created. Indexed bu caller id.
    caller_instances: Vec<SlidingWindow>,
    /// The actual windows. First indexed by target id and the caller id
    windows: Vec<Vec<SlidingWindow>>,
    /// The Mir representation of the sliding window
    mir_window: Either<MirSlidingWindow, MirDiscreteWindow>,
    /// A dummy window to get the default value.
    default_window: SlidingWindow,
    /// A queue of sliding windows whos target is already closed which should be closed after their period.
    to_be_closed: PriorityQueue<Vec<Value>, Reverse<Time>>,
}

impl TwoLayerSlidingWindowCollection {
    /// Creates a new Collection for real-time sliding windows
    pub(crate) fn new_for_sliding(window: &MirSlidingWindow) -> Self {
        TwoLayerSlidingWindowCollection {
            caller_parameters: BiHashMap::new(),
            target_parameters: BiHashMap::new(),
            caller_instances: vec![],
            windows: vec![],
            mir_window: Either::Left(window.clone()),
            default_window: SlidingWindow::from_sliding(Time::default(), window, false),
            to_be_closed: PriorityQueue::new(),
        }
    }

    /// Creates a new Collection for discrete sliding windows
    pub(crate) fn new_for_discrete(window: &MirDiscreteWindow) -> Self {
        TwoLayerSlidingWindowCollection {
            caller_parameters: BiHashMap::new(),
            target_parameters: BiHashMap::new(),
            caller_instances: vec![],
            windows: vec![],
            mir_window: Either::Right(window.clone()),
            default_window: SlidingWindow::from_discrete(
                window.duration,
                window.wait,
                window.op,
                Time::default(),
                &window.ty,
                false,
            ),
            to_be_closed: PriorityQueue::new(),
        }
    }

    fn create_window(&self, start_time: Time, active: bool) -> SlidingWindow {
        match &self.mir_window {
            Either::Left(sw) => SlidingWindow::from_sliding(start_time, sw, active),
            Either::Right(dw) => SlidingWindow::from_discrete(dw.duration, dw.wait, dw.op, start_time, &dw.ty, active),
        }
    }

    pub(crate) fn default_value(&self, ts: Time) -> Value {
        self.default_window.get_value(ts)
    }

    pub(crate) fn spawn_target_instance(&mut self, parameter: &[Value]) {
        self.to_be_closed.remove(parameter);
        let next_target_id = self.windows.len();
        self.target_parameters.insert(parameter.to_vec(), next_target_id);
        self.windows.push(self.caller_instances.clone());
    }

    fn remove_target_instance(&mut self, parameter: &[Value]) {
        debug_assert!(!self.windows.is_empty());
        let (_, id) = self
            .target_parameters
            .remove_by_left(parameter)
            .expect("Instance to exist");
        if id == self.windows.len() - 1 {
            self.windows.pop();
        } else {
            // Move last element in windows to position of removed instance
            let replacement_id = self.windows.len() - 1;
            let replacement = self.windows.pop().unwrap();
            let (replacement_paras, _) = self.target_parameters.remove_by_right(&replacement_id).unwrap();
            self.windows[id] = replacement;
            self.target_parameters.insert(replacement_paras, id);
        }
    }

    fn delete_expired_windows(&mut self, ts: Time) {
        while self.to_be_closed.peek().map(|(_, due)| ts > due.0).unwrap_or(false) {
            let (params, _) = self.to_be_closed.pop().unwrap();
            self.remove_target_instance(params.as_slice());
        }
    }

    pub(crate) fn close_target_instance(&mut self, parameter: &[Value], ts: Time) {
        match &self.mir_window {
            Either::Left(sw) => {
                self.to_be_closed.push(parameter.to_vec(), Reverse(ts + sw.duration));
            },
            Either::Right(_) => {
                self.remove_target_instance(parameter);
            },
        }
    }

    pub(crate) fn spawn_caller_instance(&mut self, parameters: &[Value], ts: Time) {
        self.delete_expired_windows(ts);
        let next_caller_id = self.caller_instances.len();
        self.caller_parameters.insert(parameters.to_vec(), next_caller_id);
        let window = self.create_window(ts, true);
        self.caller_instances.push(window.clone());
        self.windows.iter_mut().for_each(|callers| callers.push(window.clone()));
    }

    pub(crate) fn close_caller_instance(&mut self, parameter: &[Value], ts: Time) {
        self.delete_expired_windows(ts);
        debug_assert!(!self.caller_instances.is_empty());
        let (_, id) = self
            .caller_parameters
            .remove_by_left(parameter)
            .expect("Instance to exist");
        if id == self.caller_instances.len() - 1 {
            self.caller_instances.pop();
            self.windows.iter_mut().for_each(|caller| {
                caller.remove(id);
            });
        } else {
            // Move last element in windows to position of removed instance
            let replacement_id = self.caller_instances.len() - 1;
            let replacement = self.caller_instances.pop().unwrap();
            self.caller_instances[id] = replacement;
            let (replacement_paras, _) = self.caller_parameters.remove_by_right(&replacement_id).unwrap();
            self.caller_parameters.insert(replacement_paras, id);

            self.windows
                .iter_mut()
                .for_each(|caller| caller[id] = caller.pop().unwrap())
        }
    }

    /// Returns a reference to the sliding window corresponding to the parameters
    pub(crate) fn window(
        &mut self,
        target_parameter: &[Value],
        caller_parameter: &[Value],
        ts: Time,
    ) -> Option<&SlidingWindow> {
        self.delete_expired_windows(ts);
        self.target_parameters
            .get_by_left(target_parameter)
            .and_then(|target_id| {
                self.caller_parameters
                    .get_by_left(caller_parameter)
                    .map(|caller_id| (*target_id, *caller_id))
            })
            .map(|(target, caller)| &self.windows[target][caller])
    }

    /// Updates all windows in the collection with the given time
    pub(crate) fn update_all(&mut self, ts: Time) {
        self.delete_expired_windows(ts);
        self.windows
            .iter_mut()
            .for_each(|c| c.iter_mut().for_each(|w| w.update(ts)));
    }

    /// Adds a new value to all windows in the collection with the given time
    pub(crate) fn accept_value(&mut self, target_parameter: &[Value], value: Value, ts: Time) {
        self.delete_expired_windows(ts);
        let id = *self.target_parameters.get_by_left(target_parameter).unwrap();
        self.windows[id]
            .iter_mut()
            .for_each(|w| w.accept_value(value.clone(), ts));
    }
}

/// Storage to access stream values and window values during the execution
pub(crate) struct GlobalStore {
    /// Access by stream reference.
    inputs: Vec<InstanceStore>,

    /// Transforms a output stream reference into the respective index of the stream vectors ((non-)parametrized).
    stream_index_map: Vec<usize>,

    /// Non-parametrized outputs. Access by the index stored in the stream_index_map.
    /// A typical access looks like this: `np_outputs[stream_index_map[output_reference]]`
    np_outputs: Vec<InstanceStore>,

    /// parameterized outputs. Accessed by the index stored in the stream_index_map.
    /// /// A typical access looks like this: `p_outputs[stream_index_map[output_reference]]`
    p_outputs: Vec<InstanceCollection>,

    /// Transforms a sliding window reference into the respective index of the window vectors ((non-)parametrized).
    window_index_map: Vec<usize>,

    /// Holds the corresponding [WindowParameterization] for a given sliding window reference.
    window_parameterization: Vec<WindowParameterization>,

    /// Non-parametrized windows, access by the index stored in the window_index_map.
    /// A typical access looks like this: `np_windows[window_index_map[window_index]]`
    np_windows: Vec<SlidingWindow>,

    /// Windows that are parameterized by caller or target but not both. Access by the index stored in the window_index_map.
    /// A typical access looks like this: `p_windows[window_index_map[window_index]]`
    p_windows: Vec<SlidingWindowCollection>,

    /// Windows that are parameterized by caller and target.
    /// A typical access looks like this: `both_p_windows[window_index_map[window_index]]`
    both_p_windows: Vec<TwoLayerSlidingWindowCollection>,

    /// Transforms a discrete window reference into the respective index of the discrete window vectors ((non-)parametrized).
    discrete_window_index_map: Vec<usize>,

    /// Holds the corresponding [WindowParameterization] for a given discrete window reference.
    discrete_window_parameterization: Vec<WindowParameterization>,

    /// Non-parametrized discrete windows, access the index stored in the discrete_window_index_map.
    /// A typical access looks like this: `np_discrete_windows[discrete_window_index_map[window_index]]`
    np_discrete_windows: Vec<SlidingWindow>,

    /// Discrete windows that are parameterized by caller or target but not both. access the index stored in the discrete_window_index_map.
    /// A typical access looks like this: `p_discrete_windows[discrete_window_index_map[window_index]]`
    p_discrete_windows: Vec<SlidingWindowCollection>,

    /// Windows that are parameterized by caller and target.
    /// A typical access looks like this: `both_p_discrete_windows[discrete_window_index_map[window_index]]`
    both_p_discrete_windows: Vec<TwoLayerSlidingWindowCollection>,

    /// Instance aggregations indexed directly by their window reference.
    instance_aggregations: Vec<InstanceAggregation>,
}

impl GlobalStore {
    /// Returns a Global Storage for a given specification and starting point in time, given as:
    ///
    ///  # Arguments
    /// * `ir` - An intermediate representation of the specification
    /// * `time` - The starting time of the monitor
    pub(crate) fn new(ir: &RtLolaMir, ts: Time) -> GlobalStore {
        //Create stream index map
        let mut stream_index_map: Vec<Option<usize>> = vec![None; ir.outputs.len()];

        let (ps, nps): (Vec<&OutputStream>, Vec<&OutputStream>) = ir.outputs.iter().partition(|o| o.is_parameterized());
        let ps_refs: Vec<StreamReference> = ps.iter().map(|o| o.reference).collect();

        for (np_ix, o) in nps.iter().enumerate() {
            stream_index_map[o.reference.out_ix()] = Some(np_ix);
        }
        for (p_ix, o) in ps.iter().enumerate() {
            stream_index_map[o.reference.out_ix()] = Some(p_ix);
        }
        debug_assert!(stream_index_map.iter().all(Option::is_some));
        let stream_index_map = stream_index_map.into_iter().flatten().collect();

        //Create window index map
        let mut window_index_map: Vec<Option<usize>> = vec![None; ir.sliding_windows.len()];
        let mut window_parameterization: Vec<Option<WindowParameterization>> = vec![None; ir.sliding_windows.len()];

        let mut np_sliding_windows: Vec<&MirSlidingWindow> = vec![];
        let mut p_sliding_windows: Vec<&MirSlidingWindow> = vec![];
        let mut both_p_sliding_windows: Vec<&MirSlidingWindow> = vec![];
        for window in ir.sliding_windows.iter() {
            let caller = ir.output(window.caller);
            let origin = *caller
                .accesses
                .iter()
                .flat_map(|(_, accs)| accs)
                .find_map(|(orig, kind)| {
                    match kind {
                        StreamAccessKind::SlidingWindow(w) if *w == window.reference => Some(orig),
                        _ => None,
                    }
                })
                .expect("Window to actually occur in caller");
            let is_spawned = match origin {
                Origin::Eval(_) | Origin::Filter(_) => caller.is_spawned(),
                Origin::Close => caller.close.has_self_reference && caller.is_spawned(),
                _ => false,
            };

            let (idx, kind) = match (
                ps_refs.contains(&window.target),
                ps_refs.contains(&window.caller) && is_spawned,
            ) {
                (false, true) => {
                    p_sliding_windows.push(window);
                    (p_sliding_windows.len() - 1, WindowParameterization::Caller)
                },
                (true, false) => {
                    p_sliding_windows.push(window);
                    (
                        p_sliding_windows.len() - 1,
                        WindowParameterization::Target { is_spawned },
                    )
                },
                (false, false) => {
                    np_sliding_windows.push(window);
                    (
                        np_sliding_windows.len() - 1,
                        WindowParameterization::None { is_spawned },
                    )
                },
                (true, true) => {
                    both_p_sliding_windows.push(window);
                    (both_p_sliding_windows.len() - 1, WindowParameterization::Both)
                },
            };
            window_index_map[window.reference.idx()] = Some(idx);
            window_parameterization[window.reference.idx()] = Some(kind);
        }
        debug_assert!(window_index_map.iter().all(Option::is_some));
        debug_assert!(window_parameterization.iter().all(Option::is_some));
        let window_index_map = window_index_map.into_iter().flatten().collect();
        let window_parameterization: Vec<WindowParameterization> =
            window_parameterization.into_iter().flatten().collect();

        //Create discrete window index map
        let mut discrete_window_index_map: Vec<Option<usize>> = vec![None; ir.discrete_windows.len()];
        let mut discrete_window_parameterization: Vec<Option<WindowParameterization>> =
            vec![None; ir.discrete_windows.len()];
        let mut np_discrete_windows: Vec<&MirDiscreteWindow> = vec![];
        let mut p_discrete_windows: Vec<&MirDiscreteWindow> = vec![];
        let mut both_p_discrete_windows: Vec<&MirDiscreteWindow> = vec![];
        for window in ir.discrete_windows.iter() {
            let caller = ir.output(window.caller);
            let origin = *caller
                .accesses
                .iter()
                .flat_map(|(_, accs)| accs)
                .find_map(|(orig, kind)| {
                    match kind {
                        StreamAccessKind::DiscreteWindow(w) if *w == window.reference => Some(orig),
                        _ => None,
                    }
                })
                .expect("Window to actually occur in caller");
            let is_spawned = match origin {
                Origin::Eval(_) | Origin::Filter(_) => caller.is_spawned(),
                Origin::Close => caller.close.has_self_reference && caller.is_spawned(),
                _ => false,
            };

            let (idx, kind) = match (
                ps_refs.contains(&window.target),
                ps_refs.contains(&window.caller) && is_spawned,
            ) {
                (false, false) => {
                    np_discrete_windows.push(window);
                    (
                        np_discrete_windows.len() - 1,
                        WindowParameterization::None { is_spawned },
                    )
                },
                (true, false) => {
                    p_discrete_windows.push(window);
                    (
                        p_discrete_windows.len() - 1,
                        WindowParameterization::Target { is_spawned },
                    )
                },
                (false, true) => {
                    p_discrete_windows.push(window);
                    (p_discrete_windows.len() - 1, WindowParameterization::Caller)
                },
                (true, true) => {
                    both_p_discrete_windows.push(window);
                    (both_p_discrete_windows.len() - 1, WindowParameterization::Both)
                },
            };
            discrete_window_index_map[window.reference.idx()] = Some(idx);
            discrete_window_parameterization[window.reference.idx()] = Some(kind);
        }
        debug_assert!(discrete_window_index_map.iter().all(Option::is_some));
        debug_assert!(discrete_window_parameterization.iter().all(Option::is_some));
        let discrete_window_index_map = discrete_window_index_map.into_iter().flatten().collect();
        let discrete_window_parameterization: Vec<WindowParameterization> =
            discrete_window_parameterization.into_iter().flatten().collect();

        let np_outputs = nps
            .iter()
            .map(|o| InstanceStore::new(&o.ty, o.memory_bound, !o.is_spawned()))
            .collect();
        let p_outputs = ps
            .iter()
            .map(|o| InstanceCollection::new(&o.ty, o.memory_bound))
            .collect();
        let inputs = ir
            .inputs
            .iter()
            .map(|i| InstanceStore::new(&i.ty, i.memory_bound, true))
            .collect();
        let np_windows = np_sliding_windows
            .iter()
            .map(|w| SlidingWindow::from_sliding(ts, w, !window_parameterization[w.reference.idx()].is_spawned()))
            .collect();
        let p_windows = p_sliding_windows
            .iter()
            .map(|w| SlidingWindowCollection::new_for_sliding(w))
            .collect();
        let both_p_windows = both_p_sliding_windows
            .iter()
            .map(|w| TwoLayerSlidingWindowCollection::new_for_sliding(w))
            .collect();
        let np_discrete_windows = np_discrete_windows
            .iter()
            .map(|w| {
                SlidingWindow::from_discrete(
                    w.duration,
                    w.wait,
                    w.op,
                    ts,
                    &w.ty,
                    !discrete_window_parameterization[w.reference.idx()].is_spawned(),
                )
            })
            .collect();
        let p_discrete_windows = p_discrete_windows
            .iter()
            .map(|w| SlidingWindowCollection::new_for_discrete(w))
            .collect();
        let both_p_discrete_windows = both_p_discrete_windows
            .iter()
            .map(|w| TwoLayerSlidingWindowCollection::new_for_discrete(w))
            .collect();

        let instance_aggregations = ir
            .instance_aggregations
            .iter()
            .map(|ia| InstanceAggregation::from(ia))
            .collect();

        GlobalStore {
            inputs,
            stream_index_map,
            np_outputs,
            p_outputs,
            window_index_map,
            window_parameterization,
            np_windows,
            p_windows,
            both_p_windows,
            discrete_window_index_map,
            discrete_window_parameterization,
            np_discrete_windows,
            p_discrete_windows,
            both_p_discrete_windows,
            instance_aggregations,
        }
    }

    /// Returns the storage of an input stream instance
    pub(crate) fn get_in_instance(&self, inst: InputReference) -> &InstanceStore {
        let ix = inst;
        &self.inputs[ix]
    }

    /// Return the storage of an input stream instance (mutable)
    pub(crate) fn get_in_instance_mut(&mut self, inst: InputReference) -> &mut InstanceStore {
        let ix = inst;
        &mut self.inputs[ix]
    }

    /// Returns the storage of an output stream instance
    /// Note: OutputReference *must* point to non-parameterized stream
    pub(crate) fn get_out_instance(&self, inst: OutputReference) -> &InstanceStore {
        let ix = inst;
        &self.np_outputs[self.stream_index_map[ix]]
    }

    /// Returns all InstanceStores of this output as an [InstanceCollection]
    /// Note: OutputReference *must* point to parameterized stream
    pub(crate) fn get_out_instance_collection(&self, inst: OutputReference) -> &InstanceCollection {
        &self.p_outputs[self.stream_index_map[inst]]
    }

    /// Returns the storage of an output stream instance (mutable)
    /// Note: OutputReference *must* point to non-parameterized stream
    pub(crate) fn get_out_instance_mut(&mut self, inst: OutputReference) -> &mut InstanceStore {
        let ix = inst;
        &mut self.np_outputs[self.stream_index_map[ix]]
    }

    /// Returns all InstanceStores (mutable) of this output as an [InstanceCollection]
    /// Note: OutputReference *must* point to parameterized stream
    pub(crate) fn get_out_instance_collection_mut(&mut self, inst: OutputReference) -> &mut InstanceCollection {
        let ix = inst;
        &mut self.p_outputs[self.stream_index_map[ix]]
    }

    /// Returns the storage of a sliding window instance
    /// Note: The windows callee *must* be a non-parameterized stream
    pub(crate) fn get_window(&self, window: WindowReference) -> &SlidingWindow {
        match window {
            WindowReference::Sliding(x) => &self.np_windows[self.window_index_map[x]],
            WindowReference::Discrete(x) => &self.np_discrete_windows[self.discrete_window_index_map[x]],
            WindowReference::Instance(_) => unreachable!("Called window function with instance aggregation reference"),
        }
    }

    /// Returns the storage of a sliding window instance (mutable)
    /// Note: The windows callee *must* be a non-parameterized stream
    pub(crate) fn get_window_mut(&mut self, window: WindowReference) -> &mut SlidingWindow {
        match window {
            WindowReference::Sliding(x) => &mut self.np_windows[self.window_index_map[x]],
            WindowReference::Discrete(x) => &mut self.np_discrete_windows[self.discrete_window_index_map[x]],
            WindowReference::Instance(_) => unreachable!("Called window function with instance aggregation reference"),
        }
    }

    /// Returns the collection of all sliding window instances
    pub(crate) fn get_window_collection_mut(&mut self, window: WindowReference) -> &mut SlidingWindowCollection {
        match window {
            WindowReference::Sliding(x) => &mut self.p_windows[self.window_index_map[x]],
            WindowReference::Discrete(x) => &mut self.p_discrete_windows[self.discrete_window_index_map[x]],
            WindowReference::Instance(_) => unreachable!("Called window function with instance aggregation reference"),
        }
    }

    pub(crate) fn get_two_layer_window_collection_mut(
        &mut self,
        window: WindowReference,
    ) -> &mut TwoLayerSlidingWindowCollection {
        match window {
            WindowReference::Sliding(x) => &mut self.both_p_windows[self.window_index_map[x]],
            WindowReference::Discrete(x) => &mut self.both_p_discrete_windows[self.discrete_window_index_map[x]],
            WindowReference::Instance(_) => unreachable!("Called window function with instance aggregation reference"),
        }
    }

    pub(crate) fn window_parameterization(&self, window: WindowReference) -> WindowParameterization {
        match window {
            WindowReference::Sliding(x) => self.window_parameterization[x],
            WindowReference::Discrete(x) => self.discrete_window_parameterization[x],
            WindowReference::Instance(_) => unreachable!("Called window function with instance aggregation reference"),
        }
    }

    pub(crate) fn get_instance_aggregation_mut(&mut self, window: WindowReference) -> &mut InstanceAggregation {
        if let WindowReference::Instance(idx) = window {
            &mut self.instance_aggregations[idx]
        } else {
            unreachable!("Called get_instance_aggregation_mut for non instance");
        }
    }

    /// Updates the instance aggregation with all instances and returns a mutable reference to the aggregation.
    pub(crate) fn update_instance_aggregation(&mut self, window: WindowReference) -> &mut InstanceAggregation {
        if let WindowReference::Instance(idx) = window {
            let aggr = &mut self.instance_aggregations[idx];
            let target = &self.p_outputs[self.stream_index_map[aggr.target.out_ix()]];
            aggr.all_instances(target);
            aggr
        } else {
            unreachable!("Called update_instance_aggregation for non instance");
        }
    }

    /// Marks all instances in the store as fresh
    pub(crate) fn new_cycle(&mut self) {
        self.p_outputs.iter_mut().for_each(|is| is.new_cycle());
    }
}

/// Storage of a stream instance
#[derive(Clone, Debug)]
pub(crate) struct InstanceStore {
    /// Buffer contains the offset values, where new elements get stored at the front
    buffer: VecDeque<Value>,
    /// Bound of the buffer
    bound: MemorizationBound,
    /// Is the instance currently spawned
    active: bool,
}

const SIZE: usize = 256;

impl InstanceStore {
    // _type might be used later.
    /// Returns the storage of a stream instance, by setting the size of the buffer to the given bound
    pub(crate) fn new(_type: &Type, bound: MemorizationBound, active: bool) -> InstanceStore {
        match bound {
            MemorizationBound::Bounded(limit) => {
                InstanceStore {
                    buffer: VecDeque::with_capacity(limit as usize),
                    bound,
                    active,
                }
            },
            MemorizationBound::Unbounded => {
                InstanceStore {
                    buffer: VecDeque::with_capacity(SIZE),
                    bound,
                    active,
                }
            },
        }
    }

    /// Returns the current value of a stream instance at the given offset
    pub(crate) fn get_value(&self, offset: i16) -> Option<Value> {
        assert!(offset <= 0);
        if !self.active {
            return None;
        }
        if offset == 0 {
            self.buffer.front().cloned()
        } else {
            let offset = offset.unsigned_abs() as usize;
            self.buffer.get(offset).cloned()
        }
    }

    /// Updates the buffer of stream instance
    pub(crate) fn push_value(&mut self, v: Value) {
        assert!(self.active);
        if let MemorizationBound::Bounded(limit) = self.bound {
            if self.buffer.len() == limit as usize {
                self.buffer.pop_back();
            }
        }
        self.buffer.push_front(v);
    }

    pub(crate) fn activate(&mut self) {
        self.active = true;
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active
    }

    /// removes all values of the instance
    pub(crate) fn deactivate(&mut self) {
        self.active = false;
        self.buffer.clear()
    }
}
