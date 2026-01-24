//! Typed reducer support
//!
//! This module provides utilities for type-safe reducer definitions and calls.

use crate::types::Serialize;
use interstice_abi::IntersticeValue;

/// Type signature for a reducer function
/// 
/// Specifies the argument type and return type of a reducer in a type-safe way.
#[derive(Debug, Clone)]
pub struct ReducerSignature<In: Serialize, Out: Serialize> {
    module: String,
    name: String,
    _phantom_in: std::marker::PhantomData<In>,
    _phantom_out: std::marker::PhantomData<Out>,
}

impl<In: Serialize, Out: Serialize> ReducerSignature<In, Out> {
    /// Create a new reducer signature
    pub fn new(module: &str, name: &str) -> Self {
        Self {
            module: module.to_string(),
            name: name.to_string(),
            _phantom_in: std::marker::PhantomData,
            _phantom_out: std::marker::PhantomData,
        }
    }

    /// Get the module name
    pub fn module(&self) -> &str {
        &self.module
    }

    /// Get the reducer name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Call this reducer with typed arguments and receive typed result
    pub fn call(&self, arg: In) -> std::result::Result<Out, String> {
        #[cfg(target_arch = "wasm32")]
        {
            use crate::host_calls;
            let arg_value = arg.to_value();
            let result_value = host_calls::call_reducer(
                self.module.clone(),
                self.name.clone(),
                arg_value,
            );
            Out::from_value(result_value)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Err("Reducer calls not available outside WASM".to_string())
        }
    }
}

/// A reducer handler for incoming reducer calls
/// 
/// Type-safe wrapper for reducer function signatures.
pub trait TypedReducer<In: Serialize, Out: Serialize> {
    /// Handle a typed reducer invocation
    fn handle(&self, arg: In) -> std::result::Result<Out, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reducer_signature_creation() {
        let sig: ReducerSignature<String, u64> = 
            ReducerSignature::new("math", "count_chars");
        assert_eq!(sig.module(), "math");
        assert_eq!(sig.name(), "count_chars");
    }

    #[test]
    fn test_reducer_signature_with_numbers() {
        let sig: ReducerSignature<u64, u64> = 
            ReducerSignature::new("calc", "double");
        assert_eq!(sig.module(), "calc");
        assert_eq!(sig.name(), "double");
    }

    #[test]
    fn test_reducer_signature_clone() {
        let sig1: ReducerSignature<String, u32> = 
            ReducerSignature::new("test", "func");
        let sig2 = sig1.clone();
        assert_eq!(sig1.module(), sig2.module());
        assert_eq!(sig1.name(), sig2.name());
    }

    // Example typed reducer implementation
    struct CountWordsReducer;

    impl TypedReducer<String, u64> for CountWordsReducer {
        fn handle(&self, input: String) -> std::result::Result<u64, String> {
            Ok(input.split_whitespace().count() as u64)
        }
    }

    #[test]
    fn test_typed_reducer_implementation() {
        let reducer = CountWordsReducer;
        let result = reducer.handle("hello world test".to_string()).unwrap();
        assert_eq!(result, 3);
    }
}
