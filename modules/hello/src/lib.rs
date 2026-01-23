use interstice_sdk::{interstice_module, reducer, table, IntersticeValue, Row};

interstice_module!();

// TABLES

#[table]
pub struct Greetings {
    #[primary_key]
    pub id: u64,
    pub greeting: String,
}

// REDUCERS

#[reducer]
pub fn hello(name: String) {
    interstice_sdk::log(&format!("Saying hello to {}", name));
    interstice_sdk::insert_row(
        "greetings".to_string(),
        Row {
            primary_key: IntersticeValue::U64(0), // Auto-incremented by the SDK
            entries: vec![IntersticeValue::String(format!("Hello, {}!", name))],
        },
    );
}

#[reducer(on = hello.greetings.insert)]
fn on_greeting_insert(inserted_row: Row) {
    interstice_sdk::log(&format!("Inserted greeting: {:?}", inserted_row));

    let greetings = interstice_sdk::scan("greetings".into());
    interstice_sdk::log(&format!("All greetings: {:?}", greetings));
}
