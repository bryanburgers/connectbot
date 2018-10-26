use std;
use tokio;
use futures::{Future, Stream};

use config::SharedConfig;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio_rustls::{
    TlsStream, ServerConfigExt,
    rustls::{
        self, ServerConfig,
    },
};

use super::world::{self, SharedWorld};

pub mod client_connection;
mod stream_helpers;

use self::client_connection::ClientConnection;

pub struct Server {
    next_connection_id: usize,
    world: SharedWorld,
    config: SharedConfig,
}

impl Server {
    pub fn new(world: world::SharedWorld, config: SharedConfig) -> Server {
        Server {
            world: world,
            next_connection_id: 1,
            config,
        }
    }

    pub fn listen(self, socket_addr: SocketAddr, server_config: ServerConfig) -> impl Future<Item=Self, Error=()> {
        let arc_config = Arc::new(server_config);

        let listener = TcpListener::bind(&socket_addr).unwrap();
        println!("Client channel listening on {}", &socket_addr);
        let future = listener.incoming()
            .map_err(|err| println!("Incoming error: {}", err))
            .fold(self, move |mut server, connection| {
                let addr = connection.peer_addr().unwrap_or_else(|err| {
                    println!("Failed to get peer address: {}", err);
                    "[::]:0".parse().unwrap()
                });
                // let server = server.clone();
                arc_config.accept_async(connection)
                    .then(move |stream| {
                        match stream {
                            Ok(stream) => {
                                // {
                                //     let (_, s) = stream.get_ref();
                                //     let certs = s.get_peer_certificates();
                                //     println!("{:?}", certs);
                                // }
                                let connection_id = server.next_connection_id;
                                server.next_connection_id = server.next_connection_id.wrapping_add(1);
                                let future = server.handle_client_connection(connection_id, addr, stream)
                                    .map_err(|e| println!("Warning: {}", e));

                                tokio::spawn(future);

                                Ok(server)
                            },
                            Err(err) => {
                                println!("Connection failed: {}", err);

                                Ok(server)
                            }
                        }
                    })
                    // .map_err(|e| println!("Error: {}", e))
            });

        future
    }

    pub fn handle_client_connection<S, C>(&self, connection_id: usize, addr: SocketAddr, stream: TlsStream<S, C>) -> impl Future<Item=(), Error=std::io::Error>
        where S: tokio::io::AsyncWrite + tokio::io::AsyncRead + Send + 'static,
              C: rustls::Session + 'static,
    {
        println!("! {:4}: connected from {}", connection_id, &addr.ip());

        let connection = ClientConnection::new(connection_id, addr, self.world.clone(), self.config.clone());
        connection.handle_connection(stream)
    }
}
