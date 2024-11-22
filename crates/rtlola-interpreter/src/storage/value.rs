use std::cmp::Ordering;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::ops;
use std::ops::{AddAssign, DivAssign, MulAssign, RemAssign, SubAssign};

use num::ToPrimitive;
use num_traits::Pow;
use ordered_float::NotNan;
use rtlola_frontend::mir::Type;
use rust_decimal::Decimal;
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
    /**
    A signed fixed-point number
    */
    Decimal(Decimal),
}

impl Display for Value {
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
            Decimal(i) => write!(f, "{i}"),
        }
    }
}

impl Value {
    /// Returns the interpreted values of an byte representation, if possible:
    /// # Arguments
    /// * 'source' - A byte slice that holds the value
    /// * 'ty' - the type of the interpretation
    pub fn try_from_bytes(source: &[u8], ty: &Type) -> Result<Value, ValueConvertError> {
        if let Ok(source) = std::str::from_utf8(source) {
            if source == "#" {
                return Ok(None);
            }
            match ty {
                Type::Bool => {
                    source
                        .parse::<bool>()
                        .map(Bool)
                        .map_err(|_| ValueConvertError::ParseError(ty.clone(), source.to_string()))
                },
                Type::Bytes => {
                    hex::decode(source)
                        .map(|bytes| Bytes(bytes.into_boxed_slice()))
                        .map_err(|_| ValueConvertError::ParseError(ty.clone(), source.to_string()))
                },
                Type::Int(_) => {
                    source
                        .parse::<i64>()
                        .map(Signed)
                        .map_err(|_| ValueConvertError::ParseError(ty.clone(), source.to_string()))
                },
                Type::UInt(_) => {
                    // TODO: This is just a quickfix!! Think of something more general.
                    if source == "0.0" {
                        Ok(Unsigned(0))
                    } else {
                        source
                            .parse::<u64>()
                            .map(Unsigned)
                            .map_err(|_| ValueConvertError::ParseError(ty.clone(), source.to_string()))
                    }
                },
                Type::Float(_) => {
                    source
                        .parse::<f64>()
                        .map_err(|_| ValueConvertError::ParseError(ty.clone(), source.to_string()))
                        .and_then(Value::try_from)
                },
                Type::String => Ok(Str(source.into())),
                Type::Tuple(inner) => {
                    if inner.is_empty() {
                        (source == "()")
                            .then_some(Tuple(Box::new([])))
                            .ok_or_else(|| ValueConvertError::ParseError(ty.clone(), source.to_string()))
                    } else {
                        unimplemented!()
                    }
                },

                Type::Option(_) | Type::Function { args: _, ret: _ } => unreachable!(),
                Type::Fixed(_) | Type::UFixed(_) => {
                    source
                        .parse::<Decimal>()
                        .map(Decimal)
                        .map_err(|_| ValueConvertError::ParseError(ty.clone(), source.to_string()))
                },
            }
        } else {
            Err(ValueConvertError::NotUtf8(source.to_vec()))
        }
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

    /// Returns self if self is not `Value::None` and other otherwise
    pub fn and_then(self, other: Value) -> Value {
        match self {
            None => other,
            _ => self,
        }
    }

    /// Returns the value for the given type.
    pub fn from_int(ty: &Type, val: i64) -> Value {
        match ty {
            Type::Int(_) => Signed(val),
            Type::UInt(_) => Unsigned(val as u64),
            Type::Float(_) => Float(NotNan::new(val as f64).unwrap()),
            Type::Fixed(_) | Type::UFixed(_) => Decimal(Decimal::from(val)),
            _ => unreachable!("Incompatible Value Type."),
        }
    }

    /// Returns the value in the same variant as self.
    pub fn for_int(&self, val: i64) -> Value {
        match self {
            Signed(_) => Signed(val),
            Unsigned(_) => Unsigned(val as u64),
            Float(_) => Float(NotNan::new(val as f64).unwrap()),
            Decimal(_) => Decimal(Decimal::from(val)),
            _ => unreachable!("Incompatible Value Type."),
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
            (Decimal(v1), Decimal(v2)) => Decimal(v1 + v2),
            (a, b) => panic!("Incompatible types: ({:?},{:?})", a, b),
        }
    }
}

impl AddAssign for Value {
    fn add_assign(&mut self, rhs: Self) {
        match (self, rhs) {
            (Unsigned(v1), Unsigned(v2)) => v1.add_assign(v2),
            (Signed(v1), Signed(v2)) => v1.add_assign(v2),
            (Float(v1), Float(v2)) => v1.add_assign(v2),
            (Decimal(v1), Decimal(v2)) => v1.add_assign(v2),
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
            (Decimal(v1), Decimal(v2)) => Decimal(v1 - v2),
            (a, b) => panic!("Incompatible types: ({:?},{:?})", a, b),
        }
    }
}

impl SubAssign for Value {
    fn sub_assign(&mut self, rhs: Self) {
        match (self, rhs) {
            (Unsigned(v1), Unsigned(v2)) => v1.sub_assign(v2),
            (Signed(v1), Signed(v2)) => v1.sub_assign(v2),
            (Float(v1), Float(v2)) => v1.sub_assign(v2),
            (Decimal(v1), Decimal(v2)) => v1.sub_assign(v2),
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
            (Decimal(v1), Decimal(v2)) => Decimal(v1 * v2),
            (a, b) => panic!("Incompatible types: ({:?},{:?})", a, b),
        }
    }
}

impl MulAssign for Value {
    fn mul_assign(&mut self, rhs: Self) {
        match (self, rhs) {
            (Unsigned(v1), Unsigned(v2)) => v1.mul_assign(v2),
            (Signed(v1), Signed(v2)) => v1.mul_assign(v2),
            (Float(v1), Float(v2)) => v1.mul_assign(v2),
            (Decimal(v1), Decimal(v2)) => v1.mul_assign(v2),
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
            (Decimal(v1), Decimal(v2)) => Decimal(v1 / v2),
            (a, b) => panic!("Incompatible types: ({:?},{:?})", a, b),
        }
    }
}

impl DivAssign for Value {
    fn div_assign(&mut self, rhs: Self) {
        match (self, rhs) {
            (Unsigned(v1), Unsigned(v2)) => v1.div_assign(v2),
            (Signed(v1), Signed(v2)) => v1.div_assign(v2),
            (Float(v1), Float(v2)) => v1.div_assign(v2),
            (Decimal(v1), Decimal(v2)) => v1.div_assign(v2),
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
            (Decimal(v1), Decimal(v2)) => Decimal(v1 % v2),
            (a, b) => panic!("Incompatible types: ({:?},{:?})", a, b),
        }
    }
}

impl RemAssign for Value {
    fn rem_assign(&mut self, rhs: Self) {
        match (self, rhs) {
            (Unsigned(v1), Unsigned(v2)) => v1.rem_assign(v2),
            (Signed(v1), Signed(v2)) => v1.rem_assign(v2),
            (Float(v1), Float(v2)) => v1.rem_assign(v2),
            (Decimal(v1), Decimal(v2)) => v1.rem_assign(v2),
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
            (Float(v1), Float(v2)) => Value::try_from(v1.powf(v2.into())).unwrap(),
            (Float(v1), Signed(v2)) => Value::try_from(v1.powi(v2 as i32)).unwrap(),
            (Decimal(v1), Decimal(v2)) => Decimal(v1.pow(v2)),
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
            Decimal(v1) => Decimal(-v1),
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
            (Decimal(v1), Decimal(v2)) => v1.cmp(v2),
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

impl From<i16> for Value {
    fn from(i: i16) -> Self {
        Signed(i as i64)
    }
}

impl From<i32> for Value {
    fn from(i: i32) -> Self {
        Signed(i as i64)
    }
}

impl From<i64> for Value {
    fn from(i: i64) -> Self {
        Signed(i)
    }
}

impl From<u16> for Value {
    fn from(u: u16) -> Self {
        Unsigned(u as u64)
    }
}

impl From<u32> for Value {
    fn from(u: u32) -> Self {
        Unsigned(u as u64)
    }
}

impl From<u64> for Value {
    fn from(u: u64) -> Self {
        Unsigned(u)
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Str(value.into_boxed_str())
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::from(value.to_string())
    }
}

impl From<Vec<u8>> for Value {
    fn from(value: Vec<u8>) -> Self {
        Bytes(value.into_boxed_slice())
    }
}

impl From<&[u8]> for Value {
    fn from(value: &[u8]) -> Self {
        Value::from(value.to_vec())
    }
}

impl TryFrom<f64> for Value {
    type Error = ValueConvertError;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        let val = NotNan::try_from(value).map_err(|_| ValueConvertError::FloatIsNan)?;
        Ok(Float(val))
    }
}

impl TryFrom<f32> for Value {
    type Error = ValueConvertError;

    fn try_from(value: f32) -> Result<Self, Self::Error> {
        let val = NotNan::try_from(value as f64).map_err(|_| ValueConvertError::FloatIsNan)?;
        Ok(Float(val))
    }
}

impl From<Decimal> for Value {
    fn from(value: Decimal) -> Self {
        Self::Decimal(value)
    }
}

impl From<usize> for Value {
    fn from(value: usize) -> Self {
        Unsigned(value as u64)
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(value: Option<T>) -> Self {
        value.map(T::into).unwrap_or(None)
    }
}

impl TryInto<bool> for Value {
    type Error = ValueConvertError;

    fn try_into(self) -> Result<bool, Self::Error> {
        if let Bool(b) = self {
            Ok(b)
        } else {
            Err(ValueConvertError::TypeMismatch(self))
        }
    }
}

impl TryInto<u64> for Value {
    type Error = ValueConvertError;

    fn try_into(self) -> Result<u64, Self::Error> {
        if let Unsigned(v) = self {
            Ok(v)
        } else {
            Err(ValueConvertError::TypeMismatch(self))
        }
    }
}

impl TryInto<i64> for Value {
    type Error = ValueConvertError;

    fn try_into(self) -> Result<i64, Self::Error> {
        if let Signed(v) = self {
            Ok(v)
        } else {
            Err(ValueConvertError::TypeMismatch(self))
        }
    }
}

impl TryInto<f64> for Value {
    type Error = ValueConvertError;

    fn try_into(self) -> Result<f64, Self::Error> {
        if let Float(v) = self {
            Ok(v.into_inner())
        } else {
            Err(ValueConvertError::TypeMismatch(self))
        }
    }
}

impl TryInto<Box<[Value]>> for Value {
    type Error = ValueConvertError;

    fn try_into(self) -> Result<Box<[Value]>, Self::Error> {
        if let Tuple(v) = self {
            Ok(v)
        } else {
            Err(ValueConvertError::TypeMismatch(self))
        }
    }
}

impl TryInto<Vec<Value>> for Value {
    type Error = ValueConvertError;

    fn try_into(self) -> Result<Vec<Value>, Self::Error> {
        if let Tuple(v) = self {
            Ok(v.to_vec())
        } else {
            Err(ValueConvertError::TypeMismatch(self))
        }
    }
}

impl TryInto<Box<str>> for Value {
    type Error = ValueConvertError;

    fn try_into(self) -> Result<Box<str>, Self::Error> {
        if let Str(v) = self {
            Ok(v)
        } else {
            Err(ValueConvertError::TypeMismatch(self))
        }
    }
}

impl TryInto<String> for Value {
    type Error = ValueConvertError;

    fn try_into(self) -> Result<String, Self::Error> {
        if let Str(v) = self {
            Ok(v.to_string())
        } else {
            Err(ValueConvertError::TypeMismatch(self))
        }
    }
}

impl TryInto<Box<[u8]>> for Value {
    type Error = ValueConvertError;

    fn try_into(self) -> Result<Box<[u8]>, Self::Error> {
        if let Bytes(v) = self {
            Ok(v)
        } else {
            Err(ValueConvertError::TypeMismatch(self))
        }
    }
}

impl TryInto<Vec<u8>> for Value {
    type Error = ValueConvertError;

    fn try_into(self) -> Result<Vec<u8>, Self::Error> {
        if let Bytes(v) = self {
            Ok(v.to_vec())
        } else {
            Err(ValueConvertError::TypeMismatch(self))
        }
    }
}

impl TryInto<Decimal> for Value {
    type Error = ValueConvertError;

    fn try_into(self) -> Result<Decimal, Self::Error> {
        match self {
            Unsigned(v) => Ok(v.into()),
            Signed(v) => Ok(v.into()),
            Float(v) => Ok(v.to_f64().unwrap().try_into().unwrap()),
            Decimal(v) => Ok(v),
            _ => Err(ValueConvertError::ValueNotSupported(Box::new(self))),
        }
    }
}

#[derive(Debug)]
/// Describes an error occurring when converting from or into a value.
pub enum ValueConvertError {
    /// The target type could not be created from the given value.
    TypeMismatch(Value),
    /// Given bytes are expected to be utf-8 coded.
    NotUtf8(Vec<u8>),
    /// Could not parse value given as string into type.
    ParseError(Type, String),
    /// The given value is not supported by the interpreter.
    ValueNotSupported(Box<dyn std::fmt::Debug + Send>),
    /// The given float is NaN.
    FloatIsNan,
}

impl Display for ValueConvertError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ValueConvertError::TypeMismatch(val) => write!(f, "Failed to convert Value: {val}"),
            ValueConvertError::NotUtf8(bytes) => write!(f, "UTF-8 decoding failed for bytes: {bytes:?}"),
            ValueConvertError::ParseError(ty, val) => write!(f, "Failed to parse Value of type {ty} from: {val}"),
            ValueConvertError::FloatIsNan => write!(f, "The given Float is not a number (NaN)"),
            ValueConvertError::ValueNotSupported(v) => {
                write!(f, "The value {v:?} is not supported by the interpreter.")
            },
        }
    }
}

impl Error for ValueConvertError {}

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
