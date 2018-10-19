use std;
use tokio;
use tokio_codec;
use futures::{
    self, Stream, Sink, Future,
    sync::mpsc::{channel, Sender, Receiver},
};

use chrono::{DateTime, Utc};
use std::net::SocketAddr;

use connectbot_shared::codec::Codec;
use connectbot_shared::protos::device;
use connectbot_shared::timed_connection::{TimedConnection, TimedConnectionItem, TimedConnectionOptions};

use tokio_rustls::TlsStream;
use tokio_rustls::rustls;

use super::world::{self, SharedWorld};

use super::stream_helpers::{CancelableStream, CancelHandle, PrimarySecondaryStream};

/// An active client connection that is currently being processed
pub struct ClientConnection {
    /// The UUID of the *Connection* (not the client ID)
    id: usize,
    /// The state of the entire World
    world: SharedWorld,
    /// The IP address of the connection
    address: SocketAddr,
    /// The channel on which to send messages back to the client
    tx: Sender<device::ServerMessage>,
    /// Temporary storage for the receiver. Once the connection starts, this will be taken and
    /// replaced with None, so is mostly useless except to temporarily store it before the
    /// connection starts.
    rx: Option<Receiver<device::ServerMessage>>,
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
#[derive(Debug)]
pub struct ClientConnectionHandle {
    /// The UUID of the *Connection* (not the client)
    id: usize,
    /// The backchannel
    sender: Sender<BackchannelMessage>,
}

impl ClientConnectionHandle {
    pub fn disconnect(&self) -> impl Future<Item=(), Error=()> {
        self.sender.clone().send(BackchannelMessage::Disconnect)
            .map(|_| ())
            .map_err(|_| ())
    }

    /// Notify the client that an SSH connection should be handled
    pub fn connect_ssh(&self, id: &str) -> impl Future<Item=(), Error=()> {
        self.sender.clone().send(BackchannelMessage::SshConnect(id.to_string()))
            .map(|_| ())
            .map_err(|_| ())
    }

    /// Notify the client that an SSH disconnection should be handled
    pub fn disconnect_ssh(&self, id: &str) -> impl Future<Item=(), Error=()> {
        self.sender.clone().send(BackchannelMessage::SshDisconnect(id.to_string()))
            .map(|_| ())
            .map_err(|_| ())
    }

    pub fn get_id(&self) -> usize {
        self.id
    }
}

impl ClientConnection {
    /// Create a new client
    pub fn new(id: usize, addr: SocketAddr, world: SharedWorld) -> ClientConnection {
        let (sender, receiver) = channel(3);
        let (tx, rx) = futures::sync::mpsc::channel(0);

        ClientConnection {
            id: id,
            world: world,
            address: addr,
            tx: tx,
            rx: Some(rx),
            back_channel_sender: sender,
            back_channel: Some(receiver),
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

    fn sender_send_connect_ssh(tx: Sender<device::ServerMessage>, ssh_forward: world::SshForward) -> impl Future<Item=(), Error=std::io::Error> + Send {
        let mut enable = device::SshConnection_Enable::new();

        // TODO: Don't hardcode these.
        enable.set_ssh_host("test".into());
        enable.set_ssh_port(22);
        enable.set_ssh_username("test".into());
        // enable.set_ssh_key();

        enable.set_forward_host(ssh_forward.forward_host.clone().into());
        enable.set_forward_port(ssh_forward.forward_port as u32);
        enable.set_remote_port(ssh_forward.remote_port as u32);
        enable.set_gateway_port(ssh_forward.gateway_port);

        let mut ssh_connection = device::SshConnection::new();
        ssh_connection.set_id(ssh_forward.id.into());
        ssh_connection.set_enable(enable);

        let mut message = device::ServerMessage::new();
        message.set_ssh_connection(ssh_connection);

        tx.clone().send(message)
            .map(|_| ())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to send ssh connect: {}", e)))
    }

    fn on_client_message(mut self, mut message: device::ClientMessage) -> Box<dyn Future<Item=Self, Error=std::io::Error> + Send> {
        if !message.has_ping() && !message.has_pong() {
            println!("↑ {:4}: {:?}", &self.id, message);
        }

        self.last_message = Some(Utc::now());

        if message.has_ping() {
            let pong = device::Pong::new();
            let mut message = device::ServerMessage::new();
            message.set_pong(pong);

            let f = self.tx.clone().send(message)
                .map(|_| self)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e)));

            return Box::new(f);
            // let r : Box<dyn Future<Item=Self, Error=std::io::Error>> = Box::new(f);
            // return r
        }

        if message.has_initialize() {
            let mut initialize = message.take_initialize();
            let device_id = initialize.take_id().to_string();

            self.device_id = Some(device_id.clone());

            let previous_connection = {
                let mut world = self.world.write().unwrap();
                world.connect_device(&device_id, self.get_handle(), &self.address, Utc::now())
            };
            if let Some(previous_connection) = previous_connection {
                tokio::spawn(previous_connection.disconnect());
            }

            let forwards: Vec<world::SshForward> = {
                let world = self.world.read().unwrap();
                let device = world.devices.get(&device_id).unwrap();
                device.ssh_forwards.iter().map(|forward| forward.clone()).collect()
            };

            let future = {
                let tx = self.tx.clone();
                let futures = forwards.into_iter().map(move |forward| Self::sender_send_connect_ssh(tx.clone(), forward));
                futures::future::join_all(futures)
            };

            let future = future.map(|_| self);

            return Box::new(future);
        }

