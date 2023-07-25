use std::collections::{HashMap, HashSet, VecDeque};
use std::default::Default;

use either::Either;
use rtlola_frontend::mir::{
    DiscreteWindow as MirDiscreteWindow, InputReference, MemorizationBound, Origin, OutputReference, OutputStream,
    RtLolaMir, SlidingWindow as MirSlidingWindow, Stream, StreamAccessKind, StreamReference, Type, WindowReference,
};

use super::Value;
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

    pub(crate) fn delete_instances(&mut self) {
        for inst in self.closed.iter() {
            self.instances.remove(inst);
        }
    }

    /// Returns a vector of all parameters for which an instance exists
    pub(crate) fn all_instances(&self) -> Vec<Vec<Value>> {
        self.instances.keys().cloned().collect()
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

// #[derive(Debug, Clone, Hash, Eq, PartialEq, Default)]
// struct WindowParameter {
//     caller: Option<Vec<Value>>,
//     target: Option<Vec<Value>>,
// }
//
// impl WindowParameter {
//     fn parameterized_caller(paras: Vec<Value>) -> Self {
//         Self{
//             caller: Some(paras),
//             .. Default::default()
//         }
//     }
//     fn parameterized_target(paras: Vec<Value>) -> Self {
//         Self{
//             target: Some(paras),
//             .. Default::default()
//         }
//     }
// }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowParameterization {
    None {
        /// whether the window is spawned nonetheless
        is_spawned: bool,
    },
    Caller,
    Target,
    Both,
}

impl WindowParameterization {
    pub(crate) fn is_spawned(&self) -> bool {
        match self {
            WindowParameterization::None { is_spawned } => *is_spawned,
            WindowParameterization::Caller | WindowParameterization::Target | WindowParameterization::Both => true,
        }
    }
}

/// The collection of all sliding windows of a parameterized stream
pub(crate) struct SlidingWindowCollection {
    windows: HashMap<Vec<Value>, SlidingWindow>,
    mir_window: Either<MirSlidingWindow, MirDiscreteWindow>,
    default_window: SlidingWindow,
}

impl SlidingWindowCollection {
    /// Creates a new Collection for real-time sliding windows
    pub(crate) fn new_for_sliding(window: &MirSlidingWindow) -> Self {
        SlidingWindowCollection {
            windows: HashMap::new(),
            mir_window: Either::Left(window.clone()),
            default_window: SlidingWindow::from_sliding(Time::default(), window, false),
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
        }
    }

    pub(crate) fn default_value(&self, ts: Time) -> Value {
        self.default_window.get_value(ts)
    }

