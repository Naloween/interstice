use interstice_sdk::*;

interstice_module!(visibility: Public, authorities: [Module]);

#[table]
pub struct Module {
    #[primary_key(auto_inc)]
    id: u64,
    bin: Vec<u8>,
    icon: Option<Vec<u8>>,
}

#[reducer(on = "module_publish")]
fn on_publish(ctx: ReducerContext) {}

#[reducer(on = "module_remove")]
fn on_remove(ctx: ReducerContext) {}

#[reducer]
fn load(ctx: ReducerContext, wasm_binary: Vec<u8>) {
    if let Err(err) = ctx.module().publish(NodeSelection::Current, wasm_binary) {
        ctx.log(&err);
    }
}
