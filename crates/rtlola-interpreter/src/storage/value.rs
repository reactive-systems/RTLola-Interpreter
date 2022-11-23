use std::cmp::Ordering;
use std::fmt::Formatter;
use std::ops;

use ordered_float::NotNan;
use rtlola_frontend::mir::Type;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use self::Value::*;

/**
The general type for holding all kinds of values.
*/
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum Value {
    /**
    Expresses the absence of a value.
    */
    None,
    /**
    A boolean value.
    */
    Bool(bool),
    /**
    An unsigned integer with 64 bits.
    */
    Unsigned(u64),
    /**
    A signed integer with 64 bits.
    */
    Signed(i64),
    /**
    A double-precision floating-point number that is not NaN.
    */
    Float(NotNan<f64>),
    /**
    A tuple of `Value`s.

    The nested values can be of different type.
    */
    Tuple(Box<[Value]>),
    /**
    A string that must be utf-8 encoded.
    */
    Str(Box<str>),
    /**
    A slice of bytes.
    */
    Bytes(Box<[u8]>),
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            None => write!(f, "None"),
            Bool(b) => write!(f, "{}", *b),
            Unsigned(u) => write!(f, "{}", *u),
            Signed(s) => write!(f, "{}", *s),
            Float(fl) => write!(f, "{}", *fl),
            Tuple(t) => {
                write!(f, "(")?;
                if let Some(e) = t.first() {
                    write!(f, "{}", e)?;
                    for b in &t[1..] {
                        write!(f, ", {}", b)?;
                    }
                }
                write!(f, ")")
            },
            Str(str) => write!(f, "{}", *str),
            Bytes(b) => {
                let hex = hex::encode_upper(b);
                write!(f, "{}", hex)
            },
        }
    }
}

impl Value {
    // TODO: -> Result<Option, ConversionError>
    /// Returns the interpreted values of an byte representation, if possible:
    /// # Arguments
    /// * 'source' - A byte slice that holds the value
    /// * 'ty' - the type of the interpretation
    pub fn try_from(source: &[u8], ty: &Type) -> Option<Value> {
        if let Ok(source) = std::str::from_utf8(source) {
            if source == "#" {
                return Some(None);
            }
            match ty {
                Type::Bool => source.parse::<bool>().map(Bool).ok(),
                Type::Bytes => hex::decode(source).map(|bytes| Bytes(bytes.into_boxed_slice())).ok(),
                Type::Int(_) => source.parse::<i64>().map(Signed).ok(),
                Type::UInt(_) => {
                    // TODO: This is just a quickfix!! Think of something more general.
                    if source == "0.0" {
                        Some(Unsigned(0))
                    } else {
                        source.parse::<u64>().map(Unsigned).ok()
                    }
                },
                Type::Float(_) => source.parse::<f64>().ok().map(|f| Float(NotNan::new(f).unwrap())),
                Type::String => Some(Str(source.into())),
                Type::Tuple(_) => unimplemented!(),
                Type::Option(_) | Type::Function { args: _, ret: _ } => unreachable!(),
            }
        } else {
            Option::None // TODO: error message about non-utf8 encoded string?
        }
    }

    /// Returns a float value as 'Value' type:
    /// #Arguments:
    /// * 'f' - the float value as Rust float (f64)
    pub(crate) fn new_float(f: f64) -> Value {
        Float(NotNan::new(f).unwrap())
    }

    /// Decides if a value is of type bool
    pub(crate) fn is_bool(&self) -> bool {
        matches!(self, Bool(_))
    }

    /// Returns the boolean value of a 'Bool' value type
    pub(crate) fn as_bool(&self) -> bool {
        if let Bool(b) = *self {
            b
        } else {
            unreachable!()
        }
    }
}

impl ops::Add for Value {
    type Output = Value;

    fn add(self, other: Value) -> Value {
        match (self, other) {
            (Unsigned(v1), Unsigned(v2)) => Unsigned(v1 + v2),
            (Signed(v1), Signed(v2)) => Signed(v1 + v2),
            (Float(v1), Float(v2)) => Float(v1 + v2),
            (a, b) => panic!("Incompatible types: ({:?},{:?})", a, b),
        }
    }
}

impl ops::Sub for Value {
    type Output = Value;

    fn sub(self, other: Value) -> Value {
        match (self, other) {
            (Unsigned(v1), Unsigned(v2)) => Unsigned(v1 - v2),
            (Signed(v1), Signed(v2)) => Signed(v1 - v2),
            (Float(v1), Float(v2)) => Float(v1 - v2),
            (a, b) => panic!("Incompatible types: ({:?},{:?})", a, b),
        }
    }
}

impl ops::Mul for Value {
    type Output = Value;

    fn mul(self, other: Value) -> Value {
        match (self, other) {
            (Unsigned(v1), Unsigned(v2)) => Unsigned(v1 * v2),
            (Signed(v1), Signed(v2)) => Signed(v1 * v2),
            (Float(v1), Float(v2)) => Float(v1 * v2),
            (a, b) => panic!("Incompatible types: ({:?},{:?})", a, b),
        }
    }
}

impl ops::Div for Value {
    type Output = Value;

