#[derive(Debug, Clone)]
pub enum AuthorityEntry {
    Gpu {
        module_name: String,
        render_reducer: Option<String>,
    },
    Audio {
        module_name: String,
        output_reducer: Option<String>,
        input_reducer: Option<String>,
    },
    Input {
        module_name: String,
        input_reducer: Option<String>,
    },
    File {
        module_name: String,
    },
    Module {
        module_name: String,
    },
}

impl AuthorityEntry {
    pub fn module_name(&self) -> &str {
        match self {
            AuthorityEntry::Gpu { module_name, .. }
            | AuthorityEntry::Audio { module_name, .. }
            | AuthorityEntry::Input { module_name, .. }
            | AuthorityEntry::File { module_name }
            | AuthorityEntry::Module { module_name } => module_name,
        }
    }
}
