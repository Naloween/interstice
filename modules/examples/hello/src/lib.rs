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

// TIME TRAVEL (DESIGN §6.8): a table with a timeline keeps a tree of its
// commits; reducers can read past states and travel the table back and forth.

#[table(public, timeline = 64)]
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

#[reducer]
pub fn undo<Caps>(ctx: ReducerContext<Caps>) -> Result<(), String>
where
    Caps: CanRead<Notes> + CanInsert<Notes> + CanUpdate<Notes> + CanDelete<Notes>,
{
    let to = ctx.current.tables.notes().travel(TimelinePoint::Back(1))?;
    ctx.log(&format!("undo: notes -> commit {to}"));
    Ok(())
}

#[reducer]
pub fn redo<Caps>(ctx: ReducerContext<Caps>) -> Result<(), String>
where
    Caps: CanRead<Notes> + CanInsert<Notes> + CanUpdate<Notes> + CanDelete<Notes>,
{
    let to = ctx.current.tables.notes().travel(TimelinePoint::Forward(1))?;
    ctx.log(&format!("redo: notes -> commit {to}"));
    Ok(())
}

/// Travel to a commit id ("0" is the base); `truncate` drops the future.
#[reducer]
pub fn rewind<Caps>(ctx: ReducerContext<Caps>, commit: String, truncate: String) -> Result<(), String>
where
    Caps: CanRead<Notes> + CanInsert<Notes> + CanUpdate<Notes> + CanDelete<Notes>,
{
    let commit: u64 = commit.parse().map_err(|e| format!("bad commit: {e}"))?;
    let mode = if truncate == "truncate" { TravelMode::Truncate } else { TravelMode::Branch };
    let to = ctx.current.tables.notes().travel_with(TimelinePoint::Commit(commit), mode)?;
    ctx.log(&format!("rewind: notes -> commit {to}"));
    Ok(())
}

/// Log the timeline, the current notes and the notes one commit back.
#[reducer]
pub fn show_notes<Caps>(ctx: ReducerContext<Caps>) -> Result<(), String>
where
    Caps: CanRead<Notes>,
{
    let notes = ctx.current.tables.notes();
    let info = notes.timeline()?;
    let tree: Vec<String> = info.nodes.iter().map(|n| format!("{}<-{}", n.id, n.parent)).collect();
    ctx.log(&format!("timeline head {} nodes [{}]", info.head, tree.join(" ")));
    let text = |rows: Vec<Notes>| rows.into_iter().map(|n| format!("{}:{}", n.id, n.text)).collect::<Vec<_>>().join(", ");
    ctx.log(&format!("notes now: [{}]", text(notes.scan()?)));
    ctx.log(&format!("notes one back: [{}]", text(notes.rows_at(TimelinePoint::Back(1))?)));
    Ok(())
}

/// One commit of the `Notes` timeline (`parent` is 0 for children of the base).
#[interstice_type]
#[derive(Debug)]
pub struct NoteCommit {
    pub id: u64,
    pub parent: u64,
}

/// The `Notes` commit tree, for front-ends such as the `notes` app (§12.2).
#[interstice_type]
#[derive(Debug)]
pub struct NoteHistory {
    /// The commit the table is at (`0` = the base).
    pub head: u64,
    /// Every commit, by increasing id.
    pub commits: Vec<NoteCommit>,
}

#[query]
fn note_history<Caps>(ctx: QueryContext<Caps>) -> Result<NoteHistory, String>
where
    Caps: CanRead<Notes>,
{
    let info = ctx.current.tables.notes().timeline()?;
    Ok(NoteHistory {
        head: info.head,
        commits: info.nodes.into_iter().map(|n| NoteCommit { id: n.id, parent: n.parent }).collect(),
    })
}
