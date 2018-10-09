use std;
use tokio;
use tokio::net::TcpStream;
use tokio_timer::Interval;
use tokio_codec;
use futures::{self, Stream, Sink, Future};

use std::time::Duration;
use std::net::SocketAddr;

use comms_shared::codec::Codec;
use comms_shared::protos::control;

use tokio_rustls::TlsStream;
use tokio_rustls::rustls;
use uuid::Uuid;

use super::world::{self, SharedWorld};

pub mod client_connection;
mod stream_helpers;

use self::client_connection::ClientConnection;

pub struct Server {
    world: SharedWorld,
}

impl Server {
    pub fn new(world: world::SharedWorld) -> Server {
        Server {
            world: world,
        }
    }

    pub fn periodic_cleanup(&self) -> impl Future<Item=(), Error=std::io::Error> {
        // let world = self.world.clone();
        Interval::new_interval(Duration::from_millis(1_000)).for_each(move |_| {
            /*
            {
                let mut world = world.write().unwrap();
                world.devices.retain(move |_, _v| {
                    true
                });
            }
            */

            futures::future::ok(())
        })
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Interval failed: {}", e)))
    }

    pub fn handle_client_connection<S, C>(&self, addr: SocketAddr, stream: TlsStream<S, C>) -> impl Future<Item=(), Error=std::io::Error>
        where S: tokio::io::AsyncWrite + tokio::io::AsyncRead + Send + 'static,
              C: rustls::Session + 'static,
    {

        let uuid = Uuid::new_v4();
        let uuid = format!("{}", uuid);
        println!("! {}: connected from {}", uuid, &addr.ip());

        let connection = ClientConnection::new(uuid.clone(), addr, self.world.clone());
        connection.handle_connection(stream)
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

        stream.for_each(move |message| -> Box<dyn Future<Item=(), Error=std::io::Error> + Send> {
            println!("{:?}", message);
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
