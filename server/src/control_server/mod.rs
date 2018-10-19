use std;
use tokio;
use tokio::net::TcpStream;
use tokio_codec;
use futures::{self, Stream, Sink, Future};

use connectbot_shared::codec::Codec;
use connectbot_shared::protos::control;

use super::world::{self, SharedWorld};

pub struct Server {
    world: SharedWorld,
}

impl Server {
    pub fn new(world: world::SharedWorld) -> Server {
        Server {
            world: world,
        }
    }

    pub fn handle_control_connection(&self, conn: TcpStream) -> impl Future<Item=(), Error=std::io::Error> {
        // Process socket here.
        let codec: Codec<control::ServerMessage, control::ClientMessage> = Codec::new();
        let framed = tokio_codec::Decoder::framed(codec, conn);
        let (sink, stream) = framed.split();

        let (tx, rx) = futures::sync::mpsc::channel(0);

        let sink = sink.sink_map_err(|_| ());
        let rx = rx.map_err(|_| panic!());

        tokio::spawn(rx.forward(sink).then(|result| {
            if let Err(e) = result {
                panic!("failed to write to socket: {:?}", e)
            }
            Ok(())
        }));

        let world = self.world.clone();

        stream.for_each(move |mut message| -> Box<dyn Future<Item=(), Error=std::io::Error> + Send> {
            if message.has_clients_request() {
                let mut clients = Vec::new();
                {
                    let world = world.read().unwrap();
                    for device in world.devices.values() {
                        // println!("{:?} {:?}", key, value);
                        let mut client_data = control::ClientsResponse_Client::new();
                        client_data.set_id(device.id.clone().into());
                        if let world::ConnectionStatus::Connected { ref address } = device.connection_status {
                            client_data.set_address(address.to_string().into());
                        }

                        let mut connection_history = Vec::new();
                        for connection_history_item in device.connection_history.iter() {
                            let mut history_item = control::ClientsResponse_ConnectionHistoryItem::new();
                            match connection_history_item {
                                world::ConnectionHistoryItem::Closed { connected_at, last_message, address } => {
                                    history_item.set_field_type(control::ClientsResponse_ConnectionHistoryType::CLOSED);
                                    history_item.set_connected_at(connected_at.timestamp() as u64);
                                    history_item.set_last_message(last_message.timestamp() as u64);
                                    history_item.set_address(address.to_string().into());
                                },
                                world::ConnectionHistoryItem::Open { connected_at, address, .. } => {
                                    history_item.set_field_type(control::ClientsResponse_ConnectionHistoryType::OPEN);
                                    history_item.set_connected_at(connected_at.timestamp() as u64);
                                    history_item.set_address(address.to_string().into());
                                },
                            }
                            connection_history.push(history_item);
                        }
                        client_data.set_connection_history(connection_history.into());

                        clients.push(client_data);
                    }
                }

                let mut clients_response = control::ClientsResponse::new();
                clients_response.set_clients(clients.into());

                let mut response = control::ServerMessage::new();
                response.set_clients_response(clients_response);
                response.set_in_response_to(message.get_message_id());

                let f = tx.clone().send(response)
                    .map(|_| ())
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e)));

                return Box::new(f);
            }

            if message.has_create_device() {
                let device_id: String = message.take_create_device().get_device_id().into();

                let r = {
                    let mut world = world.write().unwrap();
                    match world.create_device(&device_id) {
                        Ok(_) => control::CreateDeviceResponse_Response::CREATED,
                        Err(_) => control::CreateDeviceResponse_Response::EXISTS,
                    }
                };

                let mut create_device_response = control::CreateDeviceResponse::new();
                create_device_response.set_response(r);

                let mut response = control::ServerMessage::new();
                response.set_create_device_response(create_device_response);
                response.set_in_response_to(message.get_message_id());

                let f = tx.clone().send(response)
                    .map(|_| ())
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e)));

                return Box::new(f);
            }

            if message.has_remove_device() {
                let device_id: String = message.take_remove_device().get_device_id().into();

                let r = {
                    let mut world = world.write().unwrap();
                    let is_connected = world.devices
                        .get(&device_id)
                        .map(|device| device.is_connected())
                        .unwrap_or(false);

                    if !is_connected {
                        match world.devices.remove(&device_id) {
                            Some(_) => control::RemoveDeviceResponse_Response::REMOVED,
                            None => control::RemoveDeviceResponse_Response::NOT_FOUND,
                        }
                    }
                    else {
                        control::RemoveDeviceResponse_Response::ACTIVE
                    }
                };

                let mut remove_device_response = control::RemoveDeviceResponse::new();
                remove_device_response.set_response(r);

                let mut response = control::ServerMessage::new();
                response.set_remove_device_response(remove_device_response);
                response.set_in_response_to(message.get_message_id());

                let f = tx.clone().send(response)
                    .map(|_| ())
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e)));

                return Box::new(f);
            }

            // message_handler::handle_message(message, tx.clone(), new_state.clone())
            Box::new(futures::future::ok(()))
        })
    }
}
