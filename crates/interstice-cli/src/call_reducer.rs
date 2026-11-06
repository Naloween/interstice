use crate::node_client::handshake_with_node;
use crate::node_registry::NodeRegistry;
use interstice_core::{
    IntersticeError, NetworkPacket,
    interstice_abi::IntersticeValue,
    packet::{read_packet, write_packet},
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

    // Call, and wait for the commit or the error.
    let packet = NetworkPacket::ReducerCall {
        request_id: Some(1),
        module_name,
        reducer_name,
        input,
    };
    write_packet(&mut stream, &packet).await?;
    let outcome = loop {
        match read_packet(&mut stream).await? {
            NetworkPacket::ReducerResult { request_id: 1, outcome } => break outcome,
            NetworkPacket::Error(err) => break Err(err),
            _ => {}
        }
    };

    // Close connection properly
    let packet = NetworkPacket::Close;
    write_packet(&mut stream, &packet).await?;

    match outcome {
        Ok(()) => {
            println!("committed");
            Ok(())
        }
        Err(err) => Err(IntersticeError::Internal(format!("reducer failed: {err}"))),
    }
}
