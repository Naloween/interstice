use interstice_core::{
    IntersticeError, NetworkPacket,
    interstice_abi::IntersticeValue,
    packet::{read_packet, write_packet},
};

pub async fn call_reducer(
    node_address: String,
    module_name: String,
    reducer_name: String,
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
