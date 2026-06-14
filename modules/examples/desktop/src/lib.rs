use crate::bindings::module_manager::*;
use interstice_sdk::*;

interstice_module!(visibility: Public);

// The desktop bakes its apps in directly. For this first vertical slice we ship
// the `hello` example and ask the module manager to load it at startup. Apps
// cannot be handed to a reducer over the CLI (bytes can't ride a string arg), so
// baking them in is the way the desktop owns its app set.
const HELLO_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../crates/interstice-cli/module_examples/hello_example.wasm"
));

#[reducer(on = "load")]
pub fn on_load(ctx: ReducerContext) {
    ctx.log("desktop: loading 'hello' app via module_manager");
    let mm = ctx.module_manager();
    let _ = mm
        .reducers
        .load("hello".to_string(), HELLO_BYTES.to_vec(), None);
}

#[reducer]
pub fn unload(ctx: ReducerContext) {
    ctx.log("desktop: unloading 'hello' app via module_manager");
    let mm = ctx.module_manager();
    let _ = mm.reducers.unload_app("hello".to_string());
}
