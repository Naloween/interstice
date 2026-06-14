use interstice_sdk::*;

interstice_module!(visibility: Public, authorities: [Module]);

// ── Registry ───────────────────────────────────────────────────────────────────
//
// The module manager keeps a registry of "apps" (modules) known to this node.
// An app may be registered without being loaded; loading it instantiates the
// module on the current node via the Module authority. This is the base the
// desktop module builds on to install, launch, and stop apps.

#[table]
pub struct App {
    #[primary_key(auto_inc)]
    id: u64,
    #[index(hash, unique)]
    name: String,
    bin: Vec<u8>,
    icon: Option<Vec<u8>>,
    loaded: bool,
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

// ── App lifecycle reducers ─────────────────────────────────────────────────────

/// Register an app (upsert by name) and immediately load it onto this node.
#[reducer]
fn load<Caps>(ctx: ReducerContext<Caps>, name: String, wasm_binary: Vec<u8>, icon: Option<Vec<u8>>)
where
    Caps: CanRead<App> + CanInsert<App> + CanUpdate<App>,
{
    if let Err(err) = ctx
        .module()
        .load(NodeSelection::Current, wasm_binary.clone())
    {
        ctx.log(&format!("module_manager: failed to load '{name}': {err}"));
        return;
    }
    upsert_app(&ctx, name, wasm_binary, icon, true);
}

/// Register an app without loading it (e.g. install for later launch).
#[reducer]
fn register_app<Caps>(
    ctx: ReducerContext<Caps>,
    name: String,
    wasm_binary: Vec<u8>,
    icon: Option<Vec<u8>>,
) where
    Caps: CanRead<App> + CanInsert<App> + CanUpdate<App>,
{
    upsert_app(&ctx, name, wasm_binary, icon, false);
}

/// Load an already-registered app onto this node by name.
#[reducer]
fn load_app<Caps>(ctx: ReducerContext<Caps>, name: String)
where
    Caps: CanRead<App> + CanUpdate<App>,
{
    let Some(mut app) = find_app(&ctx, &name) else {
        ctx.log(&format!("module_manager: unknown app '{name}'"));
        return;
    };
    if let Err(err) = ctx.module().load(NodeSelection::Current, app.bin.clone()) {
        ctx.log(&format!("module_manager: failed to load '{name}': {err}"));
        return;
    }
    app.loaded = true;
    let _ = ctx.current.tables.app().update(app);
}

/// Unload an app from this node (keeps it in the registry).
#[reducer]
fn unload_app<Caps>(ctx: ReducerContext<Caps>, name: String)
where
    Caps: CanRead<App> + CanUpdate<App>,
{
    let Some(mut app) = find_app(&ctx, &name) else {
        ctx.log(&format!("module_manager: unknown app '{name}'"));
        return;
    };
    if let Err(err) = ctx.module().remove(NodeSelection::Current, name.clone()) {
        ctx.log(&format!("module_manager: failed to unload '{name}': {err}"));
        return;
    }
    app.loaded = false;
    let _ = ctx.current.tables.app().update(app);
}

/// Unload (if loaded) and forget an app entirely.
#[reducer]
fn remove_app<Caps>(ctx: ReducerContext<Caps>, name: String)
where
    Caps: CanRead<App> + CanDelete<App>,
{
    let Some(app) = find_app(&ctx, &name) else {
        ctx.log(&format!("module_manager: unknown app '{name}'"));
        return;
    };
    if app.loaded {
        let _ = ctx.module().remove(NodeSelection::Current, name.clone());
    }
    let _ = ctx.current.tables.app().delete(app.id);
}

// ── Queries ────────────────────────────────────────────────────────────────────

/// List all registered apps (without their wasm bytes).
#[query]
fn list_apps<Caps>(ctx: QueryContext<Caps>) -> Vec<AppInfo>
where
    Caps: CanRead<App>,
{
    ctx.current
        .tables
        .app()
        .scan()
        .into_iter()
        .map(|app| AppInfo {
            id: app.id,
            name: app.name,
            icon: app.icon,
            loaded: app.loaded,
        })
        .collect()
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn find_app<Caps>(ctx: &ReducerContext<Caps>, name: &str) -> Option<App>
where
    Caps: CanRead<App>,
{
    ctx.current
        .tables
        .app()
        .scan()
        .into_iter()
        .find(|app| app.name == name)
}

fn upsert_app<Caps>(
    ctx: &ReducerContext<Caps>,
    name: String,
    wasm_binary: Vec<u8>,
    icon: Option<Vec<u8>>,
    loaded: bool,
) where
    Caps: CanRead<App> + CanInsert<App> + CanUpdate<App>,
{
    if let Some(mut app) = find_app(ctx, &name) {
        app.bin = wasm_binary;
        app.icon = icon;
        app.loaded = loaded;
        let _ = ctx.current.tables.app().update(app);
    } else {
        let _ = ctx.current.tables.app().insert(App {
            id: 0,
            name,
            bin: wasm_binary,
            icon,
            loaded,
        });
    }
}
