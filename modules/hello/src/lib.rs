use interstice_sdk::*;

interstice_module!(Some(Authority::Render));

// TABLES

#[table]
#[derive(Debug)]
pub struct Greetings {
    #[primary_key]
    pub id: u64,
    pub greeting: String,
    pub custom: TestCustomType,
}

#[derive(Debug, Clone, IntersticeType)]
pub struct TestCustomType {
    pub val: u32,
}

// REDUCERS
#[reducer(on = init)]
pub fn init(ctx: ReducerContext) {
    ctx.log("Hello world !");
}

#[reducer]
pub fn hello(ctx: ReducerContext, name: String) {
    ctx.log(&format!("Saying hello to {}", name));
    ctx.current.greetings().insert(Greetings {
        id: 0,
        greeting: format!("Hello, {}!", name),
        custom: TestCustomType { val: 0 },
    });
}

#[reducer(on = hello.greetings.insert)]
fn on_greeting_insert(ctx: ReducerContext, inserted_row: Row) {
    ctx.log(&format!("Inserted greeting: {:?}", inserted_row));
}