        /*
        if message.has_clients_request() {
            let clients_response = control::ClientsResponse::new();
            let mut response = control::ServerMessage::new();
            response.set_clients_response(clients_response);
            response.set_in_response_to(message.get_message_id());

            /*
            return tx.clone().send(response)
                .map(|_| ())
                .map_err(|_| ());
                */

            let f = tx.clone().send(response)
                .map(|_| ())
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e)));
                // .map_err(|_| ());

            return Box::new(f);

                /*
            let f = tx.clone().send(response)
                .map(|_| ())
                .map_err(|_| ());
            tokio::spawn(f);
            */
        }
        */

        // message_handler::handle_message(message, tx.clone(), new_state.clone())
        Box::new(futures::future::ok(self))
    }

    fn on_timeout(self) -> Box<dyn Future<Item=Self, Error=std::io::Error> + Send> {
        let ping = device::Ping::new();
        let mut response = device::ServerMessage::new();
        response.set_ping(ping);

        let f = self.tx.clone().send(response)
            .map(|_| self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to send ping: {}", e)));

        Box::new(f)
    }

    fn on_backchannel_message(mut self, message: BackchannelMessage) -> Box<dyn Future<Item=Self, Error=std::io::Error> + Send> {
        println!("! {:4}: Received backchannel message {:?}", &self.id, message);

        match message {
            BackchannelMessage::Disconnect => {
                if let Some(cancel_handle) = std::mem::replace(&mut self.cancel_handle, None) {
                    cancel_handle.cancel().unwrap();
                }

                Box::new(futures::future::ok(self))
            },
            BackchannelMessage::SshConnect(id) => {
                let forward = {
                    let world = self.world.read().unwrap();
                    let device = world.devices.get(&self.device_id.clone().expect("An ID should exist at this point")).unwrap();
                    device.ssh_forwards.find(&id).map(|forward| forward.clone())
                };
                if let Some(forward) = forward {
                    let future = Self::sender_send_connect_ssh(self.tx.clone(), forward);
                    let f = future
                        .map(|_| self);
                    Box::new(f)
                }
                else {
                    Box::new(futures::future::ok(self))
                }
            },
            _ => {
                // TODO: Connect/disconnect
                Box::new(futures::future::ok(self))
            }
        }
    }

    fn on_disconnect(self) -> impl Future<Item=(), Error=std::io::Error> {
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
    pub fn handle_connection<S, C>(mut self, conn: TlsStream<S, C>) -> impl Future<Item=(), Error=std::io::Error>
        where S: tokio::io::AsyncWrite + tokio::io::AsyncRead + Send + 'static,
              C: rustls::Session + 'static,
    {
        // Process socket here.
        let codec: Codec<device::ServerMessage, device::ClientMessage> = Codec::new();
        let framed = tokio_codec::Decoder::framed(codec, conn);

        let (client_message_sink, client_message_stream) = framed.split();

        let client_message_stream = TimedConnection::new(client_message_stream, TimedConnectionOptions { ..Default::default() });

        // In order to send things to the client, we set up a channel, and we forward that
        // receiving end directly to the client on the TlsStream.
        let client_message_sink = client_message_sink.sink_map_err(|_| ());
        let rx = std::mem::replace(&mut self.rx, None);
        let rx = rx.unwrap().map_err(|_| panic!());
        let connection_id = self.id.clone();
        let rx_forward = rx.map(move |message| {
            if !message.has_ping() && !message.has_pong() {
                println!("↓ {:4}: {:?}", connection_id, message);
            }
            message
        })
            .forward(client_message_sink)
            .then(|result| {
                if let Err(e) = result {
                    panic!("failed to write to socket: {:?}", e)
                }
                Ok(())
            });
        tokio::spawn(rx_forward);

        // Combine the back channel and the client messages into a single stream
        let back_channel_stream = std::mem::replace(&mut self.back_channel, None).unwrap();
        let back_channel_stream = back_channel_stream.map(|item| ClientBackchannelCombinedMessage::Backchannel(item))
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, format!("{:?}", err)));
        let client_message_stream = client_message_stream.map(|item| ClientBackchannelCombinedMessage::ClientMessage(item));
        // let combined = client_message_stream.select(back_channel_stream);
        let combined = PrimarySecondaryStream::new(client_message_stream, back_channel_stream);
        let (cancelable, cancel_handle) = CancelableStream::new(combined);
        self.cancel_handle = Some(cancel_handle);

        // Then handle each message as it comes in.
        cancelable.fold(self, |client_connection, message| {
            use self::ClientBackchannelCombinedMessage::*;

            match message {
                ClientMessage(TimedConnectionItem::Item(message)) => client_connection.on_client_message(message),
                ClientMessage(TimedConnectionItem::Timeout) => client_connection.on_timeout(),
                Backchannel(message) => client_connection.on_backchannel_message(message),
            }
        })
            .and_then(|client_connection| client_connection.on_disconnect())
    }
}

// A holder type for when we combine backchannel messages and client messages into the same stream.
enum ClientBackchannelCombinedMessage {
    Backchannel(BackchannelMessage),
    ClientMessage(TimedConnectionItem<device::ClientMessage>),
}

#[derive(Debug)]
enum BackchannelMessage {
    Disconnect,
    SshConnect(String),
    SshDisconnect(String),
}
