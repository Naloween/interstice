//! Rust values written to and read from the encoded form of their
//! `IntersticeValue` directly, without building the tree (DESIGN §10.2).
//!
//! The bytes are exactly postcard's encoding of the equivalent
//! `IntersticeValue`, so the host decodes them as usual. `tests` checks that
//! for every shape.

use crate::{IntersticeAbiError, IntersticeValue};

/// A Rust value with a direct encoding. Implemented for the primitives,
/// `String`, `Vec<T>`, `Option<T>`, tuples and `IntersticeValue`, and
/// derived with `IntersticeType`.
pub trait ValueCodec: Sized {
    fn encode(&self, out: &mut Vec<u8>);
    fn decode(input: &mut &[u8]) -> Result<Self, IntersticeAbiError>;

    /// A `Vec<Self>`: tagged `Vec` with one value per item, except for `u8`
    /// (`Bytes`).
    #[doc(hidden)]
    fn encode_vec(items: &[Self], out: &mut Vec<u8>) {
        wire::put_tag(out, wire::VEC);
        wire::put_len(out, items.len());
        for item in items {
            item.encode(out);
        }
    }

    #[doc(hidden)]
    fn decode_vec(input: &mut &[u8]) -> Result<Vec<Self>, IntersticeAbiError> {
        wire::expect_tag(input, wire::VEC, "a list")?;
        let len = wire::take_len(input)?;
        let mut items = Vec::with_capacity(len.min(input.len()));
        for _ in 0..len {
            items.push(Self::decode(input)?);
        }
        Ok(items)
    }
}

/// The encoding's building blocks, shared with the derive.
#[doc(hidden)]
pub mod wire {
    use crate::IntersticeAbiError;

    // `IntersticeValue`'s variant indices, in declaration order.
    pub const VOID: u8 = 0;
    pub const BOOL: u8 = 1;
    pub const U8: u8 = 2;
    pub const U32: u8 = 3;
    pub const U64: u8 = 4;
    pub const I32: u8 = 5;
    pub const I64: u8 = 6;
    pub const F32: u8 = 7;
    pub const F64: u8 = 8;
    pub const STRING: u8 = 9;
    pub const BYTES: u8 = 10;
    pub const BLOB: u8 = 11;
    pub const VEC: u8 = 12;
    pub const OPTION: u8 = 13;
    pub const TUPLE: u8 = 14;
    pub const STRUCT: u8 = 15;
    pub const ENUM: u8 = 16;

    pub fn error(what: impl std::fmt::Display) -> IntersticeAbiError {
        IntersticeAbiError::ConversionError(what.to_string())
    }

    #[inline]
    pub fn put_tag(out: &mut Vec<u8>, tag: u8) {
        out.push(tag);
    }

    #[inline]
    pub fn put_varint(out: &mut Vec<u8>, mut v: u64) {
        while v >= 0x80 {
            out.push((v as u8) | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
    }

    #[inline]
    pub fn put_len(out: &mut Vec<u8>, len: usize) {
        put_varint(out, len as u64);
    }

    #[inline]
    pub fn zigzag(v: i64) -> u64 {
        ((v << 1) ^ (v >> 63)) as u64
    }

    #[inline]
    pub fn unzigzag(v: u64) -> i64 {
        ((v >> 1) as i64) ^ -((v & 1) as i64)
    }

    #[inline]
    pub fn take_byte(input: &mut &[u8]) -> Result<u8, IntersticeAbiError> {
        let (&b, rest) = input.split_first().ok_or_else(|| error("unexpected end of value"))?;
        *input = rest;
        Ok(b)
    }

    #[inline]
    pub fn take_bytes<'a>(input: &mut &'a [u8], n: usize) -> Result<&'a [u8], IntersticeAbiError> {
        if input.len() < n {
            return Err(error("unexpected end of value"));
        }
        let (head, rest) = input.split_at(n);
        *input = rest;
        Ok(head)
    }

