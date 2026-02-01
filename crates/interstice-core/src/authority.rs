#[derive(Debug, Clone)]
pub struct AuthorityEntry {
    pub module_name: String,
    pub on_event_reducer_name: Option<String>,
}
