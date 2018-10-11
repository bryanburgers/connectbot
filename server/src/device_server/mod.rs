use std;
use tokio;
use futures::Future;

use std::net::SocketAddr;

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
