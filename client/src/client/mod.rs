use device;
use futures::{self, Future, Sink, Stream};
use futures::sync::mpsc::{Receiver, Sender, channel};
use server_connection;
use ssh_connection::{SshConnection, SshConnectionChange, SshConnectionSettings};
use ssh_manager::SshManager;
use std;
use tokio;

use tokio_rustls::rustls::ClientConfig;

/// Internal object to store information about the client during a connection.
struct Client {
    id: String,
    successful_connections: usize,
    sender: Sender<device::ClientMessage>,
    ssh_manager: SshManager,
}

impl Client {
    fn new(id: String, sender: Sender<device::ClientMessage>) -> Client {
        let manager = SshManager::new();

        Client {
            id,
            successful_connections: 0,
            sender,
            ssh_manager: manager,
        }
    }

    /// What to do when the connection gets established successfully.
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

    /// What to do with a message that we have received from the server.
    fn on_client_message(self, mut message: device::ServerMessage) -> Box<dyn Future<Item=Self, Error=std::io::Error> + Send> {
        if !message.has_ping() && !message.has_pong() {
            // Log it! Except when it's a ping or a pong. Pings and pongs are used to keep the
            // connection alive, and happen frequently, but just add noise when they are logged.
            println!("↓ {:?}", message);
        }

        if message.has_ping() {
            // If this is a ping from the server, respond with a pong. Keep the connection alive!
            let pong = device::Pong::new();
            let mut message = device::ClientMessage::new();
            message.set_pong(pong);

            let f = self.sender.clone().send(message)
                .map(|_| self)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e)));

            return Box::new(f);
        }

        if message.has_ssh_connection() {
            // This message is telling us *something* about SSH connections. Could be enabling or
            // disabling a connection.
            let mut ssh_connection = message.take_ssh_connection();
            if ssh_connection.has_enable() {
                // Enabling one. So, we had better start up the SSH connection.
                let enable = ssh_connection.take_enable();
                let id = ssh_connection.get_id();

                return Box::new(self.on_ssh_enable(id.to_string(), enable))
            }

            if ssh_connection.has_disable() {
                // Disabling one. Tear that thing down now!
                let id = ssh_connection.get_id();
                self.on_ssh_disable(id);
            }
        }

        Box::new(futures::future::ok(self))
    }

    /// Start to establish a new SSH session.
    fn on_ssh_enable(self, id: String, enable: device::SshConnection_Enable) -> impl Future<Item=Self, Error=std::io::Error> {
        let state = self.ssh_manager.current_state(&id);
        let tx = self.sender.clone();
        let state = if let Some(state) = state {
            // This SSH connection has already started.
            match state {
                SshConnectionChange::Connecting => device::SshConnectionStatus_State::CONNECTING,
                SshConnectionChange::Connected => device::SshConnectionStatus_State::CONNECTED,
                SshConnectionChange::Disconnecting => device::SshConnectionStatus_State::DISCONNECTING,
                SshConnectionChange::Disconnected => device::SshConnectionStatus_State::DISCONNECTED,
                SshConnectionChange::Failed(_) => device::SshConnectionStatus_State::FAILED,
            }
        }
        else {
            // This must be a new SSH session. So let's kick it off.
            let manager_ref = self.ssh_manager.get_ref();
            let tx = tx.clone();
            let settings = SshConnectionSettings {
                id: id.clone(),
                host: enable.get_ssh_host().to_string(),
                port: enable.get_ssh_port() as u16,
                username: enable.get_ssh_username().to_string(),
                forward_host: enable.get_forward_host().to_string(),
                forward_port: enable.get_forward_port() as u16,
                remote_port: enable.get_remote_port() as u16,
                gateway_port: enable.get_gateway_port(),
                private_key: enable.get_ssh_key().to_string(),
            };
            let future = SshConnection::new(settings);
            manager_ref.register_handle(&id.clone(), future.handle());
            let id = id.clone();
            let future = future.for_each(move |item| {
                // Every time the SSH session state changes, we should tell the server.
                manager_ref.update_state(&id.clone(), &item);

                let mut ssh_connection_status = device::SshConnectionStatus::new();
                let state = match item {
                    SshConnectionChange::Connecting => device::SshConnectionStatus_State::CONNECTING,
                    SshConnectionChange::Connected => device::SshConnectionStatus_State::CONNECTED,
                    SshConnectionChange::Disconnecting => device::SshConnectionStatus_State::DISCONNECTING,
                    SshConnectionChange::Disconnected => device::SshConnectionStatus_State::DISCONNECTED,
                    SshConnectionChange::Failed(_) => device::SshConnectionStatus_State::FAILED,
                };
                ssh_connection_status.set_id(id.clone().into());
                ssh_connection_status.set_state(state);

                let mut client_message = device::ClientMessage::new();
                client_message.set_ssh_status(ssh_connection_status);

                tx.clone().send(client_message)
                    .map(|_| ())
                    .map_err(|err| println!("{}", err))
            });
            // Spawn the future to handle the SSH connection separately.
            tokio::spawn(future);

            device::SshConnectionStatus_State::REQUESTED
        };

        // Alright, let's send a message to the server about the current state. Even if we just
        // started the connection.
        let mut ssh_connection_status = device::SshConnectionStatus::new();
        ssh_connection_status.set_id(id.clone().into());
        ssh_connection_status.set_state(state);

        let mut client_message = device::ClientMessage::new();
        client_message.set_ssh_status(ssh_connection_status);

        tx.send(client_message)
            .map(|_| self)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "Failed to send message"))
    }

    /// Disable the SSH connection. Because we already have a stream reporting to the server when
    /// the SSH state changes, we don't need to do much here.
    fn on_ssh_disable(&self, id: &str) {
        self.ssh_manager.disable(id);
    }

    /// What to do when the connection has been idle for a while. We want to send a Ping to keep
    /// the connection alive.
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

