interstice_module!();

use crate::bindings::{HasHelloContext, HasMyNodeHandle, *};
use interstice_sdk::*;

#[reducer(on = "init")]
fn caller(ctx: ReducerContext) {
    ctx.log("Calling remote hello...");
    // ctx.hello().reducers.hello("called from caller".to_string());
    ctx.mynode().hello().reducers.hello("Client !".to_string());
    ctx.log("hello remote called !");

    // ctx.log("Calling local hello...");
    // ctx.hello().reducers.hello("called from caller".to_string());
    // ctx.log("hello local called !");
}
