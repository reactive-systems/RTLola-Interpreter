use super::Value;
use byteorder::{NativeEndian, ReadBytesExt, WriteBytesExt};
use ordered_float::NotNan;
use streamlab_frontend::ir::{Expression, FloatTy, Temporary, Type};

pub(crate) struct TempStore {
    offsets: Vec<usize>,
    data: Vec<u8>,
    types: Vec<Type>,
}

impl TempStore {
    pub(crate) fn new(expr: &Expression) -> TempStore {
        let n = expr.temporaries.len();
        let mut offsets = Vec::with_capacity(n);
        let mut types = Vec::with_capacity(n);
        let mut size = 0;
        for ty in expr.temporaries.iter() {
            offsets.push(size as usize);
            let ty = match ty {
                Type::Option(t) => t, // We don't store options but resolve during lookup.
                _ => ty,
            };
            types.push(ty.clone());
            size += ty.size().unwrap().0;
        }

        let data = vec![0; size as usize];

        TempStore { offsets, data, types }
    }

    pub(crate) fn get_value(&self, t: Temporary) -> Value {
        match self.types[t.0 as usize] {
            Type::UInt(_) => Value::Unsigned(self.get_unsigned(t)),
            Type::Int(_) => Value::Signed(self.get_signed(t)),
            Type::Bool => Value::Bool(self.get_bool(t)),
            Type::Float(_) => Value::Float(NotNan::new(self.get_float(t)).unwrap()),
            _ => unimplemented!(),
        }
    }

    fn get_bounds(&self, t: Temporary) -> (usize, usize) {
        let lower = self.offsets[t.0 as usize];
        let ty = &self.types[t.0 as usize];
        let diff = ty.size().unwrap().0 as usize;
        let higher = lower + diff;
        (lower, higher)
    }

    pub(crate) fn get_unsigned(&self, t: Temporary) -> u128 {
        // TODO: The check is not required, just for safety.
        if let Type::UInt(_) = self.types[t.0 as usize] {
            let (lower, higher) = self.get_bounds(t);
            u128::from(Self::parse_bytes(&self.data[lower..higher]))
        } else {
            panic!("Unexpected call to `TempStore::get_unsigned`.")
        }
    }

    pub(crate) fn get_float(&self, t: Temporary) -> f64 {
        // TODO: The check is not required, just for safety.
        if let Type::Float(f_size) = self.types[t.0 as usize] {
            let (lower, higher) = self.get_bounds(t);
            match f_size {
                FloatTy::F32 => {
                    let mut seq = vec![0u8; std::mem::size_of::<f32>()];
                    Self::write_byte_seq(&mut seq, &self.data[lower..higher]);
                    f64::from((&seq[..]).read_f32::<NativeEndian>().unwrap())
                }
                FloatTy::F64 => {
                    let mut seq = vec![0u8; std::mem::size_of::<f64>()];
                    Self::write_byte_seq(&mut seq, &self.data[lower..higher]);
                    (&seq[..]).read_f64::<NativeEndian>().unwrap()
                }
            }
        } else {
            panic!("Unexpected call to `TempStore::get_float`.")
        }
    }

    pub(crate) fn get_signed(&self, t: Temporary) -> i128 {
        // TODO: The check is not required, just for safety.
        if let Type::Int(_) = self.types[t.0 as usize] {
            let (lower, higher) = self.get_bounds(t);
            i128::from(Self::parse_bytes(&self.data[lower..higher]))
        } else {
            panic!("Unexpected call to `TempStore::get_unsigned`.")
        }
    }

    pub(crate) fn write_value(&mut self, t: Temporary, v: Value) {
        match (&self.types[t.0], v) {
            (Type::UInt(_), Value::Unsigned(u)) => self.write_unsigned(t, u),
            (Type::Int(_), Value::Signed(i)) => self.write_signed(t, i),
            (Type::Bool, Value::Bool(b)) => self.write_bool(t, b),
            (Type::Float(_), Value::Float(f)) => self.write_float(t, f.into()),
            _ => unimplemented!(),
        }
    }

