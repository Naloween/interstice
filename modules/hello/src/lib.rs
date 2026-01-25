use interstice_sdk::*;

interstice_module!();

// TABLES

#[table]
#[derive(Debug)]
pub struct Greetings {
    #[primary_key]
    pub id: u64,
    pub greeting: String,
}

// REDUCERS

#[reducer]
pub fn hello(ctx: ReducerContext, name: String) {
    ctx.log(&format!("Saying hello to {}", name));
    ctx.current.greetings().insert(Greetings {
        id: 0,
        greeting: format!("Hello, {}!", name),
    });
}

#[reducer(on = hello.greetings.insert)]
fn on_greeting_insert(ctx: ReducerContext, inserted_row: Row) {
    ctx.log(&format!("Inserted greeting: {:?}", inserted_row));

    let greetings = ctx.current.greetings().scan();
    ctx.log(&format!("All greetings: {:?}", greetings));
}
