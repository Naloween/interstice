use interstice_sdk::*;

interstice_module!(visibility: Public, authorities: [Module]);

// ── Registry ───────────────────────────────────────────────────────────────────
//
// The module manager keeps a registry of "apps" (modules) known to this node.
// An app may be registered without being loaded; loading it instantiates the
// module on the current node via the Module authority. This is the base the
// desktop module builds on to install, launch, and stop apps.

/// A registered app. The wasm bytes live in [`AppBinary`] (same id) so that
/// listing apps, which a launcher does every frame, copies no binary.
#[table]
pub struct App {
    #[primary_key(auto_inc)]
    id: u64,
    #[index(hash, unique)]
    name: String,
    icon: Option<Vec<u8>>,
    /// FNV-1a of the binary: re-registering the same bytes rewrites nothing.
    hash: u64,
    loaded: bool,
}

#[table]
pub struct AppBinary {
    #[primary_key]
    id: u64,
    bin: Vec<u8>,
}

/// Outcome of a `load_app` (DESIGN §12.2): loads compile in the background,
/// so the result arrives later. `module` is empty when the binary could not
/// be compiled.
#[table(public, event = 32)]
#[derive(Debug)]
pub struct LoadReport {
    #[primary_key(auto_inc)]
    pub seq: u64,
    pub module: String,
    pub ok: bool,
    pub message: String,
}

/// Public, lightweight view of a registered app (without the wasm bytes).
#[interstice_type]
pub struct AppInfo {
    pub id: u64,
    pub name: String,
    pub icon: Option<Vec<u8>>,
    pub loaded: bool,
}

// ── Request hooks (DESIGN §7.4) ────────────────────────────────────────────────
//
// Holding the Module authority makes this module the node's policy for
// administration: the kernel forwards every load / unload / remove asked
// from the network here, with the authenticated caller and whether it is an
// owner of the node. The default policy: owners are obeyed, everyone else
// is refused (and the refusal logged and reported).

#[reducer(on = "module_load")]
fn on_module_load<Caps>(ctx: ReducerContext<Caps>, event: ModuleEvent) -> Result<(), String>
where
    Caps: CanAll<App> + CanAll<AppBinary> + CanInsert<LoadReport>,
{
    let ModuleEvent::LoadRequest { node_id, owner, module_name, wasm_binary } = event else {
        return Ok(());
    };
    if !owner {
        let message = format!("load of '{module_name}' refused: {node_id} is not an owner");
        ctx.log(&format!("module_manager: {message}"));
        ctx.current.tables.loadreport().insert(LoadReport { seq: 0, module: module_name, ok: false, message })?;
        return Ok(());
    }
    if let Err(err) = ctx.module().load(NodeSelection::Current, wasm_binary.clone()) {
        ctx.log(&format!("module_manager: failed to load '{module_name}': {err}"));
        return Ok(());
    }
    let icon = find_app(&ctx, &module_name)?.and_then(|a| a.icon);
    upsert_app(&ctx, module_name, wasm_binary, icon, true)
}

#[reducer(on = "module_unload")]
fn on_module_unload<Caps>(ctx: ReducerContext<Caps>, event: ModuleEvent) -> Result<(), String>
where
    Caps: CanRead<App> + CanUpdate<App>,
{
    let ModuleEvent::UnloadRequest { node_id, owner, module_name } = event else {
        return Ok(());
    };
    if !owner {
        ctx.log(&format!("module_manager: unload of '{module_name}' refused: {node_id} is not an owner"));
        return Ok(());
    }
    if let Err(err) = ctx.module().unload(NodeSelection::Current, module_name.clone()) {
        ctx.log(&format!("module_manager: failed to unload '{module_name}': {err}"));
        return Ok(());
    }
    if let Some(mut app) = find_app(&ctx, &module_name)? {
        app.loaded = false;
        ctx.current.tables.app().update(app)?;
    }
    Ok(())
}

