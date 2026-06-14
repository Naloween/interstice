use interstice_sdk::*;

use crate::GpuExt;
use crate::tables::{
    Compositor, HasCompositorEditHandle, HasSurfaceAssignmentEditHandle,
    HasSurfaceAssignmentReadHandle, HasSurfaceInfoEditHandle, HasSurfaceInfoReadHandle,
    HasSurfaceTargetEditHandle, SurfaceAssignment, SurfaceInfo, SurfaceTarget,
};

/// Single-row primary key for the compositor claim.
pub(crate) const COMPOSITOR_KEY: u32 = 0;
/// Surface 0 is reserved for the swapchain and cannot be created/destroyed.
pub(crate) const SWAPCHAIN_SURFACE_ID: u32 = 0;

/// Returns true when `caller` currently holds the compositor claim. With no
/// claim yet, nobody is the compositor.
pub(crate) fn is_compositor<Caps: CanRead<Compositor>>(
    ctx: &ReducerContext<Caps>,
    caller: &str,
) -> bool {
    ctx.current
        .tables
        .compositor()
        .get(COMPOSITOR_KEY)
        .map(|row| row.module_name == caller)
        .unwrap_or(false)
}

/// Report the surface the calling module's layers render into, resolved from the
/// caller's identity. Returns the swapchain surface (id 0) when unassigned. Apps
/// use this to size their drawing to the surface the compositor gave them; the
/// desktop assigns each app an offscreen surface before it runs, so the returned
/// dimensions are real. NOTE: surface 0's dimensions are 0×0 until the first
/// render frame populates them.
#[query]
fn surface_info<Caps>(ctx: QueryContext<Caps>) -> SurfaceInfo
where
    Caps: CanRead<SurfaceAssignment> + CanRead<SurfaceInfo>,
{
    let surface_id = ctx
        .current
        .tables
        .surfaceassignment()
        .get(ctx.caller_module_name.clone())
        .map(|a| a.surface_id)
        .unwrap_or(SWAPCHAIN_SURFACE_ID);
    ctx.current
        .tables
        .surfaceinfo()
        .get(surface_id)
        .unwrap_or(SurfaceInfo {
            id: surface_id,
            width: 0,
            height: 0,
        })
}

/// Claim exclusive authority to manage surfaces. The first caller wins; repeat
/// calls by the same owner are idempotent, others are rejected.
#[reducer]
pub fn claim_compositor<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanRead<Compositor> + CanInsert<Compositor>,
{
    let caller = ctx.caller_node_id.clone();
    match ctx.current.tables.compositor().get(COMPOSITOR_KEY) {
        Some(row) if row.module_name == caller => {
            ctx.log(&format!("Compositor already claimed by '{}'", caller));
        }
        Some(row) => {
            ctx.log(&format!(
                "Compositor claim refused: already held by '{}'",
                row.module_name
            ));
        }
        None => {
            if let Err(err) = ctx.current.tables.compositor().insert(Compositor {
                id: COMPOSITOR_KEY,
                module_name: caller.clone(),
            }) {
                ctx.log(&format!("Failed to record compositor claim: {}", err));
            } else {
                ctx.log(&format!("Compositor claimed by '{}'", caller));
            }
        }
    }
}

/// Create an offscreen surface with a caller-chosen id (reducers can't return
/// values, so the compositor owns the id namespace, like `ui.create_element`).
#[reducer]
pub fn create_surface<Caps>(ctx: ReducerContext<Caps>, id: u32, width: u32, height: u32)
where
    Caps: CanRead<Compositor>
        + CanRead<SurfaceInfo>
        + CanInsert<SurfaceInfo>
        + CanRead<SurfaceTarget>
        + CanInsert<SurfaceTarget>,
{
    if !is_compositor(&ctx, &ctx.caller_node_id) {
        ctx.log("create_surface refused: caller is not the compositor");
        return;
    }
    if id == SWAPCHAIN_SURFACE_ID {
        ctx.log("Surface id 0 is reserved for the swapchain");
        return;
    }
    if width == 0 || height == 0 {
        ctx.log("Surface dimensions must be greater than zero");
        return;
    }
    if ctx.current.tables.surfacetarget().get(id).is_some() {
        ctx.log(&format!("Surface {} already exists", id));
        return;
    }

    if let Err(err) = ctx
        .current
        .tables
        .surfaceinfo()
        .insert(SurfaceInfo { id, width, height })
    {
        ctx.log(&format!("Failed to insert surface info: {}", err));
        return;
    }
    if let Err(err) = ctx.current.tables.surfacetarget().insert(SurfaceTarget {
        id,
        width,
        height,
        texture_id: None,
        view_id: None,
    }) {
        ctx.log(&format!("Failed to insert surface target: {}", err));
    }
}

