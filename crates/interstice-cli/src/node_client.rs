//! Connecting to a node, through `interstice-client`, with the CLI's key
//! and the checks the registry allows.

use crate::data_directory::{cli_known_peers, load_cli_identity};
use interstice_client::Client;
use interstice_core::{
    IntersticeError,
    interstice_abi::{IntersticeValue, NodeSchema},
};

pub struct HandshakeInfo {
    pub node_id: String,
    pub address: String,
}

/// Connect to the node at `address` as this CLI (mutual TLS, DESIGN §7.4),
/// and check that it is the node the registry names at that address.
pub async fn connect(address: &str) -> Result<Client, IntersticeError> {
    let identity = load_cli_identity()?;
    let client = Client::connect(address, &identity, &cli_known_peers()).await?;
    if let Ok(registry) = crate::node_registry::NodeRegistry::load() {
        if let Some(expected) = registry
            .nodes
            .iter()
            .find(|n| n.address == address)
            .and_then(|n| n.node_id.as_deref())
        {
            if expected != client.node_id() {
                return Err(IntersticeError::PermissionDenied(format!(
                    "the node at {address} is {}, not {expected} as registered",
                    client.node_id()
                )));
            }
        }
    }
    Ok(client)
}

/// Connect, and say who answered.
pub async fn handshake_with_node(address: &str) -> Result<(Client, HandshakeInfo), IntersticeError> {
    let client = connect(address).await?;
    let info = HandshakeInfo {
        node_id: client.node_id().to_string(),
        address: client.node_address().to_string(),
    };
    Ok((client, info))
}

pub async fn fetch_node_schema(
    address: &str,
    node_name: &str,
) -> Result<(NodeSchema, HandshakeInfo), IntersticeError> {
    let (client, info) = handshake_with_node(address).await?;
    let schema = client.schema(node_name).await?;
    client.close().await;
    Ok((schema, info))
}

/// Type `raw` by the signature of `module.function` from the node's schema.
pub async fn typed_args(
    client: &Client,
    module: &str,
    function: &str,
    query: bool,
    raw: &[String],
) -> Result<IntersticeValue, IntersticeError> {
    let schema = client.schema("").await?;
    let what = if query { "query" } else { "reducer" };
    let module_schema = schema
        .modules
        .iter()
        .find(|m| m.name == module)
        .ok_or_else(|| IntersticeError::Internal(format!("the node has no module '{module}' you may call")))?;
    let params = crate::args::params(module_schema, function, query)
        .ok_or_else(|| IntersticeError::Internal(format!("module '{module}' has no {what} '{function}' you may call")))?;
    crate::args::parse(function, raw, params, &module_schema.type_definitions)
        .map(IntersticeValue::Vec)
        .map_err(IntersticeError::Internal)
}
