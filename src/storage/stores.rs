use super::Value;

use crate::storage::SlidingWindow;
use crate::Time;
use either::Either;
use rtlola_frontend::mir::{
    InputReference, MemorizationBound, OutputReference, OutputStream, RtLolaMir, Stream, StreamReference, Type,
    WindowOperation, WindowReference,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Duration;

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
            self.instances.insert(parameter.to_vec(), InstanceStore::new(&self.value_type, self.bound, true));
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

/// The collection of all sliding windows of a parameterized stream
pub(crate) struct SlidingWindowCollection {
    windows: HashMap<Vec<Value>, SlidingWindow>,
    duration: Either<Duration, usize>,
    wait: bool,
    op: WindowOperation,
    ty: Type,
}

impl SlidingWindowCollection {
    /// Creates a new Collection for real-time sliding windows
    pub(crate) fn new_for_sliding(dur: Duration, wait: bool, op: WindowOperation, ty: &Type) -> Self {
        SlidingWindowCollection { windows: HashMap::new(), duration: Either::Left(dur), wait, op, ty: ty.clone() }
    }

    /// Creates a new Collection for discrete sliding windows
    pub(crate) fn new_for_discrete(dur: usize, wait: bool, op: WindowOperation, ty: &Type) -> Self {
        SlidingWindowCollection { windows: HashMap::new(), duration: Either::Right(dur), wait, op, ty: ty.clone() }
    }

    /// Creates a new sliding window in the collection
    pub(crate) fn create_window(&mut self, parameters: &[Value], start_time: Time) -> Option<&mut SlidingWindow> {
        if !self.windows.contains_key(parameters) {
            let window = match self.duration {
                Either::Left(dur) => SlidingWindow::from_sliding(dur, self.wait, self.op, start_time, &self.ty, false),
                Either::Right(dur) => {
                    SlidingWindow::from_discrete(dur, self.wait, self.op, start_time, &self.ty, false)
                }
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

    /// Non-parametrized windows, access by the index stored in the window_index_map.
    /// A typical access looks like this: `np_windows[window_index_map[window_index]]`
    np_windows: Vec<SlidingWindow>,

    /// parametrized windows, access by the index stored in the window_index_map.
    /// A typical access looks like this: `p_windows[window_index_map[window_index]]`
    p_windows: Vec<SlidingWindowCollection>,

    /// Transforms a discrete window reference into the respective index of the discrete window vectors ((non-)parametrized).
    discrete_window_index_map: Vec<usize>,

    /// Non-parametrized discrete windows, access the index stored in the discrete_window_index_map.
    /// A typical access looks like this: `np_discrete_windows[discrete_window_index_map[window_index]]`
    np_discrete_windows: Vec<SlidingWindow>,

    /// parametrized discrete windows, access the index stored in the discrete_window_index_map.
    /// A typical access looks like this: `p_discrete_windows[discrete_window_index_map[window_index]]`
    p_discrete_windows: Vec<SlidingWindowCollection>,
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
        let nps_refs: Vec<StreamReference> = nps.iter().map(|o| o.reference).collect();

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
        let (np_windows, p_windows): (
            Vec<&rtlola_frontend::mir::SlidingWindow>,
            Vec<&rtlola_frontend::mir::SlidingWindow>,
        ) = ir.sliding_windows.iter().partition(|w| w.target.is_input() || nps_refs.contains(&w.target));
        for (ix, w) in np_windows.iter().enumerate() {
            window_index_map[w.reference.idx()] = Some(ix);
        }
        for (ix, w) in p_windows.iter().enumerate() {
            window_index_map[w.reference.idx()] = Some(ix);
        }
        debug_assert!(window_index_map.iter().all(Option::is_some));
        let window_index_map = window_index_map.into_iter().flatten().collect();

        //Create discrete window index map
        let mut discrete_window_index_map: Vec<Option<usize>> = vec![None; ir.discrete_windows.len()];
        let (np_discrete_windows, p_discrete_windows): (
            Vec<&rtlola_frontend::mir::DiscreteWindow>,
            Vec<&rtlola_frontend::mir::DiscreteWindow>,
        ) = ir.discrete_windows.iter().partition(|w| w.target.is_input() || nps_refs.contains(&w.target));
        for (ix, w) in np_discrete_windows.iter().enumerate() {
            discrete_window_index_map[w.reference.idx()] = Some(ix);
        }
        for (ix, w) in p_discrete_windows.iter().enumerate() {
            discrete_window_index_map[w.reference.idx()] = Some(ix);
        }
        debug_assert!(discrete_window_index_map.iter().all(Option::is_some));
        let discrete_window_index_map = discrete_window_index_map.into_iter().flatten().collect();

        let np_outputs = nps.iter().map(|o| InstanceStore::new(&o.ty, o.memory_bound, !o.is_spawned())).collect();
        let p_outputs = ps.iter().map(|o| InstanceCollection::new(&o.ty, o.memory_bound)).collect();
        let inputs = ir.inputs.iter().map(|i| InstanceStore::new(&i.ty, i.memory_bound, true)).collect();
        let np_windows = np_windows
            .iter()
            .map(|w| {
                SlidingWindow::from_sliding(w.duration, w.wait, w.op, ts, &w.ty, !ir.stream(w.target).is_spawned())
            })
            .collect();
        let p_windows = p_windows
            .iter()
            .map(|w| SlidingWindowCollection::new_for_sliding(w.duration, w.wait, w.op, &w.ty))
            .collect();
        let np_discrete_windows = np_discrete_windows
            .iter()
            .map(|w| {
                SlidingWindow::from_discrete(w.duration, w.wait, w.op, ts, &w.ty, !ir.stream(w.target).is_spawned())
            })
            .collect();
        let p_discrete_windows = p_discrete_windows
            .iter()
            .map(|w| SlidingWindowCollection::new_for_discrete(w.duration, w.wait, w.op, &w.ty))
            .collect();

        GlobalStore {
            inputs,
            stream_index_map,
            np_outputs,
            p_outputs,
            window_index_map,
            np_windows,
            p_windows,
            discrete_window_index_map,
            np_discrete_windows,
            p_discrete_windows,
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
    /// Note: The windows target *must* be a non-parameterized stream
    pub(crate) fn get_window(&self, window: WindowReference) -> &SlidingWindow {
        match window {
            WindowReference::Sliding(x) => &self.np_windows[self.window_index_map[x]],
            WindowReference::Discrete(x) => &self.np_discrete_windows[self.discrete_window_index_map[x]],
        }
    }

    /// Returns the storage of a sliding window instance (mutable)
    /// Note: The windows target *must* be a non-parameterized stream
    pub(crate) fn get_window_mut(&mut self, window: WindowReference) -> &mut SlidingWindow {
        match window {
            WindowReference::Sliding(x) => &mut self.np_windows[self.window_index_map[x]],
            WindowReference::Discrete(x) => &mut self.np_discrete_windows[self.discrete_window_index_map[x]],
        }
    }

    /// Returns the collection of all sliding window instances
    /// Note: The windows target *must* be a parameterized stream
    pub(crate) fn get_window_collection_mut(&mut self, window: WindowReference) -> &mut SlidingWindowCollection {
        match window {
            WindowReference::Sliding(x) => &mut self.p_windows[self.window_index_map[x]],
            WindowReference::Discrete(x) => &mut self.p_discrete_windows[self.discrete_window_index_map[x]],
        }
    }

    /// Marks all instances in the store as not fresh
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
                InstanceStore { buffer: VecDeque::with_capacity(limit as usize), bound, active }
            }
            MemorizationBound::Unbounded => InstanceStore { buffer: VecDeque::with_capacity(SIZE), bound, active },
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
