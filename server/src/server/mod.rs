use std;
use tokio;
use tokio::net::TcpStream;
use tokio_timer::Interval;
use tokio_codec;
use futures::{self, Stream, Sink, Future};

use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::net::{IpAddr, SocketAddr};

use comms_shared::codec::Codec;
use comms_shared::protos::{client, control};
use comms_shared::timed_connection::{TimedConnection, TimedConnectionItem, TimedConnectionOptions};

use tokio_rustls::TlsStream;
use tokio_rustls::rustls;
use uuid::Uuid;

use super::world::{self, SharedWorld};

pub mod client_connection;

use self::client_connection::ClientConnection;

pub struct Server {
    world: SharedWorld,
    state: Arc<RwLock<HashMap<String, Connection>>>,
}

/// Information about a connected (or unconnected) client.
#[derive(Debug)]
pub struct Connection {
    connection_id: String,
    address: SocketAddr,
    connected: Instant,
    last_message: Option<Instant>,
}

pub struct ServerData {
    state: RwLock<HashMap<String, ClientData>>
}

/// An SSH connection for a client.
#[derive(Debug)]
pub struct ClientData {
    id: String,
    // Notify the actual client of
    //   1. New actions it should perform
    //   2. Tell it to disconnect if another device with the same ID connected
    // tx: Option<Sender<>>
    connection_status: ClientDataConnectionStatus,
    // Connection History (how often this device has been connected over the past 2 days)
    // connection_history: ConnectionHistory,
    // When we've last seen the device
    // last_message: Option<Instant>,
    // SSH Sessions (a list of SSH sessions that this device should be connected to, and their
    // current state)
    // ssh_sessions: SshSessions,
}

#[derive(Debug)]
enum ClientDataConnectionStatus {
    /// The client is currently connected
    Connected { address: IpAddr },
    /// The client has been connected, but is not currently connected
    Disconnected { last_seen: Instant },
    /// The client has not been seen (or has not been seen recently)
    Unknown,
}

impl Server {
    pub fn new(world: world::SharedWorld) -> Server {
        let state = HashMap::new();

        Server {
            world: world,
            state: Arc::new(RwLock::new(state)),
        }
    }

    pub fn periodic_cleanup(&self) -> impl Future<Item=(), Error=std::io::Error> {
        let state = self.state.clone();
        let world = self.world.clone();
        Interval::new_interval(Duration::from_millis(1_000)).for_each(move |_| {
            let now = Instant::now();
            {
                let mut hash_map = state.write().unwrap();
                hash_map.retain(move |_, v| {
                    if let Some(last_message) = v.last_message {
                        now.duration_since(last_message).as_secs() < 10
                    }
                    else {
                        true
                    }
                });
            }

            {
                let mut world = world.write().unwrap();
                world.devices.retain(move |_, v| {
                    true
                });
            }

            futures::future::ok(())
        })
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Interval failed: {}", e)))
    }

    pub fn handle_client_connection<S, C>(&self, addr: SocketAddr, conn: TlsStream<S, C>) -> impl Future<Item=(), Error=std::io::Error>
        where S: tokio::io::AsyncWrite + tokio::io::AsyncRead + Send + 'static,
              C: rustls::Session + 'static,
    {

        let uuid = Uuid::new_v4();
        let uuid = format!("{}", uuid);
        println!("! {}: connected from {}", uuid, &addr.ip());

        // Mark the connection time.
        {
            let mut hash_map = self.state.write().unwrap();
            hash_map.entry(uuid.clone())
                .and_modify(|client| {
                    client.connected = Instant::now();
                }).or_insert(Connection {
                    connection_id: uuid.clone(),
                    address: addr.clone(),
                    connected: Instant::now(),
                    last_message: None,
                });
        }

        {
            let mut world = self.world.write().unwrap();
            world.devices.entry(uuid.clone())
                .and_modify(|device| {
                    device.connection_status = world::ConnectionStatus::Connected { address: addr.ip() };
                }).or_insert({
                    let mut device = world::Device::new(&uuid.clone());
                    device.connection_status = world::ConnectionStatus::Connected { address: addr.ip() };
                    device
                });
        }



        let mut connection = ClientConnection::new(uuid.clone(), self.world.clone());
        let connection_handle = connection.get_handle();
        connection.handle_connection(addr, conn)
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
                        client_data.set_id(client.connection_id.clone().into());
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