#[reducer(on = "module_remove")]
fn on_module_remove<Caps>(ctx: ReducerContext<Caps>, event: ModuleEvent) -> Result<(), String>
where
    Caps: CanRead<App> + CanDelete<App> + CanRead<AppBinary> + CanDelete<AppBinary>,
{
    let ModuleEvent::RemoveRequest { node_id, owner, module_name } = event else {
        return Ok(());
    };
    if !owner {
        ctx.log(&format!("module_manager: removal of '{module_name}' refused: {node_id} is not an owner"));
        return Ok(());
    }
    if let Err(err) = ctx.module().remove(NodeSelection::Current, module_name.clone()) {
        ctx.log(&format!("module_manager: failed to remove '{module_name}': {err}"));
        return Ok(());
    }
    if let Some(app) = find_app(&ctx, &module_name)? {
        if ctx.current.tables.appbinary().get(app.id)?.is_some() {
            ctx.current.tables.appbinary().delete(app.id)?;
        }
        ctx.current.tables.app().delete(app.id)?;
    }
    Ok(())
}

// ── Admission (DESIGN §7.4) ────────────────────────────────────────────────────
//
// As SpacetimeDB's `client_connected`: when a peer connects, the kernel runs
// this handler, and an `Err` disconnects the peer. The default policy admits
// every valid identity except the blocked ones.

/// Peers refused at connection, by id.
#[table(stateful)]
pub struct BlockedPeer {
    #[primary_key]
    id: String,
}

#[reducer(on = "connect")]
fn on_peer_connect<Caps>(ctx: ReducerContext<Caps>, node_id: String) -> Result<(), String>
where
    Caps: CanRead<BlockedPeer>,
{
    if ctx.current.tables.blockedpeer().get(node_id.clone())?.is_some() {
        ctx.log(&format!("module_manager: refusing blocked peer {node_id}"));
        return Err(format!("peer {node_id} is blocked"));
    }
    Ok(())
}

/// Refuse `id` from now on (its current connection is kept until it reconnects).
#[reducer(local)]
fn block_peer<Caps>(ctx: ReducerContext<Caps>, id: String) -> Result<(), String>
where
    Caps: CanRead<BlockedPeer> + CanInsert<BlockedPeer>,
{
    if ctx.current.tables.blockedpeer().get(id.clone())?.is_none() {
        ctx.current.tables.blockedpeer().insert(BlockedPeer { id })?;
    }
    Ok(())
}

#[reducer(local)]
fn unblock_peer<Caps>(ctx: ReducerContext<Caps>, id: String) -> Result<(), String>
where
    Caps: CanRead<BlockedPeer> + CanDelete<BlockedPeer>,
{
    if ctx.current.tables.blockedpeer().get(id.clone())?.is_some() {
        ctx.current.tables.blockedpeer().delete(id)?;
    }
    Ok(())
}

/// A load we requested finished. A failed app is no longer `loaded`.
#[reducer(on = "module_load_result")]
fn on_load_result<Caps>(ctx: ReducerContext<Caps>, event: ModuleEvent) -> Result<(), String>
where
    Caps: CanRead<App> + CanUpdate<App> + CanInsert<LoadReport>,
{
    let ModuleEvent::LoadResult { module_name, ok, message, .. } = event else {
        return Ok(());
    };
    if !ok {
        ctx.log(&format!("module_manager: {message}"));
        if let Some(mut app) = find_app(&ctx, &module_name)? {
            if app.loaded {
                app.loaded = false;
                ctx.current.tables.app().update(app)?;
            }
        }
    }
    ctx.current.tables.loadreport().insert(LoadReport { seq: 0, module: module_name, ok, message })?;
    Ok(())
}

// ── App lifecycle reducers ─────────────────────────────────────────────────────
//
// `local`: the desktop (or any module on this node) drives them; a remote
// peer can't go around the request hooks above by calling them (§7.4).

/// Register an app (upsert by name) and immediately load it onto this node.
#[reducer(local)]
fn load<Caps>(ctx: ReducerContext<Caps>, name: String, wasm: Blob, icon: Option<Vec<u8>>) -> Result<(), String>
where
    Caps: CanAll<App> + CanAll<AppBinary>,
{
    let bin = wasm.to_vec();
    if let Err(err) = ctx.module().load(NodeSelection::Current, bin.clone()) {
        ctx.log(&format!("module_manager: failed to load '{name}': {err}"));
        return Ok(());
    }
    upsert_app(&ctx, name, bin, icon, true)
}

/// Register an app without loading it (e.g. install for later launch). A
/// loaded app keeps its `loaded` flag.
#[reducer(local)]
fn register_app<Caps>(ctx: ReducerContext<Caps>, name: String, wasm: Blob, icon: Option<Vec<u8>>) -> Result<(), String>
where
    Caps: CanAll<App> + CanAll<AppBinary>,
{
    let loaded = find_app(&ctx, &name)?.is_some_and(|a| a.loaded);
    upsert_app(&ctx, name, wasm.to_vec(), icon, loaded)
}

