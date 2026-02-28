use interstice_sdk::*;

interstice_module!(visibility: Public);

// TABLES

#[table(public)]
#[derive(Debug)]
pub struct Greetings {
    #[primary_key(auto_inc)]
    pub id: u64,
    #[index(btree, unique)]
    pub greeting: String,
    pub custom: TestCustomType,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct TestCustomType {
    pub val: u32,
}

// REDUCERS
#[reducer(on = "init")]
pub fn init(ctx: ReducerContext) {
    ctx.log("Hello world !");

    ctx.schedule("scheduled", 1000)
        .expect("Couldn't schedule reducer");
}

#[reducer(on = "connect")]
pub fn on_connect(ctx: ReducerContext, node_id: String) {
    ctx.log(&format!("Node connected: {}", node_id));
}

#[reducer(on = "disconnect")]
pub fn on_disconnect(ctx: ReducerContext, node_id: String) {
    ctx.log(&format!("Node disconnected: {}", node_id));
}

#[reducer]
pub fn scheduled(ctx: ReducerContext) {
    ctx.log("Scheduled reducer called");
}

#[reducer]
pub fn hello(ctx: ReducerContext, name: String) {
    ctx.log(&format!("Saying hello to {}", name));
    match ctx.current.tables.greetings().insert(Greetings {
        id: 0,
        greeting: format!("Hello, {}!", name),
        custom: TestCustomType { val: 0 },
    }) {
        Ok(_) => (),
        Err(err) => ctx.log(&format!("Failed to insert greeting: {:?}", err)),
    }
}

#[reducer(on = "hello.greetings.insert")]
fn on_greeting_insert(ctx: ReducerContext, inserted_row: Greetings) {
    ctx.log(&format!("Inserted greeting: {:?}", inserted_row));
}

#[query]
fn get_greetings(ctx: QueryContext) -> Vec<Greetings> {
    ctx.current.tables.greetings().scan().unwrap_or_else(|err| {
        ctx.log(&format!("Failed to scan greetings: {}", err));
        vec![]
    })
}
