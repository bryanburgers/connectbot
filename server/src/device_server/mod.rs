//! Handling of the device server. The device server is the one that remote clients talk to.

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

/// Server information
pub struct Server {
    /// The next connection ID (we give each connection a unique connection ID)
    next_connection_id: usize,
    /// Information about the world
    world: SharedWorld,
    /// config.toml information
    config: SharedConfig,
}

impl Server {
    /// Create a new server.
    pub fn new(world: world::SharedWorld, config: SharedConfig) -> Server {
        Server {
            world: world,
            next_connection_id: 1,
            config,
        }
    }

    /// Handle listening on this server. Returns a function that returns once all of the listening
    /// is done. (Which is pretty much never.)
    pub fn listen(self, socket_addr: SocketAddr, server_config: ServerConfig) -> impl Future<Item=Self, Error=()> {
        let arc_config = Arc::new(server_config);

        let listener = TcpListener::bind(&socket_addr).unwrap();
        println!("Client channel listening on {}", &socket_addr);
        let future = listener.incoming()
            .map_err(|err| println!("Incoming error: {}", err))
            .fold(self, move |mut server, connection| {
                // We received a new connection. Log and accept it.
                let addr = connection.peer_addr().unwrap_or_else(|err| {
                    println!("Failed to get peer address: {}", err);
                    "[::]:0".parse().unwrap()
                });
                arc_config.accept_async(connection)
                    .then(move |stream| {
                        match stream {
                            Ok(stream) => {
                                // Yeah, connection accepted. Make a client connection out of it,
                                // and then let client connection fully handle it.
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

    /// Wrap a TLS connection into a future, and let ClientConnection fully handle everything that
    /// happens with the client connection.
    pub fn handle_client_connection<S, C>(&self, connection_id: usize, addr: SocketAddr, stream: TlsStream<S, C>) -> impl Future<Item=(), Error=std::io::Error>
        where S: tokio::io::AsyncWrite + tokio::io::AsyncRead + Send + 'static,
              C: rustls::Session + 'static,
    {
        println!("! {:4}: connected from {}", connection_id, &addr.ip());

        let connection = ClientConnection::new(connection_id, addr, self.world.clone(), self.config.clone());
        connection.handle_connection(stream)
    }
}
