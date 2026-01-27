use interstice_sdk::*;

interstice_module!();

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

#[reducer]
pub fn hello(ctx: ReducerContext, name: String) {
    ctx.log(&format!("Saying hello to {}", name));
    ctx.current.greetings().insert(Greetings {
        id: 0,
        greeting: format!("Hello, {}!", name),
        custom: TestCustomType { val: 0 },
    });

    let test = TestCustomType { val: 0 };
    let test_interstice_val = Into::<IntersticeValue>::into(test.clone());
    let test2: TestCustomType = test_interstice_val.clone().into();

    ctx.log(&format!("Test custom type: {:?}", &test));
    ctx.log(&format!(
        "Test custom type interstice_value: {:?}",
        &test_interstice_val
    ));
    ctx.log(&format!("Test custom type back: {:?}", &test2));
}

#[reducer(on = hello.greetings.insert)]
fn on_greeting_insert(ctx: ReducerContext, inserted_row: Row) {
    ctx.log(&format!("Inserted greeting: {:?}", inserted_row));

    let greetings = ctx.current.greetings().scan();
    ctx.log(&format!("All greetings: {:?}", greetings));
}
