interstice_module!(replicated_tables: [
    "hello-example.hello-example.greetings",
]);

use crate::bindings::{
    HasHelloExampleHandle,
    hello_example::{
        hello_example::{Greetings, HasGreetingsHandle},
        *,
    },
};
use interstice_sdk::*;

#[reducer(on = "load")]
fn caller(ctx: ReducerContext) {
    ctx.log("Calling remote hello...");
    if let Err(err) = ctx
        .hello_example()
        .hello_example()
        .reducers
        .hello("Client !".to_string())
    {
        ctx.log(&format!("Failed to call remote hello: {}", err));
        return;
    }
    ctx.log("hello remote called !");
}

#[reducer(
    on = "hello-example.hello-example.greetings.insert",
    reads = ["hello-example.hello-example.greetings"]
)]
fn on_insert_greetings(ctx: ReducerContext, inserted_row: Greetings) {
    ctx.log(&format!(
        "Caller received new greeting: {:?}",
        inserted_row.greeting
    ));

    for greeting in ctx
        .hello_example()
        .hello_example()
        .tables
        .greetings()
        .scan()
    {
        ctx.log(&format!("All hello greetings: {}", greeting.greeting));
    }
}
