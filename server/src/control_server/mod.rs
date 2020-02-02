//! Handling of the control server. The control server is the one that local controlling processes
//! (like connectbot-ctrl and connectbot-web) use to talk to the server.

use futures::{self, Future, Sink, Stream};
use std;
use tokio;
use tokio::net::TcpStream;
use tokio_codec;

use connectbot_shared::codec::Codec;
use connectbot_shared::protos::control;

use super::world::{self, SharedWorld};

/// The control server.
pub struct Server {
    world: SharedWorld,
}

impl Server {
    /// Create a new control server
    pub fn new(world: world::SharedWorld) -> Server {
        Server { world: world }
    }

    /// Create a future that handles a new control connection
    pub fn handle_control_connection(
        &self,
        conn: TcpStream,
    ) -> impl Future<Item = (), Error = std::io::Error> {
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

        stream.for_each(
            move |mut message| -> Box<dyn Future<Item = (), Error = std::io::Error> + Send> {
                if message.has_clients_request() {
                    // Return the full list of clients and their statuses.
                    let mut clients = Vec::new();
                    {
                        let world = world.read().unwrap();
                        for device in world.devices.values() {
                            // println!("{:?} {:?}", key, value);
                            let mut client_data = control::ClientsResponse_Client::new();
                            client_data.set_id(device.id.clone().into());
                            client_data.set_name(device.name.clone().into());
                            if let world::ConnectionStatus::Connected { ref address } =
                                device.connection_status
                            {
                                client_data.set_address(address.to_string().into());
                            }

                            let mut connections = Vec::new();
                            for forward in device.ssh_forwards.iter() {
                                let mut connection = control::ClientsResponse_Connection::new();
                                connection.set_id(forward.id.clone().into());
                                connection.set_state(match forward.client_state {
                                    world::SshForwardClientState::Requested => {
                                        control::ClientsResponse_ClientState::REQUESTED
                                    }
                                    world::SshForwardClientState::Connecting => {
                                        control::ClientsResponse_ClientState::CONNECTING
                                    }
                                    world::SshForwardClientState::Connected => {
                                        control::ClientsResponse_ClientState::CONNECTED
                                    }
                                    world::SshForwardClientState::Disconnecting => {
                                        control::ClientsResponse_ClientState::DISCONNECTING
                                    }
                                    world::SshForwardClientState::Disconnected => {
                                        control::ClientsResponse_ClientState::DISCONNECTED
                                    }
                                    world::SshForwardClientState::Failed => {
                                        control::ClientsResponse_ClientState::FAILED
                                    }
                                });
                                match forward.server_state {
                                    world::SshForwardServerState::Active { until } => {
                                        connection.set_active(
                                            control::ClientsResponse_ActiveState::ACTIVE,
                                        );
                                        connection.set_active_until(until.timestamp() as u64);
                                    }
                                    world::SshForwardServerState::Inactive { since } => {
                                        connection.set_active(
                                            control::ClientsResponse_ActiveState::INACTIVE,
                                        );
                                        connection.set_active_until(since.timestamp() as u64);
                                    }
                                }
                                connection.set_forward_host(forward.forward_host.clone().into());
                                connection.set_forward_port(forward.forward_port as u32);
                                connection.set_remote_port(
                                    forward
                                        .remote_port
                                        .as_ref()
                                        .map_or(0, |item| item.value() as u32),
                                );
                                connection.set_gateway_port(forward.gateway_port);
                                connections.push(connection);
                            }
                            client_data.set_connections(connections.into());

                            let mut connection_history = Vec::new();
                            for connection_history_item in device.connection_history.iter() {
                                let mut history_item =
                                    control::ClientsResponse_ConnectionHistoryItem::new();
                                match connection_history_item {
                                    world::ConnectionHistoryItem::Closed {
                                        connected_at,
                                        last_message,
                                        address,
                                    } => {
                                        history_item.set_field_type(
                                            control::ClientsResponse_ConnectionHistoryType::CLOSED,
                                        );
                                        history_item
                                            .set_connected_at(connected_at.timestamp() as u64);
                                        history_item
                                            .set_last_message(last_message.timestamp() as u64);
                                        history_item.set_address(address.to_string().into());
                                    }
                                    world::ConnectionHistoryItem::Open {
                                        connected_at,
                                        address,
                                        ..
                                    } => {
                                        history_item.set_field_type(
                                            control::ClientsResponse_ConnectionHistoryType::OPEN,
                                        );
                                        history_item
                                            .set_connected_at(connected_at.timestamp() as u64);
                                        history_item.set_address(address.to_string().into());
                                    }
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

                    let f = tx.clone().send(response).map(|_| ()).map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e))
                    });

                    return Box::new(f);
                }

                if message.has_ssh_connection() {
                    let mut ssh_connection = message.take_ssh_connection();
                    let device_id = ssh_connection.get_device_id().to_string();

                    if ssh_connection.has_enable() {
                        let enable = ssh_connection.take_enable();
                        let mut world = world.write().unwrap();

                        let device = world.devices.get_mut(&device_id);

                        let mut ssh_connection_response = control::SshConnectionResponse::new();

                        let mut backchannel_future = None;

                        if let Some(device) = device {
                            let forward_host = enable.get_forward_host();
                            // TODO: Make sure this is non-zero u16
                            let forward_port = enable.get_forward_port();
                            let gateway_port = enable.get_gateway_port();
                            let forward = device.ssh_forwards.create(
                                forward_host.into(),
                                forward_port as u16,
                                gateway_port,
                            );

                            ssh_connection_response
                                .set_status(control::SshConnectionResponse_Status::SUCCESS);
                            ssh_connection_response.set_connection_id(forward.id.clone().into());
                            ssh_connection_response.set_remote_port(
                                forward
                                    .remote_port
                                    .as_ref()
                                    .map_or(0, |item| item.value() as u32),
                            );

                            if let Some(ref handle) = device.active_connection {
                                backchannel_future = Some(handle.connect_ssh(&forward.id.clone()));
                            }
                        } else {
                            // ERROR
                            ssh_connection_response
                                .set_status(control::SshConnectionResponse_Status::ERROR);
                        }

                        let mut response = control::ServerMessage::new();
                        response.set_ssh_connection_response(ssh_connection_response);
                        response.set_in_response_to(message.get_message_id());

                        return match backchannel_future {
                            Some(future) => {
                                let tx = tx.clone();
                                let f = future
                                    .map_err(|_| {
                                        std::io::Error::new(
                                            std::io::ErrorKind::Other,
                                            "Failed to send backchannel message",
                                        )
                                    })
                                    .and_then(move |_| {
                                        tx.clone().send(response).map(|_| ()).map_err(|e| {
                                            std::io::Error::new(
                                                std::io::ErrorKind::Other,
                                                format!("{}", e),
                                            )
                                        })
                                    });

                                Box::new(f)
                            }
                            None => {
                                let f = tx.clone().send(response).map(|_| ()).map_err(|e| {
                                    std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e))
                                });

                                Box::new(f)
                            }
                        };
                    }

                    if ssh_connection.has_disable() {
                        let disable = ssh_connection.take_disable();
                        let mut world = world.write().unwrap();

                        let device = world.devices.get_mut(&device_id);

                        let mut ssh_connection_response = control::SshConnectionResponse::new();

                        let mut backchannel_future = None;

                        if let Some(device) = device {
                            let connection_id = disable.get_connection_id();

                            device.ssh_forwards.disconnect(connection_id);

                            if let Some(ref handle) = device.active_connection {
                                backchannel_future =
                                    Some(handle.disconnect_ssh(&connection_id.clone()));
                            }

                            ssh_connection_response
                                .set_status(control::SshConnectionResponse_Status::SUCCESS);
                        } else {
                            ssh_connection_response
                                .set_status(control::SshConnectionResponse_Status::ERROR);
                        }

                        let mut response = control::ServerMessage::new();
                        response.set_ssh_connection_response(ssh_connection_response);
                        response.set_in_response_to(message.get_message_id());

                        return match backchannel_future {
                            Some(future) => {
                                let tx = tx.clone();
                                let f = future
                                    .map_err(|_| {
                                        std::io::Error::new(
                                            std::io::ErrorKind::Other,
                                            "Failed to send backchannel message",
                                        )
                                    })
                                    .and_then(move |_| {
                                        tx.clone().send(response).map(|_| ()).map_err(|e| {
                                            std::io::Error::new(
                                                std::io::ErrorKind::Other,
                                                format!("{}", e),
                                            )
                                        })
                                    });

                                Box::new(f)
                            }
                            None => {
                                let f = tx.clone().send(response).map(|_| ()).map_err(|e| {
                                    std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e))
                                });

                                Box::new(f)
                            }
                        };
                    }

