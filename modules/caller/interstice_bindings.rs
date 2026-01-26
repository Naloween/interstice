
pub struct HelloContext
{
    pub tables: HelloTables,
    pub reducers: HelloReducers,
}
pub struct HelloTables{}
pub struct HelloReducers{}
impl HelloReducers{

    pub fn hello(&self, name: String){
        interstice_sdk::host_calls::call_reducer(
            ModuleSelection::Other("hello".into()),
            "hello".to_string(),
            IntersticeValue::Vec(vec![name.into()]),
        );
    }

    pub fn on_greeting_insert(&self, inserted_row: Row){
        interstice_sdk::host_calls::call_reducer(
            ModuleSelection::Other("hello".into()),
            "on_greeting_insert".to_string(),
            IntersticeValue::Vec(vec![inserted_row.into()]),
        );
    }

}

pub struct Greetings{
    id: u64,
    greeting: String,

}
pub struct GreetingsHandle{}

impl Into<interstice_sdk::Row> for Greetings {
    fn into(self) -> interstice_sdk::Row{
        Row {
            primary_key: self.id.into(),
            entries: vec![self.greeting.clone().into()],
        }
    }
}

impl Into<Greetings> for interstice_sdk::Row {
    fn into(self) -> Greetings{
        let mut row_entries = self.entries.into_iter();
        Greetings {
            id: self.primary_key.into(), // convert IntersticeValue → PK type
            greeting: row_entries.next().unwrap().into(),

        }
    }
}

impl GreetingsHandle{
    pub fn insert(&self, row: Greetings){
        interstice_sdk::host_calls::insert_row(
            ModuleSelection::Current,
            "greetings".to_string(),
            row.into(),
        );
    }

    pub fn scan(&self) -> Vec<Greetings>{
        interstice_sdk::host_calls::scan(interstice_sdk::ModuleSelection::Current, "greetings".to_string()).into_iter().map(|x| x.into()).collect()
    }
}

pub trait HasGreetingsHandle {
    fn greetings(&self) -> GreetingsHandle;
}

impl HasGreetingsHandle for HelloTables {
    fn greetings(&self) -> GreetingsHandle {
        return GreetingsHandle {}
    }
}


pub trait HasHelloContext {
    fn hello(&self) -> HelloContext;
}

impl HasHelloContext for interstice_sdk::ReducerContext {
    fn hello(&self) -> HelloContext {
        return HelloContext {
                tables: HelloTables{},
 reducers: HelloReducers{},
}
    }
}