    /// Creates a new sliding window in the collection
    pub(crate) fn create_window(&mut self, parameters: &[Value], start_time: Time) -> Option<&mut SlidingWindow> {
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
    pub(crate) fn get_or_create(&mut self, parameters: &[Value], start_time: Time) -> &mut SlidingWindow {
        if self.windows.contains_key(parameters) {
            self.window_mut(parameters).unwrap()
        } else {
            self.create_window(parameters, start_time).unwrap()
        }
    }

    /// Returns a reference to the sliding window corresponding to the parameters
    pub(crate) fn window(&self, parameter: &[Value]) -> Option<&SlidingWindow> {
        self.windows.get(parameter)
    }

    /// Returns a mutable reference to the sliding window corresponding to the parameters
    pub(crate) fn window_mut(&mut self, parameter: &[Value]) -> Option<&mut SlidingWindow> {
        self.windows.get_mut(parameter)
    }

    /// Deletes the sliding window corresponding to the parameters
    pub(crate) fn delete_window(&mut self, parameter: &[Value]) {
        debug_assert!(self.windows.contains_key(parameter));
        self.windows.remove(parameter);
    }

    /// Updates all windows in the collection with the given time
    pub(crate) fn update_all(&mut self, ts: Time) {
        self.windows.iter_mut().for_each(|(_, w)| {
            if w.is_active() {
                w.update(ts)
            }
        });
    }

    /// Adds a new value to all windows in the collection with the given time
    pub(crate) fn accept_value_all(&mut self, value: Value, ts: Time) {
        self.windows
            .iter_mut()
            .for_each(|(_, w)| w.accept_value(value.clone(), ts));
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

    /// windows where the caller is parameterized, access by the index stored in the window_index_map.
    /// A typical access looks like this: `caller_p_windows[window_index_map[window_index]]`
    caller_p_windows: Vec<SlidingWindowCollection>,

    /// windows where the target is parameterized, access by the index stored in the window_index_map.
    /// A typical access looks like this: `target_p_windows[window_index_map[window_index]]`
    target_p_windows: Vec<SlidingWindowCollection>,

    /// Transforms a discrete window reference into the respective index of the discrete window vectors ((non-)parametrized).
    discrete_window_index_map: Vec<usize>,

    /// Holds the corresponding [WindowParameterization] for a given discrete window reference.
    discrete_window_parameterization: Vec<WindowParameterization>,

    /// Non-parametrized discrete windows, access the index stored in the discrete_window_index_map.
    /// A typical access looks like this: `np_discrete_windows[discrete_window_index_map[window_index]]`
    np_discrete_windows: Vec<SlidingWindow>,

    /// discrete windows where the caller is parametrized, access the index stored in the discrete_window_index_map.
    /// A typical access looks like this: `caller_p_discrete_windows[discrete_window_index_map[window_index]]`
    caller_p_discrete_windows: Vec<SlidingWindowCollection>,

    /// discrete windows where the target is parametrized, access the index stored in the discrete_window_index_map.
    /// A typical access looks like this: `target_p_discrete_windows[discrete_window_index_map[window_index]]`
    target_p_discrete_windows: Vec<SlidingWindowCollection>,
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
        let mut caller_p_sliding_windows: Vec<&MirSlidingWindow> = vec![];
        let mut target_p_sliding_windows: Vec<&MirSlidingWindow> = vec![];
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
            let is_spawned = (origin == Origin::Eval
                || origin == Origin::Filter
                || (origin == Origin::Close && caller.close.has_self_reference))
                && caller.is_spawned();

            let (idx, kind) = match (ps_refs.contains(&window.target), ps_refs.contains(&window.caller)) {
                (false, false) => {
                    np_sliding_windows.push(window);
                    (
                        np_sliding_windows.len() - 1,
                        WindowParameterization::None { is_spawned },
                    )
                },
                (true, false) => {
                    target_p_sliding_windows.push(window);
                    (target_p_sliding_windows.len() - 1, WindowParameterization::Target)
                },
                (false, true) => {
                    if origin == Origin::Eval
                        || origin == Origin::Filter
                        || (origin == Origin::Close && caller.close.has_self_reference)
                    {
                        caller_p_sliding_windows.push(window);
                        (caller_p_sliding_windows.len() - 1, WindowParameterization::Caller)
                    } else {
                        np_sliding_windows.push(window);
                        (
                            np_sliding_windows.len() - 1,
                            WindowParameterization::None { is_spawned },
                        )
                    }
                },
                (true, true) => unimplemented!(),
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
        let mut caller_p_discrete_windows: Vec<&MirDiscreteWindow> = vec![];
        let mut target_p_discrete_windows: Vec<&MirDiscreteWindow> = vec![];
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
            let is_spawned = (origin == Origin::Eval
                || origin == Origin::Filter
                || (origin == Origin::Close && caller.close.has_self_reference))
                && caller.is_spawned();

            let (idx, kind) = match (ps_refs.contains(&window.target), ps_refs.contains(&window.caller)) {
                (false, false) => {
                    np_discrete_windows.push(window);
                    (
                        np_discrete_windows.len() - 1,
                        WindowParameterization::None { is_spawned },
                    )
                },
                (true, false) => {
                    target_p_discrete_windows.push(window);
                    (target_p_discrete_windows.len() - 1, WindowParameterization::Target)
                },
                (false, true) => {
                    if origin == Origin::Eval
                        || origin == Origin::Filter
                        || (origin == Origin::Close && caller.close.has_self_reference)
                    {
                        caller_p_discrete_windows.push(window);
                        (caller_p_discrete_windows.len() - 1, WindowParameterization::Caller)
                    } else {
                        np_discrete_windows.push(window);
                        (
                            np_discrete_windows.len() - 1,
                            WindowParameterization::None { is_spawned },
                        )
                    }
                },
                (true, true) => unimplemented!(),
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
        let caller_p_windows = caller_p_sliding_windows
            .iter()
            .map(|w| SlidingWindowCollection::new_for_sliding(w))
            .collect();
        let target_p_windows = target_p_sliding_windows
            .iter()
            .map(|w| SlidingWindowCollection::new_for_sliding(w))
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
        let caller_p_discrete_windows = caller_p_discrete_windows
            .iter()
            .map(|w| SlidingWindowCollection::new_for_discrete(w))
            .collect();
        let target_p_discrete_windows = target_p_discrete_windows
            .iter()
            .map(|w| SlidingWindowCollection::new_for_discrete(w))
            .collect();

        GlobalStore {
            inputs,
            stream_index_map,
            np_outputs,
            p_outputs,
            window_index_map,
            window_parameterization,
            np_windows,
            caller_p_windows,
            target_p_windows,
            discrete_window_index_map,
            discrete_window_parameterization,
            np_discrete_windows,
            caller_p_discrete_windows,
            target_p_discrete_windows,
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
        let ix = inst;
        &self.p_outputs[self.stream_index_map[ix]]
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
        }
    }

    /// Returns the storage of a sliding window instance (mutable)
    /// Note: The windows callee *must* be a non-parameterized stream
    pub(crate) fn get_window_mut(&mut self, window: WindowReference) -> &mut SlidingWindow {
        match window {
            WindowReference::Sliding(x) => {
                dbg!(self.window_parameterization[x]);
                &mut self.np_windows[self.window_index_map[x]]
            },
            WindowReference::Discrete(x) => &mut self.np_discrete_windows[self.discrete_window_index_map[x]],
        }
    }

    /// Returns the collection of all sliding window instances
    pub(crate) fn get_window_collection_mut(&mut self, window: WindowReference) -> &mut SlidingWindowCollection {
        match window {
            WindowReference::Sliding(x) => {
                match self.window_parameterization[x] {
                    WindowParameterization::None { .. } => {
                        panic!("Requested a window collection for a non parameterized window")
                    },
                    WindowParameterization::Caller => &mut self.caller_p_windows[self.window_index_map[x]],
                    WindowParameterization::Target => &mut self.target_p_windows[self.window_index_map[x]],
                    WindowParameterization::Both => unimplemented!(),
                }
            },
            WindowReference::Discrete(x) => {
                match self.discrete_window_parameterization[x] {
                    WindowParameterization::None { .. } => {
                        panic!("Requested a window collection for a non parameterized discrete window")
                    },
                    WindowParameterization::Caller => {
                        &mut self.caller_p_discrete_windows[self.discrete_window_index_map[x]]
                    },
                    WindowParameterization::Target => {
                        &mut self.target_p_discrete_windows[self.discrete_window_index_map[x]]
                    },
                    WindowParameterization::Both => unimplemented!(),
                }
            },
        }
    }

    pub(crate) fn window_parameterization(&self, window: WindowReference) -> WindowParameterization {
        match window {
            WindowReference::Sliding(x) => self.window_parameterization[x],
            WindowReference::Discrete(x) => self.discrete_window_parameterization[x],
        }
    }

    /// Marks all instances in the store as fresh
    pub(crate) fn new_cycle(&mut self) {
        self.p_outputs.iter_mut().for_each(|is| is.new_cycle())
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