                    if ssh_connection.has_extend_timeout() {
                        let extend = ssh_connection.take_extend_timeout();
                        let mut world = world.write().unwrap();

                        let device = world.devices.get_mut(&device_id);

                        let mut ssh_connection_response = control::SshConnectionResponse::new();

                        if let Some(device) = device {
                            let connection_id = extend.get_connection_id();

                            device.ssh_forwards.extend(connection_id);

                            ssh_connection_response
                                .set_status(control::SshConnectionResponse_Status::SUCCESS);
                        } else {
                            ssh_connection_response
                                .set_status(control::SshConnectionResponse_Status::ERROR);
                        }

                        let mut response = control::ServerMessage::new();
                        response.set_ssh_connection_response(ssh_connection_response);
                        response.set_in_response_to(message.get_message_id());

                        let f = tx.clone().send(response).map(|_| ()).map_err(|e| {
                            std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e))
                        });

                        return Box::new(f);
                    }
                }

                if message.has_create_device() {
                    let create_device = message.take_create_device();
                    let device_id = create_device.get_device_id();

                    let r = {
                        let mut world = world.write().unwrap();
                        match world.create_device(device_id) {
                            Ok(_) => control::CreateDeviceResponse_Response::CREATED,
                            Err(_) => control::CreateDeviceResponse_Response::EXISTS,
                        }
                    };

                    let mut create_device_response = control::CreateDeviceResponse::new();
                    create_device_response.set_response(r);

                    let mut response = control::ServerMessage::new();
                    response.set_create_device_response(create_device_response);
                    response.set_in_response_to(message.get_message_id());

                    let f = tx.clone().send(response).map(|_| ()).map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e))
                    });

                    return Box::new(f);
                }

                if message.has_remove_device() {
                    let device_id: String = message.take_remove_device().get_device_id().into();

                    let r = {
                        let mut world = world.write().unwrap();
                        let is_connected = world
                            .devices
                            .get(&device_id)
                            .map(|device| device.is_connected())
                            .unwrap_or(false);

                        if !is_connected {
                            match world.devices.remove(&device_id) {
                                Some(_) => control::RemoveDeviceResponse_Response::REMOVED,
                                None => control::RemoveDeviceResponse_Response::NOT_FOUND,
                            }
                        } else {
                            control::RemoveDeviceResponse_Response::ACTIVE
                        }
                    };

                    let mut remove_device_response = control::RemoveDeviceResponse::new();
                    remove_device_response.set_response(r);

                    let mut response = control::ServerMessage::new();
                    response.set_remove_device_response(remove_device_response);
                    response.set_in_response_to(message.get_message_id());

                    let f = tx.clone().send(response).map(|_| ()).map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e))
                    });

                    return Box::new(f);
                }

                if message.has_set_name() {
                    let set_name = message.take_set_name();
                    let device_id = set_name.get_device_id().to_string();
                    let name = set_name.get_name().to_string();

                    let mut world = world.write().unwrap();

                    let device = world.devices.get_mut(&device_id);

                    let mut set_name_response = control::SetNameResponse::new();

                    if let Some(device) = device {
                        device.name = name;

                        set_name_response.set_status(control::SetNameResponse_Status::SUCCESS);
                    } else {
                        set_name_response.set_status(control::SetNameResponse_Status::NOT_FOUND);
                    }

                    let mut response = control::ServerMessage::new();
                    response.set_set_name_response(set_name_response);
                    response.set_in_response_to(message.get_message_id());

                    let f = tx.clone().send(response).map(|_| ()).map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e))
                    });

                    return Box::new(f);
                }

                // message_handler::handle_message(message, tx.clone(), new_state.clone())
                Box::new(futures::future::ok(()))
            },
        )
    }
}
