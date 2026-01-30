use interstice_sdk::*;

interstice_module!(Some(Authority::Input));

// TABLES

// REDUCERS
#[reducer(on = "init")]
pub fn init(ctx: ReducerContext) {
    ctx.log("Hello world !");
}

#[reducer]
pub fn on_input(ctx: ReducerContext, event: InputEvent) {
    match event {
        InputEvent::Added { device_id } => ctx.log(&format!("Added device {}", device_id)),
        InputEvent::Removed { device_id } => ctx.log(&format!("Removed device {}", device_id)),
        InputEvent::MouseMotion { device_id, delta } => {
            ctx.log(&format!("Mouse Motion {:?}", delta))
        }
        InputEvent::MouseWheel { device_id, delta } => ctx.log(&format!("Mouse Wheel {:?}", delta)),
        InputEvent::Motion {
            device_id,
            axis_id,
            value,
        } => ctx.log(&format!(
            "Motion on axis {:?} with value {}",
            axis_id, value
        )),
        InputEvent::Button {
            device_id,
            button_id,
            state,
        } => ctx.log(&format!("Button {} is {:?}", button_id, state)),
        InputEvent::Key {
            device_id,
            physical_key,
            state,
        } => ctx.log(&format!("Key {:?} is {:?}", physical_key, state)),
    }
}
