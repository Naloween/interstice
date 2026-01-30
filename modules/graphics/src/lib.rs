use interstice_sdk::*;

interstice_module!(Some(Authority::Input));

// TABLES

// REDUCERS
#[reducer(on = "init")]
pub fn init(ctx: ReducerContext) {
    ctx.log("Hello world !");
}

#[reducer]
pub fn on_input(ctx: ReducerContext, event: InputEvent) {
    ctx.log(&format!("On event"));
}
