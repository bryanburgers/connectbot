use std;
use tokio;
use futures::{Future, Stream};

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio_rustls::{
    TlsStream, ServerConfigExt,
    rustls::{
        self, ServerConfig,
    },
};
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

    pub fn listen(self, socket_addr: SocketAddr, server_config: ServerConfig) -> impl Future<Item=Self, Error=()> {
        let arc_config = Arc::new(server_config);

        let listener = TcpListener::bind(&socket_addr).unwrap();
        println!("Client channel listening on {}", &socket_addr);
        let future = listener.incoming()
            .map_err(|err| println!("Incoming error: {}", err))
            .fold(self, move |server, connection| {
                let addr = connection.peer_addr().unwrap();
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
                                let future = server.handle_client_connection(addr, stream)
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
}
