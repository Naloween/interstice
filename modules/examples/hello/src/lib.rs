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
#[derive(Debug)]
pub struct TestCustomType {
    pub val: u32,
}

// REDUCERS
#[reducer(on = "load")]
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
pub fn hello<Caps>(ctx: ReducerContext<Caps>, name: String)
where
    Caps: CanInsert<Greetings>,
{
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

#[reducer(on = "greetings.insert")]
fn on_greeting_insert(ctx: ReducerContext, inserted_row: Greetings) {
    ctx.log(&format!("Inserted greeting: {:?}", inserted_row));
}

#[query]
fn get_greetings<Caps>(ctx: QueryContext<Caps>) -> Result<Vec<Greetings>, String>
where
    Caps: CanRead<Greetings>,
{
    ctx.current.tables.greetings().scan()
}

// TIME TRAVEL (DESIGN §6.7): a logged table's log is its history. Reducers
// can list its commits, read it at any past point and restore it there.

#[table(public)]
#[derive(Debug)]
pub struct Notes {
    #[primary_key]
    pub id: u64,
    pub text: String,
}

/// Write note `id` (insert or overwrite).
#[reducer]
pub fn note<Caps>(ctx: ReducerContext<Caps>, id: String, text: String) -> Result<(), String>
where
    Caps: CanRead<Notes> + CanInsert<Notes> + CanUpdate<Notes>,
{
    let id: u64 = id.parse().map_err(|e| format!("bad id: {e}"))?;
    let notes = ctx.current.tables.notes();
    match notes.get(id)? {
        Some(_) => notes.update(Notes { id, text }),
        None => notes.insert(Notes { id, text }).map(|_| ()),
    }
}

#[reducer]
pub fn erase<Caps>(ctx: ReducerContext<Caps>, id: String) -> Result<(), String>
where
    Caps: CanDelete<Notes>,
{
    let id: u64 = id.parse().map_err(|e| format!("bad id: {e}"))?;
    ctx.current.tables.notes().delete(id)
}

/// How many commits of `Notes` the history views look at.
const HISTORY_COMMITS: u32 = 1024;

/// Restore `Notes` to the parent of the current position.
#[reducer]
pub fn undo<Caps>(ctx: ReducerContext<Caps>) -> Result<(), String>
where
    Caps: CanRead<Notes> + CanInsert<Notes> + CanUpdate<Notes> + CanDelete<Notes>,
{
    let notes = ctx.current.tables.notes();
    let tree = note_tree(notes.commits(HISTORY_COMMITS)?);
    let Some(parent) = tree.commits.iter().find(|c| c.id == tree.head).map(|c| c.parent) else {
        return Ok(()); // already at the start
    };
    let to = notes.restore(HistoryPoint::Tx(parent))?;
    ctx.log(&format!("undo: notes -> commit {to}"));
    Ok(())
}

/// Restore `Notes` to the newest child of the current position.
#[reducer]
pub fn redo<Caps>(ctx: ReducerContext<Caps>) -> Result<(), String>
where
    Caps: CanRead<Notes> + CanInsert<Notes> + CanUpdate<Notes> + CanDelete<Notes>,
{
    let notes = ctx.current.tables.notes();
    let tree = note_tree(notes.commits(HISTORY_COMMITS)?);
    let Some(child) = tree.commits.iter().filter(|c| c.parent == tree.head).map(|c| c.id).max() else {
        return Ok(()); // nothing to redo
    };
    let to = notes.restore(HistoryPoint::Tx(child))?;
    ctx.log(&format!("redo: notes -> commit {to}"));
    Ok(())
}

/// Restore `Notes` to commit `commit` ("0": the empty start).
#[reducer]
pub fn restore<Caps>(ctx: ReducerContext<Caps>, commit: String) -> Result<(), String>
where
    Caps: CanRead<Notes> + CanInsert<Notes> + CanUpdate<Notes> + CanDelete<Notes>,
{
    let commit: u64 = commit.parse().map_err(|e| format!("bad commit: {e}"))?;
    let to = ctx.current.tables.notes().restore(HistoryPoint::Tx(commit))?;
    ctx.log(&format!("restore: notes -> commit {to}"));
    Ok(())
}

/// Log the history, the current notes and the notes one commit back.
#[reducer]
pub fn show_notes<Caps>(ctx: ReducerContext<Caps>) -> Result<(), String>
where
    Caps: CanRead<Notes>,
{
    let notes = ctx.current.tables.notes();
    let tree = note_tree(notes.commits(HISTORY_COMMITS)?);
    let nodes: Vec<String> = tree.commits.iter().map(|c| format!("{}<-{}", c.id, c.parent)).collect();
    ctx.log(&format!("history at {} [{}]", tree.head, nodes.join(" ")));
    let text = |rows: Vec<Notes>| rows.into_iter().map(|n| format!("{}:{}", n.id, n.text)).collect::<Vec<_>>().join(", ");
    ctx.log(&format!("notes now: [{}]", text(notes.scan()?)));
    ctx.log(&format!("notes one commit back: [{}]", text(notes.rows_at(HistoryPoint::Back(1))?)));
    Ok(())
}

/// An ordinary (non-restore) commit of `Notes` and its parent: the position
/// just before it (`0` = the empty start).
#[interstice_type]
#[derive(Debug)]
pub struct NoteCommit {
    pub id: u64,
    pub parent: u64,
}

/// The tree of `Notes` commits, for front-ends such as the `notes` app.
#[interstice_type]
#[derive(Debug)]
pub struct NoteHistory {
    /// The position: the commit the table's state is at (`0` = start).
    pub head: u64,
    /// Every ordinary commit, oldest first.
    pub commits: Vec<NoteCommit>,
}

/// Derive the tree from the linear log (DESIGN §6.7): a restore moves the
/// position, an ordinary commit becomes a child of the position.
fn note_tree(newest_first: Vec<HistoryCommit>) -> NoteHistory {
    let mut head = 0;
    let mut commits = Vec::new();
    for c in newest_first.into_iter().rev() {
        match c.restored_to {
            Some(to) => head = to,
            None => {
                commits.push(NoteCommit { id: c.tx, parent: head });
                head = c.tx;
            }
        }
    }
    NoteHistory { head, commits }
}

#[query]
fn note_history<Caps>(ctx: QueryContext<Caps>) -> Result<NoteHistory, String>
where
    Caps: CanRead<Notes>,
{
    Ok(note_tree(ctx.current.tables.notes().commits(HISTORY_COMMITS)?))
}
