use super::Value;

use crate::basics::Time;
use crate::storage::SlidingWindow;
use regex::internal::Inst;
use rtlola_frontend::mir::{ExpressionKind, InputReference, MemorizationBound, OutputReference, OutputStream, RtLolaMir, Type, WindowReference, StreamReference, WindowOperation};
use std::collections::{HashMap, VecDeque};
use std::time::Duration;

/// The collection of all instances of a parameterized stream
pub(crate) struct InstanceCollection {
    /// The instances accessed by parameter values
    instances: HashMap<Vec<Value>, InstanceStore>,
    /// The value type of data that should be stored
    value_type: Type,
    /// The memorization bound of the instance
    bound: MemorizationBound,
}

impl InstanceCollection {
    pub(crate) fn new(ty: &Type, bound: MemorizationBound) -> Self {
        InstanceCollection { instances: HashMap::new(), value_type: ty.clone(), bound }
    }

    /// Returns a reference to the instance store of the instance corresponding to the parameters
    pub(crate) fn get_instance(&self, parameter: &[Value]) -> Option<&InstanceStore> {
        self.instances.get(parameter)
    }

    /// Returns a mutable reference to the instance store of the instance corresponding to the parameters
    pub(crate) fn get_instance_mut(&mut self, parameter: &[Value]) -> Option<&mut InstanceStore> {
        self.instances.get_mut(parameter)
    }

    /// Creates a new instance if not existing and returns a reference to the *new* instance if created
    pub(crate) fn create_instance(&mut self, parameter: &[Value]) -> Option<&InstanceStore> {
        if !self.instances.contains_key(parameter) {
            self.instances.insert(parameter.to_vec(), InstanceStore::new(&self.value_type, self.bound));
            self.instances.get(parameter)
        } else {
            None
        }
    }

    /// Deletes the instance corresponding to the parameters
    pub(crate) fn delete_instance(&mut self, parameter: &[Value]) {
        debug_assert!(self.instances.contains_key(parameter));
        self.instances.remove(parameter);
    }

    /// Returns a vector of all parameters for which an instance exists
    pub(crate) fn get_all_instances(&self, inst: OutputReference) -> Vec<&[Value]> {
        self.instances.keys().map(|i| i.as_slice()).collect()
    }
}

/// The collection of all sliding windows of a parameterized stream
struct SlidingWindowCollection {
    windows: HashMap<Vec<Value>, SlidingWindow>,
    duration: Duration,
    wait: bool,
    op: WindowOperation,
    ty: Type,
}

impl SlidingWindowCollection {
    pub(crate) fn new(dur: Duration, wait: bool, op: WindowOperation, ty: &Type) -> Self {
        SlidingWindowCollection{windows: HashMap::new(), duration: dur, wait, op, ty: ty.clone()}
    }
}

/// Storage to access stream values and window values during the execution
pub(crate) struct GlobalStore {
    /// Access by stream reference.
    inputs: Vec<InstanceStore>,

    /// Transforms a output stream reference into the respective index of the stream vectors ((non-)parametrized).
    stream_index_map: Vec<usize>,

    /// Non-parametrized outputs. Access by index.
    np_outputs: Vec<InstanceStore>,

    /// parameterized outputs. Accessed by index.
    p_outputs: Vec<InstanceCollection>,

    /// Transforms a sliding window index into the respective index of the window vectors ((non-)parametrized).
    window_index_map: Vec<usize>,

    /// Non-parametrized windows, access by WindowReference.
    np_windows: Vec<SlidingWindow>,

    /// parametrized windows, access by WindowReference.
    p_windows: Vec<SlidingWindowCollection>,

    /// Transforms a discrete window index into the respective index of the discrete window vectors ((non-)parametrized).
    discrete_window_index_map: Vec<usize>,

    /// Non-parametrized discrete windows, access by WindowReference,
    np_discrete_windows: Vec<SlidingWindow>,

    /// parametrized discrete windows, access by WindowReference,
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

        let (nps, ps): (Vec<&OutputStream>, Vec<&OutputStream>) =
            ir.outputs.iter().partition(|o| o.instance_template.spawn.target.is_none());
        let nps_refs: Vec<StreamReference> = nps.iter().map(|o| o.reference).collect();
        let ps_refs: Vec<StreamReference> = ps.iter().map(|o| o.reference).collect();

        for (np_ix, o) in nps.iter().enumerate() {
            stream_index_map[o.reference.out_ix()] = Some(np_ix);
        }
        for (p_ix, o) in ps.iter().enumerate() {
            stream_index_map[o.reference.out_ix()] = Some(p_ix);
        }
        assert!(stream_index_map.iter().all(Option::is_some));
        let stream_index_map = stream_index_map.into_iter().flatten().collect();

        //Create window index map
        let mut window_index_map: Vec<Option<usize>> = vec![None; ir.sliding_windows.len()];
        let (np_windows, p_windows): (Vec<&rtlola_frontend::mir::SlidingWindow>, Vec<&rtlola_frontend::mir::SlidingWindow>) = ir.sliding_windows.iter().partition(|w| w.target.is_input() || nps_refs.contains(&w.target));
        for (ix, w) in np_windows.iter().enumerate() {
            window_index_map[w.reference.idx()] = Some(ix);
        }
        for (ix, w) in p_windows.iter().enumerate() {
            window_index_map[w.reference.idx()] = Some(ix);
        }
        assert!(window_index_map.iter().all(Option::is_some));
        let window_index_map = window_index_map.into_iter().flatten().collect();

