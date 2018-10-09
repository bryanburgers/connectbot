use std;
use tokio;
use tokio_codec;
use futures::{
    self, Stream, Sink, Future,
    sync::mpsc::{channel, Sender, Receiver},
};

use std::time::Instant;
use std::net::SocketAddr;

use comms_shared::codec::Codec;
use comms_shared::protos::client;
use comms_shared::timed_connection::{TimedConnection, TimedConnectionItem, TimedConnectionOptions};

use tokio_rustls::TlsStream;
use tokio_rustls::rustls;

use super::world::{self, SharedWorld};

use super::stream_helpers::{CancelableStream, CancelHandle, PrimarySecondaryStream};

/// An active client connection that is currently being processed
pub struct ClientConnection {
    /// The UUID of the *Connection* (not the client ID)
    id: String,
    /// The state of the entire World
    world: SharedWorld,
    /// The IP address of the connection
    address: SocketAddr,
    /// The channel on which to send messages back to the client
    tx: Sender<client::ServerMessage>,
    /// Temporary storage for the receiver. Once the connection starts, this will be taken and
    /// replaced with None, so is mostly useless except to temporarily store it before the
    /// connection starts.
    rx: Option<Receiver<client::ServerMessage>>,
    /// The channel which other things (especially the ClientConnectionHandle) can use to send
    /// back-channel messages to this client.
    back_channel_sender: Sender<()>,
    /// Temporary storage for the backchannel receiver. Once the connection starts, this will be
    /// taken and replaced with None, so it is mostly useless except to temporarily store it before
    /// the connection starts.
    back_channel: Option<Receiver<()>>,
    /// The device ID for this client
    device_id: Option<String>,
    /// The last time we received any message from this client
    last_message: Option<Instant>,
    /// A handle that will cancel the stream
    cancel_handle: Option<CancelHandle>,
}

/// A handle to an active client connection. Certain messages can be sent on this client's back
/// channel to the client.
#[derive(Debug)]
pub struct ClientConnectionHandle {
    /// The UUID of the *Connection* (not the client)
    id: String,
    /// The backchannel
    sender: Sender<()>,
}

impl ClientConnectionHandle {
    pub fn disconnect(&self) -> impl Future<Item=(), Error=()> {
        self.sender.clone().send(())
            .map(|_| ())
            .map_err(|_| ())
    }
}

impl ClientConnection {
    /// Create a new client
    pub fn new(id: String, addr: SocketAddr, world: SharedWorld) -> ClientConnection {
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

    fn on_client_message(mut self, mut message: client::ClientMessage) -> Box<dyn Future<Item=Self, Error=std::io::Error> + Send> {
        println!("↑ {}: {:?}", &self.id, message);

        self.last_message = Some(Instant::now());

        if message.has_ping() {
            let pong = client::Pong::new();
            let mut message = client::ServerMessage::new();
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
            let device_id_2 = device_id.clone();

            self.device_id = Some(device_id);
            // Update some thingses.

            {
                let mut world = self.world.write().unwrap();
                world.devices.entry(device_id_2.clone())
                    .and_modify(|device| {
                        device.connection_status = world::ConnectionStatus::Connected { address: self.address.ip() };
                        let previous_connection = std::mem::replace(&mut device.active_connection, Some(self.get_handle()));
                        if let Some(previous_connection) = previous_connection {
                            tokio::spawn(previous_connection.disconnect());
                        }
                    })
                    .or_insert({
                        let mut device = world::Device::new(&device_id_2.clone());
                        device.active_connection = Some(self.get_handle());
                        device.connection_status = world::ConnectionStatus::Connected { address: self.address.ip() };
                        device
                    });
            }
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
        let ping = client::Ping::new();
        let mut response = client::ServerMessage::new();
        response.set_ping(ping);

        let f = self.tx.clone().send(response)
            .map(|_| self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to send ping: {}", e)));

        Box::new(f)
    }

    fn on_backchannel_message(mut self, message: ()) -> Box<dyn Future<Item=Self, Error=std::io::Error> + Send> {
        println!("! {}: Received backchannel message {:?}", &self.id, message);

        if let Some(cancel_handle) = std::mem::replace(&mut self.cancel_handle, None) {
            cancel_handle.cancel().unwrap();
        }

        Box::new(futures::future::ok(self))
    }

    fn on_disconnect(self) -> impl Future<Item=(), Error=std::io::Error> {
        let uuid = self.id;
        let world = self.world.clone();

        if let Some(device_id) = self.device_id {
            // If we know the device_id, that means we received AT least one message. So we know
            // that there was a last message to unwrap.
            let last_message = self.last_message.unwrap();

            let mut world = world.write().unwrap();
            world.devices.entry(device_id.clone())
                .and_modify(|device| {
                    if let Some(ref active_connection) = device.active_connection {
                        // If there is an active connection, only disconnect if the active
                        // connection is the current connection. Otherwise, our connection was
                        // replaced by a different connection, and so we don't want to replace THAT
                        // connection status with disconnected.
                        if active_connection.id == device_id {
                            device.connection_status = world::ConnectionStatus::Disconnected { last_seen: last_message };
                        }
                    }
                    else {
                        // There is no active connection. Mark as disconnected.
                        device.connection_status = world::ConnectionStatus::Disconnected { last_seen: last_message };
                    }
                });
            println!("! {}: Disconnect {}", uuid, device_id);
        }

        futures::future::ok(())
    }

    /// Handle the TlsStream connection for this client. This consumes the client.
    pub fn handle_connection<S, C>(mut self, conn: TlsStream<S, C>) -> impl Future<Item=(), Error=std::io::Error>
        where S: tokio::io::AsyncWrite + tokio::io::AsyncRead + Send + 'static,
              C: rustls::Session + 'static,
    {
        // Process socket here.
        let codec: Codec<client::ServerMessage, client::ClientMessage> = Codec::new();
        let framed = tokio_codec::Decoder::framed(codec, conn);

        let (client_message_sink, client_message_stream) = framed.split();

        let client_message_stream = TimedConnection::new(client_message_stream, TimedConnectionOptions { ..Default::default() });

        // In order to send things to the client, we set up a channel, and we forward that
        // receiving end directly to the client on the TlsStream.
        let client_message_sink = client_message_sink.sink_map_err(|_| ());
        let rx = std::mem::replace(&mut self.rx, None);
        let rx = rx.unwrap().map_err(|_| panic!());
        let connection_id = self.id.clone();
        let rx_forward = rx.map(move |message| { println!("↓ {}: {:?}", connection_id, message); message })
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
    Backchannel(()),
    ClientMessage(TimedConnectionItem<client::ClientMessage>),
}