    #[inline]
    pub fn take_varint(input: &mut &[u8]) -> Result<u64, IntersticeAbiError> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = take_byte(input)?;
            v |= ((b & 0x7f) as u64) << shift;
            if b < 0x80 {
                return Ok(v);
            }
        }
        Err(error("bad varint"))
    }

    #[inline]
    pub fn take_len(input: &mut &[u8]) -> Result<usize, IntersticeAbiError> {
        take_varint(input).map(|v| v as usize)
    }

    #[inline]
    pub fn expect_tag(input: &mut &[u8], tag: u8, what: &str) -> Result<(), IntersticeAbiError> {
        let found = take_byte(input)?;
        if found == tag { Ok(()) } else { Err(error(format_args!("expected {what}, found tag {found}"))) }
    }

    /// The `Struct` header of `count` fields.
    #[inline]
    pub fn put_struct(out: &mut Vec<u8>, count: usize) {
        put_tag(out, STRUCT);
        put_len(out, count);
    }

    pub fn expect_struct(input: &mut &[u8], count: usize, what: &str) -> Result<(), IntersticeAbiError> {
        expect_tag(input, STRUCT, what)?;
        let found = take_len(input)?;
        if found == count { Ok(()) } else { Err(error(format_args!("{what} has {count} fields, got {found}"))) }
    }

    /// The `Enum` header of `variant`.
    #[inline]
    pub fn put_enum(out: &mut Vec<u8>, variant: u32) {
        put_tag(out, ENUM);
        put_varint(out, variant as u64);
    }

    pub fn take_enum(input: &mut &[u8], what: &str) -> Result<u32, IntersticeAbiError> {
        expect_tag(input, ENUM, what)?;
        take_varint(input).map(|v| v as u32)
    }

    /// The `Tuple` header of `count` items.
    #[inline]
    pub fn put_tuple(out: &mut Vec<u8>, count: usize) {
        put_tag(out, TUPLE);
        put_len(out, count);
    }

    pub fn expect_tuple(input: &mut &[u8], count: usize) -> Result<(), IntersticeAbiError> {
        expect_tag(input, TUPLE, "a tuple")?;
        let found = take_len(input)?;
        if found == count { Ok(()) } else { Err(error(format_args!("expected a tuple of {count}, got {found}"))) }
    }

    pub fn expect_void(input: &mut &[u8]) -> Result<(), IntersticeAbiError> {
        expect_tag(input, VOID, "()")
    }
}

use wire::*;

impl ValueCodec for () {
    fn encode(&self, out: &mut Vec<u8>) {
        put_tag(out, VOID);
    }
    fn decode(input: &mut &[u8]) -> Result<Self, IntersticeAbiError> {
        expect_void(input)
    }
}

impl ValueCodec for bool {
    fn encode(&self, out: &mut Vec<u8>) {
        put_tag(out, BOOL);
        out.push(*self as u8);
    }
    fn decode(input: &mut &[u8]) -> Result<Self, IntersticeAbiError> {
        expect_tag(input, BOOL, "a bool")?;
        match take_byte(input)? {
            0 => Ok(false),
            1 => Ok(true),
            b => Err(error(format_args!("bad bool {b}"))),
        }
    }
}

impl ValueCodec for u8 {
    fn encode(&self, out: &mut Vec<u8>) {
        put_tag(out, U8);
        out.push(*self);
    }
    fn decode(input: &mut &[u8]) -> Result<Self, IntersticeAbiError> {
        expect_tag(input, U8, "a u8")?;
        take_byte(input)
    }
    fn encode_vec(items: &[Self], out: &mut Vec<u8>) {
        put_tag(out, BYTES);
        put_len(out, items.len());
        out.extend_from_slice(items);
    }
    /// `Bytes`, or a `Vec` of `U8` (as the CLI may send).
    fn decode_vec(input: &mut &[u8]) -> Result<Vec<Self>, IntersticeAbiError> {
        match take_byte(input)? {
            BYTES => {
                let len = take_len(input)?;
                Ok(take_bytes(input, len)?.to_vec())
            }
            VEC => {
                let len = take_len(input)?;
                (0..len).map(|_| u8::decode(input)).collect()
            }
            tag => Err(error(format_args!("expected bytes, found tag {tag}"))),
        }
    }
}

