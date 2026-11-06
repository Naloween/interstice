use crate::node_client::{connect, typed_args};
use crate::node_registry::NodeRegistry;
use interstice_core::IntersticeError;

pub async fn call_query(
    node_ref: String,
    module_name: String,
    query_name: String,
    args: Vec<String>,
) -> Result<(), IntersticeError> {
    let registry = NodeRegistry::load()?;
    let node_address = registry
        .resolve_address(&node_ref)
        .ok_or_else(|| IntersticeError::Internal("Unknown node".into()))?;
    let client = connect(&node_address).await?;
    let call = match typed_args(&client, &module_name, &query_name, true, &args).await {
        Ok(call) => call,
        Err(err) => {
            client.close().await;
            return Err(err);
        }
    };
    let result = client.call_query(&module_name, &query_name, call.args.clone()).await;
    client.close().await;
    let value = result.map_err(|e| IntersticeError::Internal(format!("query failed: {e}")))?;
    println!("{}", call.render(&value));
    Ok(())
}
