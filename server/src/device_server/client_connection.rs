use super::stream_helpers::{CancelHandle, CancelableStream, PrimarySecondaryStream};
use super::world::{self, SharedWorld};
use crate::config::SharedConfig;
use chrono::{DateTime, Utc};
use connectbot_shared::codec::Codec;
use connectbot_shared::protos::device;
use connectbot_shared::timed_connection::{
    TimedConnection, TimedConnectionItem, TimedConnectionOptions,
};
use futures::{
    self,
    sync::mpsc::{channel, Receiver, Sender},
    Future, Sink, Stream,
};
use std::net::SocketAddr;
use tokio_rustls::rustls;
use tokio_rustls::TlsStream;

/// An active client connection that is currently being processed
pub struct ClientConnection {
    config: SharedConfig,
    /// The UUID of the *Connection* (not the client ID)
    id: usize,
    /// The state of the entire World
    world: SharedWorld,
    /// The IP address of the connection
    address: SocketAddr,
    /// The channel on which to send messages back to the client
    socket_sender: Sender<device::ServerMessage>,
    /// Temporary storage for the receiver. Once the connection starts, this will be taken and
    /// replaced with None, so is mostly useless except to temporarily store it before the
    /// connection starts.
    socket_receiver: Option<Receiver<device::ServerMessage>>,
    /// The channel which other things (especially the ClientConnectionHandle) can use to send
    /// back-channel messages to this client.
    back_channel_sender: Sender<BackchannelMessage>,
    /// Temporary storage for the backchannel receiver. Once the connection starts, this will be
    /// taken and replaced with None, so it is mostly useless except to temporarily store it before
    /// the connection starts.
    back_channel: Option<Receiver<BackchannelMessage>>,
    /// The device ID for this client
    device_id: Option<String>,
    /// The last time we received any message from this client
    last_message: Option<DateTime<Utc>>,
    /// A handle that will cancel the stream
    cancel_handle: Option<CancelHandle>,
}

/// A handle to an active client connection. Certain messages can be sent on this client's back
/// channel to the client.
#[derive(Debug, Clone)]
pub struct ClientConnectionHandle {
    /// The UUID of the *Connection* (not the client)
    id: usize,
    /// The backchannel
    sender: Sender<BackchannelMessage>,
}

impl ClientConnectionHandle {
    /// Disconnect a client
    pub fn disconnect(&self) -> impl Future<Item = (), Error = ()> {
        self.sender
            .clone()
            .send(BackchannelMessage::Disconnect)
            .then(|result| match result {
                Ok(_) => Ok(()),
                Err(err) => {
                    println!("Failed to send BackchannelMessage::Disconnect: {:?}", err);
                    Ok(())
                }
            })
    }

    /// Notify the client that an SSH connection should be handled
    pub fn connect_ssh(&self, id: &str) -> impl Future<Item = (), Error = ()> {
        self.sender
            .clone()
            .send(BackchannelMessage::SshConnect(id.to_string()))
            .then(|result| match result {
                Ok(_) => Ok(()),
                Err(err) => {
                    println!("Failed to send BackchannelMessage::SshConnect: {:?}", err);
                    Ok(())
                }
            })
    }

    /// Notify the client that an SSH disconnection should be handled
    pub fn disconnect_ssh(&self, id: &str) -> impl Future<Item = (), Error = ()> {
        self.sender
            .clone()
            .send(BackchannelMessage::SshDisconnect(id.to_string()))
            .then(|result| match result {
                Ok(_) => Ok(()),
                Err(err) => {
                    println!(
                        "Failed to send BackchannelMessage::SshDisconnect: {:?}",
                        err
                    );
                    Ok(())
                }
            })
    }

    pub fn disconnect_ssh_no_future(&self, id: &str) {
        let result = self
            .sender
            .clone()
            .try_send(BackchannelMessage::SshDisconnect(id.to_string()));
        match result {
            Err(err) => println!(
                "Failed to send disconnect to device after ssh timeout: {:?}",
                err
            ),
            _ => {}
        }
    }

    /// Get the ID of the connection
    pub fn get_id(&self) -> usize {
        self.id
    }
}

impl ClientConnection {
    /// Create a new client
    pub fn new(
        id: usize,
        addr: SocketAddr,
        world: SharedWorld,
        config: SharedConfig,
    ) -> ClientConnection {
        let (back_channel_sender, back_channel_receiver) = channel(5);
        let (socket_sender, socket_receiver) = channel(5);

        ClientConnection {
            config,
            id,
            world,
            address: addr,
            socket_sender,
            socket_receiver: Some(socket_receiver),
            back_channel_sender,
            back_channel: Some(back_channel_receiver),
            device_id: None,
            last_message: None,
            cancel_handle: None,
        }
    }