        //Create discrete window index map
        let mut discrete_window_index_map: Vec<Option<usize>> = vec![None; ir.discrete_windows.len()];
        let (np_discrete_windows, p_discrete_windows): (Vec<&rtlola_frontend::mir::DiscreteWindow>, Vec<&rtlola_frontend::mir::DiscreteWindow>) = ir.discrete_windows.iter().partition(|w| w.target.is_input() || nps_refs.contains(&w.target));
        for (ix, w) in np_discrete_windows.iter().enumerate() {
            discrete_window_index_map[w.reference.idx()] = Some(ix);
        }
        for (ix, w) in p_discrete_windows.iter().enumerate() {
            discrete_window_index_map[w.reference.idx()] = Some(ix);
        }
        assert!(discrete_window_index_map.iter().all(Option::is_some));
        let discrete_window_index_map = discrete_window_index_map.into_iter().flatten().collect();


        let np_outputs = nps.iter().map(|o| InstanceStore::new(&o.ty, o.memory_bound)).collect();
        let p_outputs = ps.iter().map(|o| InstanceCollection::new(&o.ty, o.memory_bound)).collect();
        let inputs = ir.inputs.iter().map(|i| InstanceStore::new(&i.ty, i.memory_bound)).collect();
        let np_windows = np_windows
            .iter()
            .map(|w| SlidingWindow::from_sliding(w.duration, w.wait, w.op, ts, &w.ty))
            .collect();
        let p_windows = p_windows.iter().map(|w| SlidingWindowCollection::new(w.duration, w.wait, w.op, &w.ty)).collect();
        let np_discrete_windows = np_discrete_windows
            .iter()
            .map(|w| SlidingWindow::from_discrete(w.duration, w.wait, w.op, ts, &w.ty))
            .collect();
        let p_discrete_windows = p_discrete_windows
            .iter()
            .map(|w| SlidingWindowCollection::new(w.duration, w.wait, w.op, &w.ty))
            .collect();

        GlobalStore { inputs, stream_index_map, np_outputs, p_outputs, window_index_map, np_windows, p_windows, discrete_window_index_map, np_discrete_windows, p_discrete_windows }
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
    pub(crate) fn get_out_instance(&self, inst: OutputReference) -> Option<&InstanceStore> {
        let ix = inst;
        Some(&self.np_outputs[self.stream_index_map[ix]])
    }

    /// Returns all InstanceStores of this output as an [InstanceCollection]
    pub(crate) fn get_out_instance_collection(&self, inst: OutputReference) -> &InstanceCollection {
        let ix = inst;
        &self.p_outputs[self.stream_index_map[ix]]
    }

    /// Returns the storage of an output stream instance (mutable)
    pub(crate) fn get_out_instance_mut(&mut self, inst: OutputReference) -> Option<&mut InstanceStore> {
        let ix = inst;
        Some(&mut self.np_outputs[self.stream_index_map[ix]])
    }

    /// Returns all InstanceStores (mutable) of this output as an [InstanceCollection]
    pub(crate) fn get_out_instance_collection_mut(&mut self, inst: OutputReference) -> &mut InstanceCollection {
        let ix = inst;
        &mut self.p_outputs[self.stream_index_map[ix]]
    }

    /// Returns the storage of a sliding window instance
    pub(crate) fn get_window(&self, window: WindowReference) -> &SlidingWindow {
        match window {
            WindowReference::Sliding(x) => &self.np_windows[x],
            WindowReference::Discrete(x) => &self.np_discrete_windows[x],
        }
    }

    /// Returns the storage of a sliding window instance (mutable)
    pub(crate) fn get_window_mut(&mut self, window: WindowReference) -> &mut SlidingWindow {
        match window {
            WindowReference::Sliding(x) => &mut self.np_windows[x],
            WindowReference::Discrete(x) => &mut self.np_discrete_windows[x],
        }
    }
}

/// Storage of a stream instance
#[derive(Clone, Debug)]
pub(crate) struct InstanceStore {
    /// Buffer contains the offset values, where new elements get stored at the front
    buffer: VecDeque<Value>,
    /// Bound of the buffer
    bound: MemorizationBound,
}

const SIZE: usize = 256;

impl InstanceStore {
    // _type might be used later.
    /// Returns the storage of a stream instance, by setting the size of the buffer to the given bound
    pub(crate) fn new(_type: &Type, bound: MemorizationBound) -> InstanceStore {
        match bound {
            MemorizationBound::Bounded(limit) => {
                InstanceStore { buffer: VecDeque::with_capacity(limit as usize), bound }
            }
            MemorizationBound::Unbounded => InstanceStore { buffer: VecDeque::with_capacity(SIZE), bound },
        }
    }

    /// Returns the current value of a stream instance at the given offset
    pub(crate) fn get_value(&self, offset: i16) -> Option<Value> {
        assert!(offset <= 0);
        if offset == 0 {
            self.buffer.front().cloned()
        } else {
            let offset = offset.abs() as usize;
            self.buffer.get(offset).cloned()
        }
    }

    /// Updates the buffer of stream instance
    pub(crate) fn push_value(&mut self, v: Value) {
        if let MemorizationBound::Bounded(limit) = self.bound {
            if self.buffer.len() == limit as usize {
                self.buffer.pop_back();
            }
        }
        self.buffer.push_front(v);
    }

    /// removes all values of the instance
    pub(crate) fn clear(&mut self) {
        self.buffer.clear()
    }
}
