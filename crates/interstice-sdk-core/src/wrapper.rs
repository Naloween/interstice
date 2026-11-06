//! Helpers for the generated reducer and query wrappers (DESIGN §10.2).

use interstice_abi::{ValueCodec, wire};
use serde::de::DeserializeOwned;

/// Split a call's input, `(context, IntersticeValue::Vec(args))`, into the
/// context and the encoded arguments, checking there are `count` of them.
pub fn split_input<C: DeserializeOwned>(bytes: &[u8], count: usize) -> (C, &[u8]) {
    let (context, mut rest) =
        postcard::take_from_bytes::<C>(bytes).unwrap_or_else(|e| panic!("bad call context: {e}"));
    let header = wire::expect_tag(&mut rest, wire::VEC, "the argument list").and_then(|_| wire::take_len(&mut rest));
    match header {
        Ok(n) if n == count => (context, rest),
        Ok(n) => panic!("expected {count} arguments, got {n}"),
        Err(e) => panic!("bad arguments: {e}"),
    }
}

/// The next argument.
pub fn arg<T: ValueCodec>(input: &mut &[u8], name: &str) -> T {
    T::decode(input).unwrap_or_else(|e| panic!("argument `{name}`: {e}"))
}

/// A query's result, in memory the host frees.
pub fn return_value<T: ValueCodec>(value: &T, alloc: extern "C" fn(i32) -> i32) -> i64 {
    let mut bytes = Vec::new();
    value.encode(&mut bytes);
    let ptr = alloc(bytes.len() as i32);
    // SAFETY: `alloc` returned `bytes.len()` writable bytes.
    unsafe { std::slice::from_raw_parts_mut(ptr as *mut u8, bytes.len()).copy_from_slice(&bytes) };
    interstice_abi::pack_ptr_len(ptr, bytes.len() as i32)
}