/// The primary function that runs the client connection. Returns a future to pass to `tokio::run`.
pub fn connect(id: String, connection_details: server_connection::ConnectionDetails, tls_config: ClientConfig) -> impl Future<Item=(), Error=()> {
    // Create a new sink/stream for the connection to the server.
    let server_connection = server_connection::ServerConnection::new(connection_details, tls_config);

    // Split it into the constituent parts.
    let (sink, stream) = server_connection.split();

    // Create a new channel that we can connect to the server connection sink. Because these are
    // cloneable (and our sink/stream isn't), it's possible to pass these around easier.
    let (tx, rx): (Sender<device::ClientMessage>, Receiver<device::ClientMessage>) = channel(0);

    // Tokio::spawn expects Future<Item=(), Error=()>, so transform these by "handling" the errors.
    let sink = sink.sink_map_err(|err| panic!("{}", err));
    let rx = rx.map_err(|err| panic!("{:?}", err));

    let client = Client::new(id, tx);

    let sender_future = rx.inspect(|message| {
        if !message.has_ping() && !message.has_pong() {
            // Log all of the messages we send to the server (except the Ping/Pongs; we don't want
            // that noise when logging).
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
        // Every time we get a message from the server, what do we do with it.
        match message {
            server_connection::ServerConnectionEvent::Connecting => {
                // Just log it.
                println!("! Connecting...");
            },
            server_connection::ServerConnectionEvent::TcpConnected => {
                // Just log it.
                println!("! TCP connected");
            },
            server_connection::ServerConnectionEvent::TlsConnected => {
                // Log it.
                println!("! TLS connected");

                // And do whatever we need to do once the connection is established.
                return client.on_connected();
            },
            server_connection::ServerConnectionEvent::ConnectionFailed(i) => {
                // Just log it.
                println!("! Connection failed: {}. Trying again in {:?}.", i.err, i.duration);
            },
            server_connection::ServerConnectionEvent::Item(message) => {
                // Deal with the message.
                return client.on_client_message(message);
            },
            server_connection::ServerConnectionEvent::TimeoutWarning => {
                // Deal with the idle connection.
                return client.on_timeout_warning();
            },
        }
        Box::new(futures::future::ok(client))
    });

    stream_future.join(sender_future)
        .map(|_| ())
        .map_err(|err| panic!("{}", err))
}
