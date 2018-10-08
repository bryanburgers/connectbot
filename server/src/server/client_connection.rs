use std;
use tokio;
use tokio::net::TcpStream;
use tokio_timer::Interval;
use tokio_codec;
use futures::{
    self, Stream, Sink, Future,
    sync::mpsc::{channel, Sender, Receiver},
};

use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::net::{IpAddr, SocketAddr};

use comms_shared::codec::Codec;
use comms_shared::protos::client;
use comms_shared::timed_connection::{TimedConnection, TimedConnectionItem, TimedConnectionOptions};

use tokio_rustls::TlsStream;
use tokio_rustls::rustls;
use uuid::Uuid;

use super::world::{self, SharedWorld};

pub struct ClientConnection {
    id: String,
    client_id: Option<String>,
    world: SharedWorld,
    back_channel_sender: Arc<Sender<()>>,
    back_channel: Receiver<()>,
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

        ClientConnection {
            id: id,
            client_id: None,
            world: world,
            back_channel_sender: sender,
            back_channel: receiver,
        }
    }

    pub fn get_handle(&self) -> ClientConnectionHandle {
        ClientConnectionHandle {
            id: self.id.clone(),
            sender: self.back_channel_sender.clone(),
        }
    }

    pub fn handle_connection<S, C>(&mut self, addr: SocketAddr, conn: TlsStream<S, C>) -> impl Future<Item=(), Error=std::io::Error>
        where S: tokio::io::AsyncWrite + tokio::io::AsyncRead + Send + 'static,
              C: rustls::Session + 'static,
    {
        // Process socket here.
        let codec: Codec<client::ServerMessage, client::ClientMessage> = Codec::new();
        let framed = tokio_codec::Decoder::framed(codec, conn);

        let (sink, stream) = framed.split();

        let stream = TimedConnection::new(stream, TimedConnectionOptions { ..Default::default() });

        let (tx, rx) = futures::sync::mpsc::channel(0);

        let sink = sink.sink_map_err(|_| ());
        let rx = rx.map_err(|_| panic!());

        let client_id = self.id.clone();
        tokio::spawn(rx.map(move |message| { println!("↓ {}: {:?}", client_id, message); message }).forward(sink).then(|result| {
            if let Err(e) = result {
                panic!("failed to write to socket: {:?}", e)
            }
            Ok(())
        }));

        let id = self.id.clone();
        let world_disconnect = self.world.clone();
        let id_disconnect = self.id.clone();

        stream.for_each(move |message| -> Box<dyn Future<Item=(), Error=std::io::Error> + Send> {
            match message {
                TimedConnectionItem::Item(message) => {
                    println!("↑ {}: {:?}", id.clone(), message);
                    /*
                    {
                        let id = id.clone();
                        let mut hash_map = state.write().unwrap();
                        hash_map.entry(id)
                            .and_modify(|client| {
                                client.last_message = Some(Instant::now());
                            });
                    }
                    */

                    if message.has_ping() {
                        let pong = client::Pong::new();
                        let mut message = client::ServerMessage::new();
                        message.set_pong(pong);

                        let f = tx.clone().send(message)
                            .map(|_| ())
                            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e)));

                        return Box::new(f);
                    }

                    if message.has_initialize() {
                        println!("Initialized!");

                        // TODO: Do stuff. Maybe disconnect any existing items. Store some data.
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
                    Box::new(futures::future::ok(()))
                },
                TimedConnectionItem::Timeout => {
                    let ping = client::Ping::new();
                    let mut response = client::ServerMessage::new();
                    response.set_ping(ping);

                    let f = tx.clone().send(response)
                        .map(|_| ())
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to send ping: {}", e)));

                    Box::new(f)
                },
            }
        })
            .and_then(move |_| {
                let uuid = id_disconnect.clone();
                let world = world_disconnect.clone();
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