macro_rules! varint_codec {
    ($ty:ty, $tag:ident, $what:literal, unsigned) => {
        impl ValueCodec for $ty {
            #[inline]
            fn encode(&self, out: &mut Vec<u8>) {
                put_tag(out, $tag);
                put_varint(out, *self as u64);
            }
            #[inline]
            fn decode(input: &mut &[u8]) -> Result<Self, IntersticeAbiError> {
                expect_tag(input, $tag, $what)?;
                <$ty>::try_from(take_varint(input)?).map_err(|_| error(concat!("out of range for ", $what)))
            }
        }
    };
    ($ty:ty, $tag:ident, $what:literal, signed) => {
        impl ValueCodec for $ty {
            #[inline]
            fn encode(&self, out: &mut Vec<u8>) {
                put_tag(out, $tag);
                put_varint(out, zigzag(*self as i64));
            }
            #[inline]
            fn decode(input: &mut &[u8]) -> Result<Self, IntersticeAbiError> {
                expect_tag(input, $tag, $what)?;
                <$ty>::try_from(unzigzag(take_varint(input)?)).map_err(|_| error(concat!("out of range for ", $what)))
            }
        }
    };
}

varint_codec!(u32, U32, "a u32", unsigned);
varint_codec!(u64, U64, "a u64", unsigned);
varint_codec!(i32, I32, "an i32", signed);
varint_codec!(i64, I64, "an i64", signed);

macro_rules! float_codec {
    ($ty:ty, $tag:ident, $what:literal, $n:literal) => {
        impl ValueCodec for $ty {
            fn encode(&self, out: &mut Vec<u8>) {
                put_tag(out, $tag);
                out.extend_from_slice(&self.to_le_bytes());
            }
            fn decode(input: &mut &[u8]) -> Result<Self, IntersticeAbiError> {
                expect_tag(input, $tag, $what)?;
                Ok(<$ty>::from_le_bytes(take_bytes(input, $n)?.try_into().unwrap()))
            }
        }
    };
}

float_codec!(f32, F32, "an f32", 4);
float_codec!(f64, F64, "an f64", 8);

impl ValueCodec for String {
    fn encode(&self, out: &mut Vec<u8>) {
        put_tag(out, STRING);
        put_len(out, self.len());
        out.extend_from_slice(self.as_bytes());
    }
    fn decode(input: &mut &[u8]) -> Result<Self, IntersticeAbiError> {
        expect_tag(input, STRING, "a string")?;
        let len = take_len(input)?;
        let bytes = take_bytes(input, len)?;
        std::str::from_utf8(bytes).map(str::to_owned).map_err(|_| error("a string is not UTF-8"))
    }
}

impl<T: ValueCodec> ValueCodec for Vec<T> {
    fn encode(&self, out: &mut Vec<u8>) {
        T::encode_vec(self, out)
    }
    fn decode(input: &mut &[u8]) -> Result<Self, IntersticeAbiError> {
        T::decode_vec(input)
    }
}

impl<T: ValueCodec> ValueCodec for Option<T> {
    fn encode(&self, out: &mut Vec<u8>) {
        put_tag(out, OPTION);
        match self {
            None => out.push(0),
            Some(v) => {
                out.push(1);
                v.encode(out);
            }
        }
    }
    fn decode(input: &mut &[u8]) -> Result<Self, IntersticeAbiError> {
        expect_tag(input, OPTION, "an option")?;
        match take_byte(input)? {
            0 => Ok(None),
            1 => T::decode(input).map(Some),
            b => Err(error(format_args!("bad option {b}"))),
        }
    }
}

macro_rules! tuple_codec {
    ($n:literal; $($name:ident $idx:tt),+) => {
        impl<$($name: ValueCodec),+> ValueCodec for ($($name,)+) {
            fn encode(&self, out: &mut Vec<u8>) {
                put_tuple(out, $n);
                $(self.$idx.encode(out);)+
            }
            fn decode(input: &mut &[u8]) -> Result<Self, IntersticeAbiError> {
                expect_tuple(input, $n)?;
                Ok(($($name::decode(input)?,)+))
            }
        }
    };
}