/// Resize an offscreen surface. Clears the cached texture/view so the render
/// loop reallocates at the new size; the old GPU resources are freed here.
#[reducer]
pub fn resize_surface<Caps>(ctx: ReducerContext<Caps>, id: u32, width: u32, height: u32)
where
    Caps: CanRead<Compositor>
        + CanRead<SurfaceInfo>
        + CanUpdate<SurfaceInfo>
        + CanRead<SurfaceTarget>
        + CanUpdate<SurfaceTarget>,
{
    if !is_compositor(&ctx, &ctx.caller_node_id) {
        ctx.log("resize_surface refused: caller is not the compositor");
        return;
    }
    if width == 0 || height == 0 {
        ctx.log("Surface dimensions must be greater than zero");
        return;
    }
    let Some(mut target) = ctx.current.tables.surfacetarget().get(id) else {
        ctx.log(&format!("Surface {} not found", id));
        return;
    };

    free_target_gpu(&ctx, &target);
    target.width = width;
    target.height = height;
    target.texture_id = None;
    target.view_id = None;
    let _ = ctx.current.tables.surfacetarget().update(target);

    if let Some(mut info) = ctx.current.tables.surfaceinfo().get(id) {
        info.width = width;
        info.height = height;
        let _ = ctx.current.tables.surfaceinfo().update(info);
    }
}

/// Destroy an offscreen surface: free its GPU resources, drop its registry rows,
/// and reset any module routed to it back to the swapchain.
#[reducer]
pub fn destroy_surface<Caps>(ctx: ReducerContext<Caps>, id: u32)
where
    Caps: CanRead<Compositor>
        + CanRead<SurfaceInfo>
        + CanDelete<SurfaceInfo>
        + CanRead<SurfaceTarget>
        + CanDelete<SurfaceTarget>
        + CanRead<SurfaceAssignment>
        + CanUpdate<SurfaceAssignment>
        + CanDelete<SurfaceAssignment>,
{
    if !is_compositor(&ctx, &ctx.caller_node_id) {
        ctx.log("destroy_surface refused: caller is not the compositor");
        return;
    }
    if id == SWAPCHAIN_SURFACE_ID {
        ctx.log("The swapchain surface cannot be destroyed");
        return;
    }
    let Some(target) = ctx.current.tables.surfacetarget().get(id) else {
        ctx.log(&format!("Surface {} not found", id));
        return;
    };

    free_target_gpu(&ctx, &target);
    let _ = ctx.current.tables.surfacetarget().delete(id);
    let _ = ctx.current.tables.surfaceinfo().delete(id);

    // Re-route anything pointing at the destroyed surface back to the swapchain.
    for mut assignment in ctx
        .current
        .tables
        .surfaceassignment()
        .scan()
        .into_iter()
        .filter(|a| a.surface_id == id)
    {
        assignment.surface_id = SWAPCHAIN_SURFACE_ID;
        let _ = ctx.current.tables.surfaceassignment().update(assignment);
    }
}

/// Route a module's layers into a surface (or back to the swapchain with id 0).
#[reducer]
pub fn assign_module_surface<Caps>(ctx: ReducerContext<Caps>, module_name: String, surface_id: u32)
where
    Caps: CanRead<Compositor>
        + CanRead<SurfaceTarget>
        + CanRead<SurfaceAssignment>
        + CanInsert<SurfaceAssignment>
        + CanUpdate<SurfaceAssignment>
        + CanDelete<SurfaceAssignment>,
{
    if !is_compositor(&ctx, &ctx.caller_node_id) {
        ctx.log("assign_module_surface refused: caller is not the compositor");
        return;
    }
    if surface_id != SWAPCHAIN_SURFACE_ID && ctx.current.tables.surfacetarget().get(surface_id).is_none() {
        ctx.log(&format!("Surface {} does not exist", surface_id));
        return;
    }

    if surface_id == SWAPCHAIN_SURFACE_ID {
        // Default routing — drop any explicit assignment.
        let _ = ctx.current.tables.surfaceassignment().delete(module_name);
        return;
    }

    if ctx
        .current
        .tables
        .surfaceassignment()
        .get(module_name.clone())
        .is_some()
    {
        let _ = ctx.current.tables.surfaceassignment().update(SurfaceAssignment {
            module_name,
            surface_id,
        });
    } else if let Err(err) = ctx.current.tables.surfaceassignment().insert(SurfaceAssignment {
        module_name,
        surface_id,
    }) {
        ctx.log(&format!("Failed to insert surface assignment: {}", err));
    }
}

fn free_target_gpu<Caps>(ctx: &ReducerContext<Caps>, target: &SurfaceTarget) {
    let gpu = ctx.gpu();
    if let Some(view) = target.view_id {
        let _ = gpu.destroy_texture_view(view);
    }
    if let Some(texture) = target.texture_id {
        let _ = gpu.destroy_texture(texture);
    }
}
