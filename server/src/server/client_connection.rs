use std;
use tokio;
use tokio_codec;
use futures::{
    self, Stream, Sink, Future,
    sync::mpsc::{channel, Sender, Receiver},
};

use std::sync::Arc;
use std::time::Instant;
use std::net::SocketAddr;

use comms_shared::codec::Codec;
use comms_shared::protos::client;
use comms_shared::timed_connection::{TimedConnection, TimedConnectionItem, TimedConnectionOptions};

use tokio_rustls::TlsStream;
use tokio_rustls::rustls;

use super::world::{self, SharedWorld};

pub struct ClientConnection {
    id: String,
    world: SharedWorld,
    tx: Sender<client::ServerMessage>,
    rx: Option<Receiver<client::ServerMessage>>,
    back_channel_sender: Arc<Sender<()>>,
    _back_channel: Receiver<()>,
}

#[derive(Debug)]
pub struct ClientConnectionHandle {
    id: String,
    sender: Arc<Sender<()>>,
}

impl ClientConnection {
    pub fn new(id: String, world: SharedWorld) -> ClientConnection {
        let (sender, receiver) = channel(3);
        let sender = Arc::new(sender);
        let (tx, rx) = futures::sync::mpsc::channel(0);

        ClientConnection {
            id: id,
            world: world,
            tx: tx,
            rx: Some(rx),
            back_channel_sender: sender,
            _back_channel: receiver,
        }
    }

    pub fn get_handle(&self) -> ClientConnectionHandle {
        ClientConnectionHandle {
            id: self.id.clone(),
            sender: self.back_channel_sender.clone(),
        }
    }

    fn handle_client_message(self, message: client::ClientMessage) -> Box<dyn Future<Item=Self, Error=std::io::Error> + Send> {
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
            println!("Initialized!");
            // Update some thingses.
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

    pub fn handle_connection<S, C>(mut self, _addr: SocketAddr, conn: TlsStream<S, C>) -> impl Future<Item=(), Error=std::io::Error>
        where S: tokio::io::AsyncWrite + tokio::io::AsyncRead + Send + 'static,
              C: rustls::Session + 'static,
    {
        // Process socket here.
        let codec: Codec<client::ServerMessage, client::ClientMessage> = Codec::new();
        let framed = tokio_codec::Decoder::framed(codec, conn);

        let (sink, stream) = framed.split();

        let stream = TimedConnection::new(stream, TimedConnectionOptions { ..Default::default() });

        let sink = sink.sink_map_err(|_| ());
        let rx = std::mem::replace(&mut self.rx, None);
        let rx = rx.unwrap().map_err(|_| panic!());

        let client_id = self.id.clone();
        tokio::spawn(rx.map(move |message| { println!("↓ {}: {:?}", client_id, message); message }).forward(sink).then(|result| {
            if let Err(e) = result {
                panic!("failed to write to socket: {:?}", e)
            }
            Ok(())
        }));

        let id = self.id.clone();

        stream.fold(self, move |client_connection, message| -> Box<dyn Future<Item=ClientConnection, Error=std::io::Error> + Send> {
            match message {
                TimedConnectionItem::Item(message) => {
                    println!("↑ {}: {:?}", id.clone(), message);
                    client_connection.handle_client_message(message)
                },
                TimedConnectionItem::Timeout => {
                    let ping = client::Ping::new();
                    let mut response = client::ServerMessage::new();
                    response.set_ping(ping);

                    let f = client_connection.tx.clone().send(response)
                        .map(|_| client_connection)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to send ping: {}", e)));

                    Box::new(f)
                },
            }
        })
            .and_then(move |client_connection| {
                let uuid = client_connection.id.clone();
                let world = client_connection.world.clone();
                {
                    let mut world = world.write().unwrap();
                    world.devices.entry(uuid.clone())
                        .and_modify(|device| {
                            device.connection_status = world::ConnectionStatus::Disconnected { last_seen: Instant::now() };
                        });
                }
                println!("Disconnect? {:?}", uuid);

                Ok(())
            })
    }
}
