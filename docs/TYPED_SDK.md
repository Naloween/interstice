# Typed SDK System - Phase 3.1 Implementation

## Overview

The Typed SDK System provides strong typing for Interstice module development. Instead of working with raw `IntersticeValue`, module developers can now use Rust's type system to catch errors at compile time.

## Key Components

### 1. Serialize Trait

The `Serialize` trait bridges Rust types and IntersticeValue:

```rust
pub trait Serialize: Sized + Debug + Clone {
    fn from_value(v: IntersticeValue) -> std::result::Result<Self, String>;
    fn to_value(&self) -> IntersticeValue;
}
```

**Built-in implementations:**
- `String`, `u64`, `u32`, `i64`, `i32`, `bool`, `f32`, `f64`

### 2. Derive Macros

Two derive macros make custom types easy:

#### `#[derive(Serialize)]` - Basic implementation
```rust
#[derive(Debug, Clone, Serialize)]
struct MyType {
    // ...
}
```

#### `#[derive(SerializeNewtype)]` - Newtype wrapper
```rust
#[derive(Debug, Clone, SerializeNewtype)]
struct UserId(u64);  // Automatically impl Serialize
```

### 3. Typed Helpers

Simplify table operations:

```rust
// Type-safe insert
insert_typed_row("users", 1u64, user_data)?;

// Type-safe scan
let users: Vec<UserData> = scan_typed("users");
```

### 4. TypedReducerContext

Type-safe reducer calls:

```rust
let result: u32 = TypedReducerContext::call_reducer::<String, u32>(
    "math",
    "add",
    "5".to_string()
)?;
```

## Usage Example

### Before (Raw Types)
```rust
#[reducer]
fn create_user(name: String) {
    interstice_sdk::insert_row(
        "users".to_string(),
        Row {
            primary_key: IntersticeValue::U64(1),
            entries: vec![IntersticeValue::String(name)],
        },
    );
}
```

### After (Typed SDK)
```rust
#[reducer]
fn create_user(name: String) {
    insert_typed_row("users", 1u64, name).unwrap();
}
```

## Benefits

1. **Type Safety**: Compile-time checking of types
2. **Ergonomics**: No raw IntersticeValue handling needed
3. **Custom Types**: Easy to implement Serialize for custom types
4. **Extensibility**: Derive macros for boilerplate
5. **Clear Intent**: Type signatures document expectations

## Testing

All components include comprehensive tests:
- Type conversion roundtrips
- Error handling
- Custom type examples
- Numeric type coverage

Run tests:
```bash
cargo test -p interstice-sdk-core types
```

## Future Work (Phase 3.2-3.5)

1. **Table Macro Integration**: Automatic TableHandle generation
2. **Reducer Type Signatures**: Full typed reducer arguments
3. **Subscription Type Safety**: Type-checked events
4. **Advanced Derive**: Multi-field struct handling

## Files

- `interstice-sdk-core/src/types.rs` - Serialize trait & implementations
- `interstice-sdk-core/src/typed_context.rs` - TypedReducerContext
- `interstice-sdk-core/src/typed_helpers.rs` - Helper functions
- `interstice-sdk-core/src/typed_examples.rs` - Usage examples
- `interstice-sdk-macros/src/lib.rs` - Derive macro implementations
