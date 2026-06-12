use crate::{layout::delete_recursive, tables::*};
use interstice_sdk::*;

#[reducer]
pub fn create_element<Caps>(ctx: ReducerContext<Caps>, element: UiElement)
where
    Caps: CanInsert<UiElement>,
{
    if let Err(err) = ctx.current.tables.uielement().insert(element) {
        ctx.log(&format!("ui: create_element failed: {err}"));
    }
}

#[reducer]
pub fn update_element<Caps>(ctx: ReducerContext<Caps>, element: UiElement)
where
    Caps: CanUpdate<UiElement>,
{
    if let Err(err) = ctx.current.tables.uielement().update(element) {
        ctx.log(&format!("ui: update_element failed: {err}"));
    }
}

#[reducer]
pub fn delete_element<Caps>(ctx: ReducerContext<Caps>, id: String)
where
    Caps: CanRead<UiElement> + CanDelete<UiElement>,
{
    delete_recursive(&ctx, &id);
}

#[reducer]
pub fn clear_elements<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanRead<UiElement> + CanDelete<UiElement>,
{
    for el in ctx.current.tables.uielement().scan() {
        let _ = ctx.current.tables.uielement().delete(el.id);
    }
}

#[reducer]
pub fn set_focus<Caps>(ctx: ReducerContext<Caps>, id: String)
where
    Caps: CanRead<InputFocus> + CanUpdate<InputFocus>,
{
    if let Some(mut f) = ctx.current.tables.inputfocus().get(0) {
        f.focused_element = Some(id);
        let _ = ctx.current.tables.inputfocus().update(f);
    }
}

#[reducer]
pub fn clear_focus<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanRead<InputFocus> + CanUpdate<InputFocus>,
{
    if let Some(mut f) = ctx.current.tables.inputfocus().get(0) {
        f.focused_element = None;
        let _ = ctx.current.tables.inputfocus().update(f);
    }
}
