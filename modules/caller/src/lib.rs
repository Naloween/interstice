// REDUCERS

use interstice_sdk::{host_calls::call_reducer, *};

interstice_module!();

#[reducer]
fn caller(ctx: ReducerContext) {
    ctx.log("Calling hello...");
    call_reducer(
        ModuleSelection::Other("hello".into()),
        "hello".to_string(),
        IntersticeValue::Vec(vec![IntersticeValue::String(
            "called from caller".to_string(),
        )]),
    );
    ctx.log("hello called !");
}
