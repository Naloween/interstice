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

/// Outcome of a `replay` (DESIGN §12.2). `ok` is false when the replay
/// diverged or could not run; `message` is the report or the divergence.
#[table(public, event = 32)]
#[derive(Debug)]
pub struct ReplayReport {
    #[primary_key(auto_inc)]
    pub seq: u64,
    pub module: String,
    pub ok: bool,
    pub message: String,
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

// ── Request hooks ──────────────────────────────────────────────────────────────
//
// When this module holds the Module authority, the runtime routes external
// load/remove requests through these hooks. For now they are intentionally
// minimal no-ops (the request payload is not yet passed in) — policy and
// registry integration for routed requests will be wired later.

#[reducer(on = "module_load")]
fn on_module_load(_ctx: ReducerContext) {}

#[reducer(on = "module_remove")]
fn on_module_remove(_ctx: ReducerContext) {}

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

/// Register an app (upsert by name) and immediately load it onto this node.
#[reducer]
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
#[reducer]
fn register_app<Caps>(ctx: ReducerContext<Caps>, name: String, wasm: Blob, icon: Option<Vec<u8>>) -> Result<(), String>
where
    Caps: CanAll<App> + CanAll<AppBinary>,
{
    let loaded = find_app(&ctx, &name)?.is_some_and(|a| a.loaded);
    upsert_app(&ctx, name, wasm.to_vec(), icon, loaded)
}

/// Load an already-registered app onto this node by name.
#[reducer]
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
#[reducer]
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
#[reducer]
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

// ── Record and replay (DESIGN §6.7) ────────────────────────────────────────────

/// Start recording a module loaded on this node (DESIGN §6.7).
#[reducer]
fn record_start(ctx: ReducerContext, module: String) -> Result<(), String> {
    ctx.module().record(module, true)
}

/// Stop recording a module and drop its log.
#[reducer]
fn record_stop(ctx: ReducerContext, module: String) -> Result<(), String> {
    ctx.module().record(module, false)
}

/// Replay a recorded module against a detached copy and check that it
/// reproduces the live state. The outcome (report or first divergence) is
/// published as a `ReplayReport` row; a divergence is a result, not a failure
/// of this reducer.
#[reducer]
fn replay<Caps>(ctx: ReducerContext<Caps>, module: String) -> Result<(), String>
where
    Caps: CanInsert<ReplayReport>,
{
    let (ok, message) = match ctx.module().replay(module.clone()) {
        Ok(report) => (true, report),
        Err(err) => (false, err),
    };
    ctx.log(&format!("module_manager: replay of '{module}': {message}"));
    ctx.current.tables.replayreport().insert(ReplayReport { seq: 0, module, ok, message })?;
    Ok(())
}

// ── Queries ────────────────────────────────────────────────────────────────────

/// List all registered apps (without their wasm bytes).
#[query]
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
