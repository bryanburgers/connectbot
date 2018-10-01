use std;
use tokio;
use tokio::net::TcpStream;
use tokio_timer::Interval;
use tokio_codec;
use futures::{self, Stream, Sink, Future};

use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::net::SocketAddr;

use comms_shared::codec::Codec;
use comms_shared::protos::{client, control};
use comms_shared::timed_connection::{TimedConnection, TimedConnectionItem, TimedConnectionOptions};

use tokio_rustls::TlsStream;
use tokio_rustls::rustls;

pub struct Server {
    state: Arc<RwLock<HashMap<String, Client>>>,
}

/// Information about a connected (or unconnected) client.
#[derive(Debug)]
pub struct Client {
    id: String,
    address: SocketAddr,
    connected: Instant,
    last_message: Option<Instant>,
}

/// An SSH connection for a client.
#[derive(Debug)]
pub struct ClientConnection {
}

impl Server {
    pub fn new() -> Server {
        let state = HashMap::new();

        Server {
            state: Arc::new(RwLock::new(state)),
        }
    }

    pub fn periodic_cleanup(&self) -> impl Future<Item=(), Error=std::io::Error> {
        let state = self.state.clone();
        Interval::new_interval(Duration::from_millis(1_000)).for_each(move |_| {
            let now = Instant::now();
            let mut hash_map = state.write().unwrap();
            hash_map.retain(move |_, v| {
                if let Some(last_message) = v.last_message {
                    now.duration_since(last_message).as_secs() < 10
                }
                else {
                    true
                }
            });

            futures::future::ok(())
        })
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Interval failed: {}", e)))
    }

    pub fn handle_client_connection<S, C>(&self, addr: SocketAddr, conn: TlsStream<S, C>) -> impl Future<Item=(), Error=std::io::Error>
        where S: tokio::io::AsyncWrite + tokio::io::AsyncRead + Send + 'static,
              C: rustls::Session + 'static,
    {

        // TODO: Get ID from TLS certificate
        let id = "abcd".to_string();
        println!("! {}: connected from {}", id, &addr);

        // Mark the connection time.
        {
            let mut hash_map = self.state.write().unwrap();
            hash_map.entry(id.clone())
                .and_modify(|client| {
                    client.connected = Instant::now();
                }).or_insert(Client {
                    id: id.clone(),
                    address: addr.clone(),
                    connected: Instant::now(),
                    last_message: None,
                });
        }


        // Process socket here.
        let codec: Codec<client::ServerMessage, client::ClientMessage> = Codec::new();
        let framed = tokio_codec::Decoder::framed(codec, conn);
        let (sink, stream) = framed.split();

        let stream = TimedConnection::new(stream, TimedConnectionOptions { ..Default::default() });

        let (tx, rx) = futures::sync::mpsc::channel(0);

        let sink = sink.sink_map_err(|_| ());
        let rx = rx.map_err(|_| panic!());

        let client_id = id.clone();
        tokio::spawn(rx.map(move |message| { println!("↓ {}: {:?}", client_id, message); message }).forward(sink).then(|result| {
            if let Err(e) = result {
                panic!("failed to write to socket: {:?}", e)
            }
            Ok(())
        }));

        let state = self.state.clone();
        let id = id.clone();

        stream.for_each(move |message| -> Box<dyn Future<Item=(), Error=std::io::Error> + Send> {
            match message {
                TimedConnectionItem::Item(message) => {
                    println!("↑ {}: {:?}", id.clone(), message);
                    {
                        let id = id.clone();
                        let mut hash_map = state.write().unwrap();
                        hash_map.entry(id)
                            .and_modify(|client| {
                                client.last_message = Some(Instant::now());
                            });
                    }

                    if message.has_ping() {
                        let pong = client::Pong::new();
                        let mut message = client::ServerMessage::new();
                        message.set_pong(pong);

                        let f = tx.clone().send(message)
                            .map(|_| ())
                            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e)));

                        return Box::new(f);
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
                println!("Disconnect?");

                Ok(())
            })
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

        let state = self.state.clone();

        stream.for_each(move |message| -> Box<dyn Future<Item=(), Error=std::io::Error> + Send> {
            println!("{:?}", message);
            if message.has_clients_request() {
                let mut clients = Vec::new();
                {
                    let hash_map = state.read().unwrap();
                    for client in hash_map.values() {
                        // println!("{:?} {:?}", key, value);
                        let mut client_data = control::ClientsResponse_Client::new();
                        client_data.set_id(client.id.clone().into());
                        // TODO: This should be the IpAddr, not the SocketAddr. But I'm on a plane
                        // and can't look up how to get just the IpAddr part from the SocketAddr
                        // part.
                        client_data.set_address(client.address.to_string().into());
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

            // message_handler::handle_message(message, tx.clone(), new_state.clone())
            Box::new(futures::future::ok(()))
        })
    }
}