    fn div(self, other: Value) -> Value {
        match (self, other) {
            (Unsigned(v1), Unsigned(v2)) => Unsigned(v1 / v2),
            (Signed(v1), Signed(v2)) => Signed(v1 / v2),
            (Float(v1), Float(v2)) => Float(v1 / v2),
            (a, b) => panic!("Incompatible types: ({:?},{:?})", a, b),
        }
    }
}

impl ops::Rem for Value {
    type Output = Value;

    fn rem(self, other: Value) -> Value {
        match (self, other) {
            (Unsigned(v1), Unsigned(v2)) => Unsigned(v1 % v2),
            (Signed(v1), Signed(v2)) => Signed(v1 % v2),
            (Float(v1), Float(v2)) => Float(v1 % v2),
            (a, b) => panic!("Incompatible types: ({:?},{:?})", a, b),
        }
    }
}

impl Value {
    /// Returns the powered value of a value type, with:
    /// # Arguments:
    /// * 'exp' - The exponent given as a 'Value'
    pub(crate) fn pow(self, exp: Value) -> Value {
        match (self, exp) {
            (Unsigned(v1), Unsigned(v2)) => Unsigned(v1.pow(v2 as u32)),
            (Signed(v1), Signed(v2)) => Signed(v1.pow(v2 as u32)),
            (Float(v1), Float(v2)) => Value::new_float(v1.powf(v2.into())),
            (Float(v1), Signed(v2)) => Value::new_float(v1.powi(v2 as i32)),
            (a, b) => panic!("Incompatible types: ({:?},{:?})", a, b),
        }
    }
}

impl ops::BitAnd for Value {
    type Output = Value;

    fn bitand(self, other: Value) -> Value {
        match (self, other) {
            (Bool(v1), Bool(v2)) => Bool(v1 && v2),
            (Unsigned(u1), Unsigned(u2)) => Unsigned(u1 & u2),
            (Signed(s1), Signed(s2)) => Signed(s1 & s2),
            (a, b) => panic!("Incompatible types: ({:?},{:?})", a, b),
        }
    }
}

impl ops::BitOr for Value {
    type Output = Value;

    fn bitor(self, other: Value) -> Value {
        match (self, other) {
            (Bool(v1), Bool(v2)) => Bool(v1 || v2),
            (Unsigned(u1), Unsigned(u2)) => Unsigned(u1 | u2),
            (Signed(s1), Signed(s2)) => Signed(s1 | s2),
            (a, b) => panic!("Incompatible types: ({:?},{:?})", a, b),
        }
    }
}

impl ops::BitXor for Value {
    type Output = Value;

    fn bitxor(self, other: Value) -> Value {
        match (self, other) {
            (Unsigned(u1), Unsigned(u2)) => Unsigned(u1 ^ u2),
            (Signed(s1), Signed(s2)) => Signed(s1 ^ s2),
            (a, b) => panic!("Incompatible types: ({:?},{:?})", a, b),
        }
    }
}

impl ops::Shl for Value {
    type Output = Value;

    fn shl(self, other: Value) -> Value {
        match (self, other) {
            (Unsigned(u1), Unsigned(u2)) => Unsigned(u1 << u2),
            (Signed(s1), Unsigned(u)) => Signed(s1 << u),
            (a, b) => panic!("Incompatible types: ({:?},{:?})", a, b),
        }
    }
}

impl ops::Shr for Value {
    type Output = Value;

    fn shr(self, other: Value) -> Value {
        match (self, other) {
            (Unsigned(u1), Unsigned(u2)) => Unsigned(u1 >> u2),
            (Signed(s1), Unsigned(u)) => Signed(s1 >> u),
            (a, b) => panic!("Incompatible types: ({:?},{:?})", a, b),
        }
    }
}

impl ops::Not for Value {
    type Output = Value;

    fn not(self) -> Value {
        match self {
            Bool(v) => Bool(!v),
            Unsigned(u) => Unsigned(!u),
            Signed(s) => Signed(!s),
            a => panic!("Incompatible type: {:?}", a),
        }
    }
}

impl ops::Neg for Value {
    type Output = Value;

    fn neg(self) -> Value {
        match self {
            Signed(v) => Signed(-v), // TODO Check
            Float(v) => Float(-v),
            a => panic!("Incompatible type: {:?}", a),
        }
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Unsigned(u1), Unsigned(u2)) => u1.cmp(u2),
            (Signed(i1), Signed(i2)) => i1.cmp(i2),
            (Float(f1), Float(f2)) => f1.cmp(f2),
            (Str(s1), Str(s2)) => s1.cmp(s2),
            (a, b) => panic!("Incompatible types: ({:?},{:?})", a, b),
        }
    }
}

// Implement From for Value

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Bool(b)
    }
}

impl From<i64> for Value {
    fn from(i: i64) -> Self {
        Signed(i)
    }
}

impl From<u64> for Value {
    fn from(u: u64) -> Self {
        Unsigned(u)
    }
}

impl From<f64> for Value {
    fn from(f: f64) -> Self {
        Self::new_float(f)
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn size_of_value() {
        let result = std::mem::size_of::<Value>();
        let expected = 24;
        assert!(
            result == expected,
            "Size of `Value` should be {} bytes, was `{}`",
            expected,
            result
        );
    }
}
