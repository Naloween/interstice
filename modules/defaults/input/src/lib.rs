use interstice_sdk::*;

interstice_module!(visibility: Private, authorities: [Input]);

#[table(public, ephemeral)]
struct KeyState {
    #[primary_key]
    code: u32,
    pressed: bool,
}

#[table(public, ephemeral)]
struct MouseState {
    #[primary_key]
    id: u32,
    position: (f32, f32),
    wheel_delta: (f32, f32),
}

#[reducer(on = "load")]
fn on_load(ctx: ReducerContext) {
    let res = ctx.current.tables.mousestate().insert(MouseState {
        id: 0,
        position: (0.0, 0.0),
        wheel_delta: (0.0, 0.0),
    });
    if let Err(err) = res {
        ctx.log(&format!("Failed to initialize mouse state: {}", err));
    }
}

#[reducer(on = "input")]
fn on_input(ctx: ReducerContext, event: InputEvent) {
    match event {
        InputEvent::Added { .. } => {}
        InputEvent::Removed { .. } => {}
        InputEvent::MouseMotion { delta, .. } => {
            let mouse_state = ctx.current.tables.mousestate().get(0).unwrap();
            let new_position = (
                mouse_state.position.0 + delta.0 as f32,
                mouse_state.position.1 + delta.1 as f32,
            );

            let res = ctx.current.tables.mousestate().update(MouseState {
                id: 0,
                position: new_position,
                wheel_delta: mouse_state.wheel_delta,
            });
            if let Err(err) = res {
                ctx.log(&format!("Failed to update mouse state: {}", err));
            }
        }
        InputEvent::MouseWheel { delta, .. } => {
            let mouse_state = ctx.current.tables.mousestate().get(0).unwrap();
            let new_wheel_delta = (
                mouse_state.wheel_delta.0 + delta.0 as f32,
                mouse_state.wheel_delta.1 + delta.1 as f32,
            );

            let res = ctx.current.tables.mousestate().update(MouseState {
                id: 0,
                position: mouse_state.position,
                wheel_delta: new_wheel_delta,
            });
            if let Err(err) = res {
                ctx.log(&format!("Failed to update mouse state: {}", err));
            }
        }
        InputEvent::Motion { .. } => {}
        InputEvent::Button { .. } => {}
        InputEvent::Key {
            physical_key,
            state,
            ..
        } => {
            let code = match physical_key {
                PhysicalKey::Code(key_code) => key_code as u32,
                PhysicalKey::Unidentified(code) => code,
            };
            let pressed = matches!(state, ElementState::Pressed);

            match ctx.current.tables.keystate().get(code) {
                None => {
                    let res = ctx
                        .current
                        .tables
                        .keystate()
                        .insert(KeyState { code, pressed });
                    if let Err(err) = res {
                        ctx.log(&format!("Failed to insert key state: {}", err));
                    }
                }
                _ => {}
            }
            // Update the pressed state anyway to throw the update event for the clients
            let _ = ctx
                .current
                .tables
                .keystate()
                .update(KeyState { code, pressed });
        }
    }
}
