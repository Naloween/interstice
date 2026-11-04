//! Input routing (DESIGN §12.2): a compositor ("router") tells `input` where
//! each app's surface sits on screen; `input_view` then gives every caller the
//! pointer in its own coordinates and tells it whether it has the pointer and
//! the keyboard. Without a router, every caller sees the raw global state.

use interstice_sdk::*;

use crate::{
    HasMouseButtonEditHandle, HasMouseButtonReadHandle, HasMouseStateEditHandle,
    HasMouseStateReadHandle, MouseButton, MouseState,
};

const KEY: u32 = 0;

/// The router claim (singleton). `keyboard` is the module with keyboard focus
/// (`""` for none); `capture` gives the pointer to the router itself.
#[table(ephemeral)]
pub struct InputRouter {
    #[primary_key]
    id: u32,
    module_name: String,
    capture: bool,
    keyboard: String,
}

/// Where an app's content sits on screen, in pixels, and its stacking order.
#[table(ephemeral)]
pub struct InputRoute {
    #[primary_key]
    module_name: String,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    z: i32,
}

/// Implicit pointer grab: the pointer owner when a button went down with none
/// down before, held until the last button goes up. `module` is `""` for the
/// router / no app.
#[table(ephemeral)]
pub struct PointerGrab {
    #[primary_key]
    id: u32,
    active: bool,
    module: String,
}

/// What a caller may know about the input devices (see [`input_view`]).
#[interstice_type]
#[derive(Debug, Default)]
pub struct InputView {
    /// Pointer position in the caller's coordinates (surface-local when routed).
    pub position: (f32, f32),
    /// Cumulative wheel total; diff it per frame, act only while `pointer_inside`.
    pub wheel_delta: (f32, f32),
    pub left: bool,
    pub right: bool,
    pub middle: bool,
    /// The pointer is over the caller (or grabbed by it).
    pub pointer_inside: bool,
    /// The caller receives keyboard input.
    pub keyboard_focus: bool,
}

/// The input as seen by the calling module (DESIGN §12.2).
#[query]
fn input_view<Caps>(ctx: QueryContext<Caps>) -> Result<InputView, String>
where
    Caps: CanRead<InputRouter>
        + CanRead<InputRoute>
        + CanRead<PointerGrab>
        + CanRead<MouseState>
        + CanRead<MouseButton>,
{
    let t = &ctx.current.tables;
    let mouse = t.mousestate().get(0)?.map(|m| (m.position, m.wheel_delta)).unwrap_or_default();
    let (position, wheel_delta) = mouse;
    let button = |id: u32| -> Result<bool, String> {
        Ok(t.mousebutton().get(id)?.map(|b| b.pressed).unwrap_or(false))
    };
    let (left, right, middle) = (button(0)?, button(1)?, button(2)?);
    let raw = InputView {
        position,
        wheel_delta,
        left,
        right,
        middle,
        pointer_inside: true,
        keyboard_focus: true,
    };
    let Some(router) = t.inputrouter().get(KEY)? else {
        return Ok(raw);
    };
    let routes = t.inputroute().scan()?;
    let grab = t.pointergrab().get(KEY)?;
    let owner = pointer_owner(&router, &routes, grab.as_ref(), position);
    let caller = ctx.caller_module_name.as_str();
    if caller == router.module_name {
        return Ok(InputView {
            pointer_inside: owner.is_empty(),
            keyboard_focus: router.keyboard.is_empty(),
            ..raw
        });
    }
    let Some(route) = routes.iter().find(|r| r.module_name == caller) else {
        // Not routed while a router exists: no input, as in Wayland.
        return Ok(InputView {
            left: false,
            right: false,
            middle: false,
            pointer_inside: false,
            keyboard_focus: false,
            ..raw
        });
    };
    let inside = owner == caller;
    Ok(InputView {
        position: (position.0 - route.x, position.1 - route.y),
        wheel_delta,
        left: left && inside,
        right: right && inside,
        middle: middle && inside,
        pointer_inside: inside,
        keyboard_focus: router.keyboard == caller,
    })
}

/// The module that owns the pointer (`""` = the router / no app).
fn pointer_owner(
    router: &InputRouter,
    routes: &[InputRoute],
    grab: Option<&PointerGrab>,
    (px, py): (f32, f32),
) -> String {
    if let Some(g) = grab.filter(|g| g.active) {
        return g.module.clone();
    }
    if router.capture {
        return String::new();
    }
    routes
        .iter()
        .filter(|r| px >= r.x && px < r.x + r.w && py >= r.y && py < r.y + r.h)
        .max_by_key(|r| r.z)
        .map(|r| r.module_name.clone())
        .unwrap_or_default()
}

