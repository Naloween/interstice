// REDUCERS

use interstice_sdk::{call_reducer, interstice_module, log, reducer, IntersticeValue};

interstice_module!();

#[reducer]
fn caller() {
    log("Calling hello...");
    call_reducer(
        "hello".to_string(),
        "hello".to_string(),
        IntersticeValue::Vec(vec![IntersticeValue::String(
            "called from caller".to_string(),
        )]),
    );
    log("hello called !");
}