/// Load an already-registered app onto this node by name.
#[reducer(local)]
fn load_app<Caps>(ctx: ReducerContext<Caps>, name: String) -> Result<(), String>
where
    Caps: CanRead<App> + CanUpdate<App> + CanRead<AppBinary>,
{
    let Some(mut app) = find_app(&ctx, &name)? else {
        ctx.log(&format!("module_manager: unknown app '{name}'"));
        return Ok(());
    };
    let Some(bin) = ctx.current.tables.appbinary().get(app.id)? else {
        return Err(format!("module_manager: app '{name}' has no binary"));
    };
    if let Err(err) = ctx.module().load(NodeSelection::Current, bin.bin) {
        ctx.log(&format!("module_manager: failed to load '{name}': {err}"));
        return Ok(());
    }
    app.loaded = true;
    ctx.current.tables.app().update(app)
}

/// Unload an app from this node (keeps it in the registry).
#[reducer(local)]
fn unload_app<Caps>(ctx: ReducerContext<Caps>, name: String) -> Result<(), String>
where
    Caps: CanRead<App> + CanUpdate<App>,
{
    let Some(mut app) = find_app(&ctx, &name)? else {
        ctx.log(&format!("module_manager: unknown app '{name}'"));
        return Ok(());
    };
    if let Err(err) = ctx.module().unload(NodeSelection::Current, name.clone()) {
        ctx.log(&format!("module_manager: failed to unload '{name}': {err}"));
        return Ok(());
    }
    app.loaded = false;
    let _ = ctx.current.tables.app().update(app);
    Ok(())
}

/// Unload (if loaded) and forget an app entirely.
#[reducer(local)]
fn remove_app<Caps>(ctx: ReducerContext<Caps>, name: String) -> Result<(), String>
where
    Caps: CanRead<App> + CanDelete<App> + CanRead<AppBinary> + CanDelete<AppBinary>,
{
    let Some(app) = find_app(&ctx, &name)? else {
        ctx.log(&format!("module_manager: unknown app '{name}'"));
        return Ok(());
    };
    if app.loaded {
        let _ = ctx.module().remove(NodeSelection::Current, name.clone());
    }
    if ctx.current.tables.appbinary().get(app.id)?.is_some() {
        ctx.current.tables.appbinary().delete(app.id)?;
    }
    ctx.current.tables.app().delete(app.id)?;
    Ok(())
}

// ── Queries ────────────────────────────────────────────────────────────────────

/// List all registered apps (without their wasm bytes).
#[query(local)]
fn list_apps<Caps>(ctx: QueryContext<Caps>) -> Result<Vec<AppInfo>, String>
where
    Caps: CanRead<App>,
{
    Ok(ctx
        .current
        .tables
        .app()
        .scan()?
        .into_iter()
        .map(|app| AppInfo {
            id: app.id,
            name: app.name,
            icon: app.icon,
            loaded: app.loaded,
        })
        .collect())
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn find_app<Caps>(ctx: &ReducerContext<Caps>, name: &str) -> Result<Option<App>, String>
where
    Caps: CanRead<App>,
{
    ctx.current.tables.app().get_by_name(name.to_string())
}

/// FNV-1a, 64 bits: enough to notice that a re-registered binary changed.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

fn upsert_app<Caps>(
    ctx: &ReducerContext<Caps>,
    name: String,
    bin: Vec<u8>,
    icon: Option<Vec<u8>>,
    loaded: bool,
) -> Result<(), String>
where
    Caps: CanAll<App> + CanAll<AppBinary>,
{
    let hash = fnv1a(&bin);
    let t = &ctx.current.tables;
    let id = match find_app(ctx, &name)? {
        Some(mut app) => {
            let same_bin = app.hash == hash && t.appbinary().get(app.id)?.is_some();
            if app.icon != icon || app.loaded != loaded || app.hash != hash {
                app.icon = icon;
                app.loaded = loaded;
                app.hash = hash;
                t.app().update(app.clone())?;
            }
            if same_bin {
                return Ok(());
            }
            app.id
        }
        None => t.app().insert(App { id: 0, name, icon, hash, loaded })?.id,
    };
    let row = AppBinary { id, bin };
    if t.appbinary().get(id)?.is_some() {
        t.appbinary().update(row)
    } else {
        t.appbinary().insert(row).map(|_| ())
    }
}
