interstice_module!();

use crate::bindings::{HasHelloContext, HasMyNodeHandle, *};
use interstice_sdk::*;

#[reducer(on = "init")]
fn caller(ctx: ReducerContext) {
    ctx.log("Calling remote hello...");
    if let Err(err) = ctx.mynode().hello().reducers.hello("Client !".to_string()) {
        ctx.log(&format!("Failed to call remote hello: {}", err));
        return;
    }
    ctx.log("hello remote called !");

    // ctx.log("Calling local hello...");
    // ctx.hello().reducers.hello("called from caller".to_string());
    // ctx.log("hello local called !");

    // ctx.log(&format!(
    //     "Caller received all greetings: {:?}",
    //     ctx.mynode()
    //         .hello()
    //         .queries
    //         .get_greetings()
    //         .into_iter()
    //         .map(|f| f.greeting)
    //         .collect::<Vec<_>>()
    // ));
}

#[reducer(on = "MyNode.hello.greetings.insert")]
fn on_insert_greetings(ctx: ReducerContext, inserted_row: Greetings) {
    ctx.log(&format!(
        "Caller received new greeting: {:?}",
        inserted_row.greeting
    ));
}
