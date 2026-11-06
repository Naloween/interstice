use crate::node_client::{connect, typed_args};
use crate::node_registry::NodeRegistry;
use interstice_core::IntersticeError;

/// Call a reducer and wait for its commit or its error (DESIGN §7.1).
pub async fn call_reducer(
    node_ref: String,
    module_name: String,
    reducer_name: String,
    args: Vec<String>,
) -> Result<(), IntersticeError> {
    let registry = NodeRegistry::load()?;
    let node_address = registry
        .resolve_address(&node_ref)
        .ok_or_else(|| IntersticeError::Internal("Unknown node".into()))?;
    let client = connect(&node_address).await?;
    let input = match typed_args(&client, &module_name, &reducer_name, false, &args).await {
        Ok(input) => input,
        Err(err) => {
            client.close().await;
            return Err(err);
        }
    };
    let outcome = client.call_reducer(&module_name, &reducer_name, input).await;
    client.close().await;
    match outcome {
        Ok(()) => {
            println!("committed");
            Ok(())
        }
        Err(err) => Err(IntersticeError::Internal(format!("reducer failed: {err}"))),
    }
}