tuple_codec!(2; A 0, B 1);
tuple_codec!(3; A 0, B 1, C 2);
tuple_codec!(4; A 0, B 1, C 2, D 3);
tuple_codec!(5; A 0, B 1, C 2, D 3, E 4);
tuple_codec!(6; A 0, B 1, C 2, D 3, E 4, F 5);
tuple_codec!(7; A 0, B 1, C 2, D 3, E 4, F 5, G 6);
tuple_codec!(8; A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);

/// Any value, through postcard.
impl ValueCodec for IntersticeValue {
    fn encode(&self, out: &mut Vec<u8>) {
        let encoded = postcard::to_extend(self, std::mem::take(out)).expect("values always encode");
        *out = encoded;
    }
    fn decode(input: &mut &[u8]) -> Result<Self, IntersticeAbiError> {
        let (value, rest) = postcard::take_from_bytes(input).map_err(error)?;
        *input = rest;
        Ok(value)
    }
}

/// `value` encoded alone.
pub fn encode_value<T: ValueCodec>(value: &T) -> Vec<u8> {
    let mut out = Vec::new();
    value.encode(&mut out);
    out
}

/// A `T` from `bytes`, which must hold exactly one value.
pub fn decode_value<T: ValueCodec>(mut bytes: &[u8]) -> Result<T, IntersticeAbiError> {
    let value = T::decode(&mut bytes)?;
    if bytes.is_empty() { Ok(value) } else { Err(error("trailing bytes after a value")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same bytes as postcard's encoding of the `IntersticeValue`, and back.
    fn check<T: ValueCodec + Clone + PartialEq + std::fmt::Debug + Into<IntersticeValue>>(value: T) {
        let direct = encode_value(&value);
        let tree: IntersticeValue = value.clone().into();
        assert_eq!(direct, postcard::to_allocvec(&tree).unwrap(), "{value:?}");
        assert_eq!(decode_value::<T>(&direct).unwrap(), value);
    }

    #[test]
    fn same_bytes_as_the_tree() {
        check(());
        check(true);
        check(7u8);
        check(0u32);
        check(u32::MAX);
        check(300u64);
        check(u64::MAX);
        check(-1i32);
        check(i32::MIN);
        check(i64::MIN);
        check(i64::MAX);
        check(1.5f32);
        check(-2.25f64);
        check(String::from("héllo"));
        check(vec![1u8, 2, 255]);
        check(Vec::<u8>::new());
        check(vec![1u32, 70000]);
        check(vec![String::from("a"), String::new()]);
        check(Some(5u64));
        check(None::<String>);
        check((1u32, String::from("x")));
        check((1u8, -2i64, Some(true)));
        check(vec![(1u32, vec![2u8])]);
    }

    #[test]
    fn bytes_accept_a_list_of_u8() {
        let tree = IntersticeValue::Vec(vec![IntersticeValue::U8(1), IntersticeValue::U8(2)]);
        let bytes = postcard::to_allocvec(&tree).unwrap();
        assert_eq!(decode_value::<Vec<u8>>(&bytes).unwrap(), vec![1, 2]);
    }

    #[test]
    fn wrong_or_short_input_is_an_error() {
        assert!(decode_value::<u32>(&encode_value(&5u64)).is_err());
        assert!(decode_value::<String>(&[STRING, 5, b'a']).is_err());
        assert!(decode_value::<u32>(&[U32, 0xff]).is_err());
        assert!(decode_value::<u8>(&[U8, 1, 2]).is_err());
    }

    #[test]
    fn tree_values_pass_through() {
        let v = IntersticeValue::Struct(vec![IntersticeValue::U32(1), IntersticeValue::String("a".into())]);
        let mut out = vec![9];
        v.encode(&mut out);
        let mut input = &out[1..];
        assert_eq!(IntersticeValue::decode(&mut input).unwrap(), v);
        assert!(input.is_empty());
    }
}