    /// Get a handle to the client, which can be used to send backchannel messages.
    pub fn get_handle(&self) -> ClientConnectionHandle {
        ClientConnectionHandle {
            id: self.id.clone(),
            sender: self.back_channel_sender.clone(),
        }
    }

    fn sender_send_connect_ssh(
        tx: Sender<device::ServerMessage>,
        ssh_forward: world::SshForwardData,
        config: SharedConfig,
    ) -> impl Future<Item = (), Error = std::io::Error> + Send {
        let mut enable = device::SshConnection_Enable::new();

        enable.set_ssh_host(
            config
                .ssh
                .host
                .as_ref()
                .map(String::as_str)
                .unwrap_or("localhost")
                .into(),
        );
        enable.set_ssh_port(*config.ssh.port.as_ref().unwrap_or(&22) as u32);
        enable.set_ssh_username(
            config
                .ssh
                .user
                .as_ref()
                .map(String::as_str)
                .unwrap_or("test")
                .into(),
        );
        if let Some(ref ssh_key) = config.ssh.private_key_data {
            enable.set_ssh_key(String::as_str(ssh_key).into());
        }

        enable.set_forward_host(ssh_forward.forward_host.clone().into());
        enable.set_forward_port(ssh_forward.forward_port as u32);
        enable.set_remote_port(ssh_forward.remote_port as u32);
        enable.set_gateway_port(ssh_forward.gateway_port);

        let mut ssh_connection = device::SshConnection::new();
        ssh_connection.set_id(ssh_forward.id.clone().into());
        ssh_connection.set_enable(enable);

        let mut message = device::ServerMessage::new();
        message.set_ssh_connection(ssh_connection);

        tx.clone().send(message).map(|_| ()).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to send ssh connect: {}", e),
            )
        })
    }

    fn sender_send_disconnect_ssh(
        tx: Sender<device::ServerMessage>,
        connection_id: &str,
    ) -> impl Future<Item = (), Error = std::io::Error> + Send {
        let disable = device::SshConnection_Disable::new();

        let mut ssh_connection = device::SshConnection::new();
        ssh_connection.set_id(connection_id.into());
        ssh_connection.set_disable(disable);

        let mut message = device::ServerMessage::new();
        message.set_ssh_connection(ssh_connection);

        tx.clone().send(message).map(|_| ()).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to send ssh disconnect: {}", e),
            )
        })
    }

    /// Handle what happens when when receive a message from a client
    fn on_client_message(
        mut self,
        mut message: device::ClientMessage,
    ) -> Box<dyn Future<Item = Self, Error = std::io::Error> + Send> {
        if !message.has_ping() && !message.has_pong() {
            // Log it! (Unless it's from a ping or a pong, then don't. Too noisy.)
            println!("↑ {:4}: {:?}", &self.id, message);
        }

        // Keep track of when the last message was received. We use this when calculating when a
        // device was online and when it was offline.
        self.last_message = Some(Utc::now());

        if message.has_ping() {
            // If this is a ping, send back a pong. This keeps the connection alive.
            let pong = device::Pong::new();
            let mut message = device::ServerMessage::new();
            message.set_pong(pong);

            let f = self
                .socket_sender
                .clone()
                .send(message)
                .map(|_| self)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e)));

            return Box::new(f);
        }

        if message.has_initialize() {
            // Oh goody! The client is telling us about itself.
            let mut initialize = message.take_initialize();
            let device_id = initialize.take_id().to_string();

            // Keep track of which device this connection claims to be.
            self.device_id = Some(device_id.clone());

            // We only want to have one active connection for any given device. So if there is
            // already an existing connection, disconnect it. This happens frequently when
            // something happens to the TCP connection and we never get the RST that tells us it is
            // disconnected. But then the client retries and that gets through.
            //
            // Also, having a single active connection makes life easier when we try to send
            // commands to the device: we only need to send the command down a single network pipe.
            let previous_connection = {
                let mut world = self.world.write().unwrap();
                world.connect_device(&device_id, self.get_handle(), &self.address, Utc::now())
            };
            if let Some(previous_connection) = previous_connection {
                // We DO have a previous connection, so disconnect it.
                tokio::spawn(previous_connection.disconnect());
            }

            // Are there any SSH forwards that are already defined for this device? Let the device
            // know immediately.
            let forwards: Vec<world::SshForwardData> = {
                let world = self.world.read().unwrap();
                let device = world.devices.get(&device_id).unwrap();
                device
                    .ssh_forwards
                    .iter()
                    .filter(|forward| match forward.server_state {
                        world::SshForwardServerState::Active { .. } => true,
                        _ => false,
                    })
                    .map(|forward| forward.data())
                    .collect()
            };

            let future = {
                let socket_sender = self.socket_sender.clone();
                let config = self.config.clone();
                let futures = forwards.into_iter().map(move |forward| {
                    Self::sender_send_connect_ssh(socket_sender.clone(), forward, config.clone())
                });
                futures::future::join_all(futures)
            };

            let future = future.map(|_| self);

            return Box::new(future);
        }

        // After this point, we want to assume that any message we receive is from an authorized
        // client. If it's not, ignore the message. Maybe the client will still initialize.
        // Crossing our fingers! The other option would be to disconnect here, but let's be nice.
        if self.device_id.is_none() {
            println!("Ignoring message from non-initialized client");
            return Box::new(futures::future::ok(self));
        }

        let device_id = self.device_id.as_ref().unwrap().clone();

        if message.has_ssh_status() {
            // The client is telling us about an SSH connection it knows about.
            let ssh_status = message.take_ssh_status();
            let connection_id = ssh_status.get_id();
            let state = ssh_status.get_state();

            let future = {
                let mut world = self.world.write().unwrap();
                let device = world.devices.get_mut(&device_id).unwrap();
                let new_state = match state {
                    device::SshConnectionStatus_State::UNKNOWN_STATE => {
                        world::SshForwardClientState::Requested
                    }
                    device::SshConnectionStatus_State::REQUESTED => {
                        world::SshForwardClientState::Requested
                    }
                    device::SshConnectionStatus_State::CONNECTING => {
                        world::SshForwardClientState::Connecting
                    }
                    device::SshConnectionStatus_State::CONNECTED => {
                        world::SshForwardClientState::Connected
                    }
                    device::SshConnectionStatus_State::DISCONNECTING => {
                        world::SshForwardClientState::Disconnecting
                    }
                    device::SshConnectionStatus_State::DISCONNECTED => {
                        world::SshForwardClientState::Disconnected
                    }
                    device::SshConnectionStatus_State::FAILED => {
                        world::SshForwardClientState::Failed
                    }
                };
                match device
                    .ssh_forwards
                    .update_client_state(&connection_id, new_state)
                {
                    Ok(()) => None,
                    Err(()) => {
                        // We don't know about this SSH connection. We should tell the client to
                        // terminate it.

                        let future = Self::sender_send_disconnect_ssh(
                            self.socket_sender.clone(),
                            connection_id,
                        );
                        Some(future)
                    }
                }
            };

            if let Some(future) = future {
                return Box::new(future.map(|_| self));
            } else {
                return Box::new(futures::future::ok(self));
            }
        }

        Box::new(futures::future::ok(self))
    }

    /// Handle what happens when no messages have been received on this connection for a while. By
    /// sending a ping.
    fn on_timeout(self) -> Box<dyn Future<Item = Self, Error = std::io::Error> + Send> {
        let ping = device::Ping::new();
        let mut response = device::ServerMessage::new();
        response.set_ping(ping);

        let f = self
            .socket_sender
            .clone()
            .send(response)
            .map(|_| self)
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to send ping: {}", e),
                )
            });

        Box::new(f)
    }

    /// A backchannel message is a message that comes to this connection, but not from the
    /// connection itself. So these are things like the control channel telling us to disconnect,
    /// the control channel telling us to do something with SSH, etc.
    fn on_backchannel_message(
        mut self,
        message: BackchannelMessage,
    ) -> Box<dyn Future<Item = Self, Error = std::io::Error> + Send> {
        println!(
            "! {:4}: Received backchannel message {:?}",
            &self.id, message
        );

        match message {
            BackchannelMessage::Disconnect => {
                if let Some(cancel_handle) = std::mem::replace(&mut self.cancel_handle, None) {
                    cancel_handle.cancel().unwrap();
                }

                Box::new(futures::future::ok(self))
            }
            BackchannelMessage::SshConnect(id) => {
                let forward = {
                    let world = self.world.read().unwrap();
                    let device = world
                        .devices
                        .get(
                            &self
                                .device_id
                                .clone()
                                .expect("An ID should exist at this point"),
                        )
                        .unwrap();
                    device.ssh_forwards.find(&id).map(|forward| forward.data())
                };
                if let Some(forward) = forward {
                    let future = Self::sender_send_connect_ssh(
                        self.socket_sender.clone(),
                        forward,
                        self.config.clone(),
                    );
                    let f = future.map(|_| self);
                    Box::new(f)
                } else {
                    Box::new(futures::future::ok(self))
                }
            }
            BackchannelMessage::SshDisconnect(id) => {
                let forward = {
                    let world = self.world.read().unwrap();
                    let device = world
                        .devices
                        .get(
                            &self
                                .device_id
                                .clone()
                                .expect("An ID should exist at this point"),
                        )
                        .unwrap();
                    device.ssh_forwards.find(&id).map(|forward| forward.data())
                };
                if let Some(forward) = forward {
                    let future =
                        Self::sender_send_disconnect_ssh(self.socket_sender.clone(), &forward.id);
                    let f = future.map(|_| self);
                    Box::new(f)
                } else {
                    Box::new(futures::future::ok(self))
                }
            }
        }
    }

    /// Handle what happens when the remote client disconnects.
    fn on_disconnect(self) -> impl Future<Item = (), Error = std::io::Error> {
        let client_id = self.id;
        let world = self.world.clone();

        if let Some(device_id) = self.device_id {
            // If we know the device_id, that means we received AT least one message. So we know
            // that there was a last message to unwrap.
            let last_message = self.last_message.unwrap();

            let mut world = world.write().unwrap();
            world.disconnect_device(&device_id, client_id, last_message);
            println!("! {:4}: Disconnect {}", client_id, device_id);
        }

        futures::future::ok(())
    }

    /// Handle the TlsStream connection for this client. This consumes the client.
    pub fn handle_connection<S, C>(
        mut self,
        conn: TlsStream<S, C>,
    ) -> impl Future<Item = (), Error = std::io::Error>
    where
        S: tokio::io::AsyncWrite + tokio::io::AsyncRead + Send + 'static,
        C: rustls::Session + 'static,
    {
        // Process socket here.
        let codec: Codec<device::ServerMessage, device::ClientMessage> = Codec::new();
        let framed = tokio_codec::Decoder::framed(codec, conn);

        let (client_message_sink, client_message_stream) = framed.split();

        let client_message_stream = TimedConnection::new(
            client_message_stream,
            TimedConnectionOptions {
                ..Default::default()
            },
        );

        // In order to send things to the client, we set up a channel, and we forward that
        // receiving end directly to the client on the TlsStream.
        let client_message_sink = client_message_sink.sink_map_err(|err| panic!("{:?}", err));
        let socket_receiver = std::mem::replace(&mut self.socket_receiver, None);
        let socket_receiver = socket_receiver.unwrap().map_err(|err| panic!("{:?}", err));
        let connection_id = self.id.clone();
        let socket_forward = socket_receiver
            .inspect(move |message| {
                if !message.has_ping() && !message.has_pong() {
                    println!("↓ {:4}: {:?}", connection_id, message);
                }
            })
            .forward(client_message_sink)
            .then(|result| {
                if let Err(e) = result {
                    panic!("failed to write to socket: {:?}", e)
                }
                Ok(())
            });

        // Combine the back channel and the client messages into a single stream
        let back_channel_stream = std::mem::replace(&mut self.back_channel, None).unwrap();
        let back_channel_stream = back_channel_stream
            .map(|item| ClientBackchannelCombinedMessage::Backchannel(item))
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, format!("{:?}", err)));
        let client_message_stream =
            client_message_stream.map(|item| ClientBackchannelCombinedMessage::ClientMessage(item));
        // let combined = client_message_stream.select(back_channel_stream);
        let combined = PrimarySecondaryStream::new(client_message_stream, back_channel_stream);
        let (cancelable, cancel_handle) = CancelableStream::new(combined);
        self.cancel_handle = Some(cancel_handle);

        // Then handle each message as it comes in.
        let send = cancelable
            .fold(self, |client_connection, message| {
                use self::ClientBackchannelCombinedMessage::*;

                match message {
                    ClientMessage(TimedConnectionItem::Item(message)) => {
                        client_connection.on_client_message(message)
                    }
                    ClientMessage(TimedConnectionItem::Timeout) => client_connection.on_timeout(),
                    Backchannel(message) => client_connection.on_backchannel_message(message),
                }
            })
            .and_then(|client_connection| client_connection.on_disconnect());

        send.join(socket_forward).map(|_| ())
    }
}

/// A holder type for when we combine backchannel messages and client messages into the same
/// stream.
enum ClientBackchannelCombinedMessage {
    Backchannel(BackchannelMessage),
    ClientMessage(TimedConnectionItem<device::ClientMessage>),
}

/// The type of message that a backchannel can send to this connection.
#[derive(Debug)]
enum BackchannelMessage {
    Disconnect,
    SshConnect(String),
    SshDisconnect(String),
}
