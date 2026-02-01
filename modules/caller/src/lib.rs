interstice_module!();

use interstice_sdk::*;

#[reducer]
fn caller(ctx: ReducerContext) {
    ctx.log("Calling hello...");
    // ctx.hello().reducers.hello("called from caller".to_string());
    ctx.log("hello called !");
}