    pub(crate) fn write_value_forcefully(&mut self, t: Temporary, v: Value) {
        match (&self.types[t.0], v) {
            (Type::UInt(_), Value::Unsigned(u)) => self.write_unsigned(t, u),
            (Type::UInt(_), Value::Signed(i)) => self.write_unsigned(t, i as u128),
            (Type::UInt(_), Value::Float(f)) => {
                let f: f64 = f.into();
                self.write_unsigned(t, f as u128)
            }
            (Type::Int(_), Value::Unsigned(u)) => self.write_signed(t, u as i128),
            (Type::Int(_), Value::Signed(i)) => self.write_signed(t, i),
            (Type::Int(_), Value::Float(f)) => {
                let f: f64 = f.into();
                self.write_signed(t, f as i128)
            }
            (Type::Float(_), Value::Unsigned(u)) => self.write_float(t, u as f64),
            (Type::Float(_), Value::Signed(i)) => self.write_float(t, i as f64),
            (Type::Float(_), Value::Float(f)) => self.write_float(t, f.into()),
            (Type::Bool, Value::Bool(b)) => self.write_bool(t, b),
            _ => unimplemented!(),
        }
    }

    pub(crate) fn write_unsigned(&mut self, t: Temporary, v: u128) {
        // TODO: The check is not required, just for safety.
        if let Type::UInt(_) = self.types[t.0 as usize] {
            let (lower, higher) = self.get_bounds(t);
            Self::write_bytes(&mut self.data[lower..higher], v)
        } else {
            panic!("Unexpected call to `TempStore::get_unsigned`.")
        }
    }

    pub(crate) fn write_float(&mut self, t: Temporary, v: f64) {
        // TODO: The check is not required, just for safety.
        if let Type::Float(f_size) = self.types[t.0 as usize] {
            let (lower, higher) = self.get_bounds(t);
            match f_size {
                FloatTy::F32 => {
                    let mut seq = [0u8; std::mem::size_of::<f32>()];
                    let _ = seq.as_mut().write_f32::<NativeEndian>(v as f32); // TODO: Use `Result`?
                    Self::write_byte_seq(&mut self.data[lower..higher], &seq);
                }
                FloatTy::F64 => {
                    let mut seq = [0u8; std::mem::size_of::<f64>()];
                    let _ = seq.as_mut().write_f64::<NativeEndian>(v); // TODO: Use `Result`?
                    Self::write_byte_seq(&mut self.data[lower..higher], &seq);
                }
            };
        } else {
            panic!("Unexpected call to `TempStore::get_unsigned`.")
        }
    }

    pub(crate) fn write_signed(&mut self, t: Temporary, v: i128) {
        // TODO: The check is not required, just for safety.
        if let Type::Int(_) = self.types[t.0 as usize] {
            let (lower, higher) = self.get_bounds(t);
            Self::write_bytes(&mut self.data[lower..higher], v as u128)
        } else {
            panic!("Unexpected call to `TempStore::get_unsigned`.")
        }
    }

    pub(crate) fn get_bool(&self, t: Temporary) -> bool {
        self.data[self.offsets[t.0 as usize]] != 0
    }

    pub(crate) fn write_bool(&mut self, t: Temporary, v: bool) {
        self.data[self.offsets[t.0 as usize]] = v as u8
    }

    #[inline]
    fn write_byte_seq(dest: &mut [u8], source: &[u8]) {
        assert_eq!(source.len(), dest.len());
        dest.clone_from_slice(&source[..dest.len()]);
    }

    #[inline]
    fn write_bytes(data: &mut [u8], mut v: u128) {
        // Write least significant byte first.
        for i in (0..data.len()).rev() {
            data[i] = v as u8;
            v >>= 8;
        }
    }

    #[inline]
    fn parse_bytes(d: &[u8]) -> u64 {
        assert!(d.len().is_power_of_two());
        let mut res = 0u64;
        for byte in d {
            res = (res << 8) | u64::from(*byte);
        }
        res
    }
}