use interstice_core::{
    IntersticeError, NetworkPacket,
    interstice_abi::IntersticeValue,
    packet::{read_packet, write_packet},
};

pub async fn call_query(
    node_address: String,
    module_name: String,
    query_name: String,
    input: IntersticeValue,
) -> Result<(), IntersticeError> {
    // The CLI simulate a remote node instance
    let cli_node_id = uuid::Uuid::new_v4();

    // connect to node
    let mut stream = tokio::net::TcpStream::connect(node_address).await.unwrap();
    let packet = NetworkPacket::Handshake {
        node_id: cli_node_id.to_string(),
        address: "127.0.0.1:12345".into(),
    };
    write_packet(&mut stream, &packet).await?;
    let _ = read_packet(&mut stream).await?;

    // Send call query packet to node
    let request_id = uuid::Uuid::new_v4().to_string();
    let packet = NetworkPacket::QueryCall {
        module_name,
        query_name,
        input,
        request_id: request_id.clone(),
    };
    write_packet(&mut stream, &packet).await?;

    // Wait receiving query response packet from node
    let response_packet = read_packet(&mut stream).await?;
    match response_packet {
        NetworkPacket::QueryResponse {
            request_id: response_request_id,
            result,
        } => {
            println!("Query response: {:?}", result);
        }
        _ => {
            println!("Unexpected packet received: {:?}", response_packet);
        }
    }

    // Close connection properly
    let packet = NetworkPacket::Close;
    write_packet(&mut stream, &packet).await?;

    Ok(())
}
