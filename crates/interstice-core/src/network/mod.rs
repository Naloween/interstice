// use crate::error::IntersticeError;
// use crate::node::NodeId;
// use interstice_abi::{NodeSchema, Row, encode};
// use serde::{Deserialize, Serialize};
// use std::collections::HashMap;
// use std::io::{BufRead, BufReader, Write as _};
// use std::net::{TcpListener, TcpStream};
// use std::sync::{Arc, Mutex};
// use std::thread;
// use tokio::sync::mpsc;

// #[derive(Debug, Serialize, Deserialize)]
// pub enum NetworkPacket {
//     RequestSubscription(RequestSubscription),
//     SubscriptionEvent(SubscriptionEvent),
//     Error(String),
// }

// #[derive(Debug, Serialize, Deserialize)]
// pub struct RequestSubscription {
//     module_name: String,
//     table_name: String,
//     event: TableEvent,
// }

// #[derive(Debug, Serialize, Deserialize)]
// pub enum TableEvent {
//     Insert,
//     Update,
//     Delete,
// }

// #[derive(Serialize, Deserialize)]
// pub enum SubscriptionEvent {
//     TableInsertEvent {
//         module_name: String,
//         table_name: String,
//         inserted_row: Row,
//     },
//     TableUpdateEvent {
//         module_name: String,
//         table_name: String,
//         old_row: Row,
//         new_row: Row,
//     },
//     TableDeleteEvent {
//         module_name: String,
//         table_name: String,
//         deleted_row: Row,
//     },
// }

// pub trait CustomClone {
//     fn clone(&self) -> Self;
// }

// impl CustomClone for Vec<TcpStream> {
//     fn clone(&self) -> Self {
//         let mut res = Vec::with_capacity(self.capacity());
//         for stream in self.iter() {
//             res.push(stream.try_clone().unwrap());
//         }
//         return res;
//     }
// }

// pub struct PeerHandle {
//     pub node_id: NodeId,
//     sender: mpsc::UnboundedSender<NetworkPacket>,
// }

// pub struct Network {
//     adress: String,
//     peers: HashMap<NodeId, PeerHandle>,
// }

// // impl Network {
// //     pub fn send_event(
// //         &mut self,
// //         node_id: NodeId,
// //         event: SubscriptionEvent,
// //     ) -> Result<(), IntersticeError> {
// //         if let Some(client) = self.client_nodes.get_mut(&node_id) {
// //             let packet = NetworkPacket::SubscriptionEvent(match event {
// //                 SubscriptionEvent::TableInsertEvent {
// //                     module_name,
// //                     table_name,
// //                     inserted_row,
// //                 } => SubscriptionEvent::TableInsertEvent {
// //                     module_name,
// //                     table_name,
// //                     inserted_row,
// //                 },
// //                 SubscriptionEvent::TableUpdateEvent {
// //                     module_name,
// //                     table_name,
// //                     old_row,
// //                     new_row,
// //                 } => SubscriptionEvent::TableUpdateEvent {
// //                     module_name,
// //                     table_name,
// //                     old_row,
// //                     new_row,
// //                 },
// //                 SubscriptionEvent::TableDeleteEvent {
// //                     module_name,
// //                     table_name,
// //                     deleted_row,
// //                 } => SubscriptionEvent::TableDeleteEvent {
// //                     module_name,
// //                     table_name,
// //                     deleted_row,
// //                 },
// //             });
// //             let bytes = encode(&packet).expect("Couldn't encode network packet");
// //             client.stream.write_all(&bytes);
// //         }
// //         Ok(())
// //     }

// //     pub fn add_server_node(&mut self, node: NodeSchema) {
// //         if self.server_nodes.get(&node.id).is_some() {
// //             return;
// //         }
// //         let mut stream = TcpStream::connect(&node.adress).unwrap();
// //         println!("Connected to node {}", node.id);
// //         self.server_nodes.insert(
// //             node.id,
// //             NodeHandle {
// //                 schema: node,
// //                 stream,
// //             },
// //         );

// //         thread::spawn(move || {
// //             Self::handle_server_messages(self.server_nodes.clone());
// //         });

// //         let mut reader = BufReader::new(std::io::stdin());

// //         loop {
// //             let mut buffer = String::new();
// //             reader.read_line(&mut buffer).unwrap();

// //             if buffer.trim() == "/quit" {
// //                 break;
// //             }

// //             stream.write_all(buffer.as_bytes()).unwrap();
// //         }

// //         println!("Disconnected from server");
// //     }

// //     pub fn start_server(&self) {
// //         let listener = TcpListener::bind(&self.adress).unwrap();
// //         println!("Server listening on {}", &self.adress);

// //         let mut clients = Vec::new();

// //         for stream in listener.incoming() {
// //             let stream = stream.unwrap();
// //             println!("New client connected: {:?}", stream.peer_addr().unwrap());

// //             clients.push(stream.try_clone().unwrap());

// //             let mut clients_clone = clients.clone();
// //             thread::spawn(move || {
// //                 Self::handle_client(stream, &mut clients_clone);
// //             });
// //         }
// //     }

// //     fn start_client(&self) {
// //         let mut servers = Vec::new();
// //         for server in self.server_nodes.values() {
// //             let stream = TcpStream::connect(&server.adress).unwrap();
// //             servers.push(stream);
// //         }

// //         let server_clone = servers.clone();
// //         thread::spawn(move || {
// //             Self::handle_server_messages(server_clone);
// //         });

// //         let mut reader = BufReader::new(std::io::stdin());

// //         loop {
// //             let mut buffer = String::new();
// //             reader.read_line(&mut buffer).unwrap();

// //             if buffer.trim() == "/quit" {
// //                 break;
// //             }

// //             for server in &mut servers {
// //                 server.write_all(buffer.as_bytes()).unwrap();
// //             }
// //         }

// //         println!("Disconnected from server");
// //     }

// //     fn handle_client(stream: TcpStream, clients: &mut Vec<TcpStream>) {
// //         let mut reader = BufReader::new(&stream);

// //         loop {
// //             let mut buffer = String::new();
// //             let bytes_read = reader.read_line(&mut buffer).unwrap();

// //             if bytes_read == 0 {
// //                 break;
// //             }

// //             println!("Received message: {:?}", buffer.trim());

// //             for client in clients.iter_mut() {
// //                 if client.local_addr().unwrap() != stream.local_addr().unwrap() {
// //                     client.write_all(buffer.as_bytes()).unwrap();
// //                 }
// //             }
// //         }

// //         println!("Client disconnected: {:?}", stream.peer_addr().unwrap());
// //     }

// //     fn handle_server_messages(servers: Arc<Mutex<HashMap<NodeId, NodeHandle>>>) {
// //         loop {
// //             for (id, handle) in servers.iter_mut() {
// //                 let mut buffer = String::new();
// //                 let bytes_read = reader.read_line(&mut buffer).unwrap();

// //                 if bytes_read == 0 {
// //                     reader_to_remove = Some(idx);
// //                     break; // Skip remaining listeners to remove the reader, handle messages in next loop
// //                 }

// //                 println!("{}", buffer.trim());
// //             }
// //         }
// //     }
// // }
