use crate::node_client::handshake_with_node;
use crate::node_registry::NodeRegistry;
use interstice_core::{
    IntersticeError, NetworkPacket, interstice_abi::IntersticeValue, packet::write_packet,
};

pub async fn call_reducer(
    node_ref: String,
    module_name: String,
    reducer_name: String,
    input: IntersticeValue,
) -> Result<(), IntersticeError> {
    let registry = NodeRegistry::load()?;
    let node_address = registry
        .resolve_address(&node_ref)
        .ok_or_else(|| IntersticeError::Internal("Unknown node".into()))?;
    // connect to node
    let (mut stream, _handshake) = handshake_with_node(&node_address).await?;

    // Send call reducer packet to node
    let packet = NetworkPacket::ReducerCall {
        module_name,
        reducer_name,
        input,
    };
    write_packet(&mut stream, &packet).await?;

    // Close connection properly
    let packet = NetworkPacket::Close;
    write_packet(&mut stream, &packet).await?;

    Ok(())
}
