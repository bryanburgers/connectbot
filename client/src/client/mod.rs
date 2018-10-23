use device;
use futures::{self, Future, Sink, Stream};
use server_connection;
use std;

use tokio_rustls::rustls::ClientConfig;

struct Client {
    id: String,
    successful_connections: usize,
    sender: futures::sync::mpsc::Sender<device::ClientMessage>,
}

impl Client {
    fn new(id: String, sender: futures::sync::mpsc::Sender<device::ClientMessage>) -> Client {
        Client {
            id,
            successful_connections: 0,
            sender,
        }
    }

    fn on_connected(mut self) -> Box<dyn Future<Item=Self, Error=std::io::Error> + Send> {
        self.successful_connections += 1;

        // Send the initialize message.
        let mut initialize = device::Initialize::new();
        initialize.set_id(self.id.clone().into());
        initialize.set_comms_version("1.0".into());
        let mut client_message = device::ClientMessage::new();
        client_message.set_initialize(initialize);

        let f = self.sender.clone().send(client_message)
            .map(|_| self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to send ping: {}", e)));

        return Box::new(f);
    }

    fn on_client_message(self, message: device::ServerMessage) -> Box<dyn Future<Item=Self, Error=std::io::Error> + Send> {
        if !message.has_ping() && !message.has_pong() {
            println!("↓ {:?}", message);
        }

        if message.has_ping() {
            let pong = device::Pong::new();
            let mut message = device::ClientMessage::new();
            message.set_pong(pong);

            let f = self.sender.clone().send(message)
                .map(|_| self)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e)));

            return Box::new(f);
        }

        Box::new(futures::future::ok(self))
    }

    fn on_timeout_warning(self) -> Box<dyn Future<Item=Self, Error=std::io::Error> + Send> {
        let ping = device::Ping::new();
        let mut message = device::ClientMessage::new();
        message.set_ping(ping);

        let f = self.sender.clone().send(message)
            .map(|_| self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e)));

        return Box::new(f);
    }
}

pub fn connect(id: String, connection_details: server_connection::ConnectionDetails, tls_config: ClientConfig) -> impl Future<Item=(), Error=()> {
    let server_connection = server_connection::ServerConnection::new(connection_details, tls_config);

    let (sink, stream) = server_connection.split();

    let (tx, rx): (futures::sync::mpsc::Sender<device::ClientMessage>, futures::sync::mpsc::Receiver<device::ClientMessage>) = futures::sync::mpsc::channel(0);

    let sink = sink.sink_map_err(|err| panic!("{}", err));
    let rx = rx.map_err(|err| panic!("{:?}", err));

    let client = Client::new(id, tx);

    let sender_future = rx.inspect(|message| {
        if !message.has_ping() && !message.has_pong() {
            println!("↑ {:?}", message);
        }
    })
        .forward(sink)
        .then(|result| {
            if let Err(e) = result {
                println!("Something happened: {:?}", e);
                // panic!("failed to write to socket: {:?}", e)
            }
            Ok(())
        });

    let stream_future = stream.fold(client, move |client, message| {
        match message {
            server_connection::ServerConnectionEvent::Connecting => {
                println!("! Connecting...");
            },
            server_connection::ServerConnectionEvent::TcpConnected => {
                println!("! TCP connected");
            },
            server_connection::ServerConnectionEvent::TlsConnected => {
                println!("! TLS connected");

                return client.on_connected();
            },
            server_connection::ServerConnectionEvent::ConnectionFailed(i) => {
                println!("! Connection failed: {}. Trying again in {:?}.", i.err, i.duration);
            },
            server_connection::ServerConnectionEvent::Item(message) => {
                return client.on_client_message(message);
            },
            server_connection::ServerConnectionEvent::TimeoutWarning => {
                return client.on_timeout_warning();
            },
        }
        Box::new(futures::future::ok(client))
    });

    stream_future.join(sender_future)
        .map(|_| ())
        .map_err(|err| panic!("{}", err))
}