/// Called by `on_input` before a button row changes: starts the implicit grab
/// on the first press, releases it with the last release.
pub(crate) fn update_grab<Caps>(ctx: &ReducerContext<Caps>, button_id: u32, pressed: bool) -> Result<(), String>
where
    Caps: CanRead<InputRouter>
        + CanRead<InputRoute>
        + CanAll<PointerGrab>
        + CanRead<MouseState>
        + CanRead<MouseButton>,
{
    let t = &ctx.current.tables;
    let Some(router) = t.inputrouter().get(KEY)? else {
        return Ok(());
    };
    let others_down = t
        .mousebutton()
        .scan()?
        .iter()
        .any(|b| b.button_id != button_id && b.pressed);
    if others_down {
        return Ok(());
    }
    let grab = if pressed {
        let position = t.mousestate().get(0)?.map(|m| m.position).unwrap_or_default();
        let routes = t.inputroute().scan()?;
        let module = pointer_owner(&router, &routes, None, position);
        PointerGrab { id: KEY, active: true, module }
    } else {
        PointerGrab { id: KEY, active: false, module: String::new() }
    };
    if t.pointergrab().get(KEY)?.is_some() {
        t.pointergrab().update(grab)
    } else {
        t.pointergrab().insert(grab).map(|_| ())
    }
}

/// Become the input router. The first caller wins; later claims by others are
/// refused (logged), repeats by the holder are no-ops.
#[reducer]
fn claim_input_router<Caps>(ctx: ReducerContext<Caps>) -> Result<(), String>
where
    Caps: CanRead<InputRouter> + CanInsert<InputRouter>,
{
    let caller = ctx.caller_module_name.clone();
    match ctx.current.tables.inputrouter().get(KEY)? {
        Some(row) if row.module_name == caller => {}
        Some(row) => ctx.log(&format!("input router claim by '{caller}' refused: held by '{}'", row.module_name)),
        None => {
            ctx.current.tables.inputrouter().insert(InputRouter {
                id: KEY,
                module_name: caller.clone(),
                capture: false,
                keyboard: String::new(),
            })?;
            ctx.log(&format!("input router claimed by '{caller}'"));
        }
    }
    Ok(())
}

fn router<Caps: CanRead<InputRouter>>(ctx: &ReducerContext<Caps>) -> Result<InputRouter, String> {
    match ctx.current.tables.inputrouter().get(KEY)? {
        Some(r) if r.module_name == ctx.caller_module_name => Ok(r),
        _ => Err(format!("'{}' is not the input router", ctx.caller_module_name)),
    }
}

/// Place (or move) an app's content rect, in screen pixels.
#[reducer]
fn set_input_route<Caps>(
    ctx: ReducerContext<Caps>,
    module_name: String,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    z: i32,
) -> Result<(), String>
where
    Caps: CanRead<InputRouter> + CanRead<InputRoute> + CanInsert<InputRoute> + CanUpdate<InputRoute>,
{
    router(&ctx)?;
    let row = InputRoute { module_name, x, y, w, h, z };
    let t = ctx.current.tables.inputroute();
    if t.get(row.module_name.clone())?.is_some() {
        t.update(row)
    } else {
        t.insert(row).map(|_| ())
    }
}

#[reducer]
fn remove_input_route<Caps>(ctx: ReducerContext<Caps>, module_name: String) -> Result<(), String>
where
    Caps: CanRead<InputRouter> + CanRead<InputRoute> + CanDelete<InputRoute>,
{
    router(&ctx)?;
    if ctx.current.tables.inputroute().get(module_name.clone())?.is_some() {
        ctx.current.tables.inputroute().delete(module_name)?;
    }
    Ok(())
}

/// Give the keyboard to `module_name` (`""` for nobody).
#[reducer]
fn set_keyboard_focus<Caps>(ctx: ReducerContext<Caps>, module_name: String) -> Result<(), String>
where
    Caps: CanRead<InputRouter> + CanUpdate<InputRouter>,
{
    let mut r = router(&ctx)?;
    if r.keyboard != module_name {
        r.keyboard = module_name;
        ctx.current.tables.inputrouter().update(r)?;
    }
    Ok(())
}

/// While `capture` is set, no app owns the pointer (outside a grab).
#[reducer]
fn set_pointer_capture<Caps>(ctx: ReducerContext<Caps>, capture: bool) -> Result<(), String>
where
    Caps: CanRead<InputRouter> + CanUpdate<InputRouter>,
{
    let mut r = router(&ctx)?;
    if r.capture != capture {
        r.capture = capture;
        ctx.current.tables.inputrouter().update(r)?;
    }
    Ok(())
}
