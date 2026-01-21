#[derive(Debug, Clone)]
pub struct TableSchema {
    pub name: String,
    pub visibility: TableVisibility,
    // schema details will come later
}

#[derive(Debug, Clone)]
pub enum TableVisibility {
    Public,
    Private,
}
