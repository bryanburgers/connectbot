extern crate bytes;
extern crate clap;
extern crate chrono;
extern crate futures;
extern crate protobuf;
extern crate rand;
// extern crate signal_hook;
extern crate tokio;
extern crate tokio_codec;
extern crate tokio_dns;
extern crate tokio_io;
extern crate tokio_threadpool;
extern crate tokio_timer;
extern crate tokio_rustls;
extern crate connectbot_shared;
extern crate webpki_roots;

mod server_connection;
// mod ssh_connection;

use clap::{Arg, App};

use futures::{Future, Stream, Sink};

use std::net::IpAddr;
use connectbot_shared::protos::device;

use rand::RngCore;
use std::io::BufReader;
use tokio_rustls::{
    rustls::{
        Certificate, ClientConfig, PrivateKey,
        internal::pemfile::{ certs, rsa_private_keys },
    },
};
use std::fs::{self, File};

fn load_certs(path: &str) -> Vec<Certificate> {
    certs(&mut BufReader::new(File::open(path).unwrap())).unwrap()
}

fn load_keys(path: &str) -> Vec<PrivateKey> {
    rsa_private_keys(&mut BufReader::new(File::open(path).unwrap())).unwrap()
}

fn main() -> Result<(), std::io::Error> {
    let matches = App::new("connectbot-client")
        .version("1.0")
        .author("Bryan Burgers <bryan@burgers.io>")
        .about("The client")
        .arg(Arg::with_name("id")
             .long("id")
             .value_name("IDENTIFIER")
             .help("Set the unique identifier of the device")
             .takes_value(true)
             .required(true))
        .arg(Arg::with_name("host")
             .long("host")
             .value_name("HOST")
             .help("The hostname of the server. This is used both to determine which server to connect to AND how to do TLS validation. To use only for TLS validation, use --resolve to override the IP address of the server.")
             .takes_value(true)
             .required(true))
        .arg(Arg::with_name("resolve")
             .long("resolve")
             .value_name("IP")
             .help("Provide a custom address for the connection. Using this, you can get correct TLS validation with --address, but still connect to a different IP address.")
             .takes_value(true)
             .validator(|s| {
                 s.parse::<IpAddr>()
                     .map(|_| ())
                     .map_err(|_| format!("'{}' could not be parsed as a valid IP address", s))
             }))
        .arg(Arg::with_name("port")
             .long("port")
             .value_name("PORT")
             .help("The port of the server.")
             .takes_value(true)
             .default_value("4004")
             .validator(|s| {
                 s.parse::<u16>()
                     .map(|_| ())
                     .map_err(|_| format!("'{}' could not be parsed as a valid port number", s))
             }))
        .arg(Arg::with_name("cafile")
             .long("cafile")
             .value_name("FILE")
             .help("The certificate authority to use to validate the server. If this is not included, the server will be validated against the device's root certificate store")
             .takes_value(true))
        .arg(Arg::with_name("cert")
             .long("cert")
             .value_name("FILE")
             .help("The location of the TLS certificate file (pem), if doing client authentication")
             .takes_value(true)
             .requires("key"))
        .arg(Arg::with_name("key")
             .long("key")
             .value_name("FILE")
             .help("The location of the TLS key file (rsa), if doing client authentication")
             .takes_value(true))
        .get_matches();

    let id = matches.value_of("id").unwrap().to_string();

    let address = matches.value_of("host").unwrap().to_string();
    let port = matches.value_of("port").unwrap().parse().unwrap();
    let connection = if let Some(resolve) = matches.value_of("resolve") {
        let resolve = resolve.parse()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "Failed to parse --resolve as an address"))?;

        server_connection::ConnectionDetails::AddressWithResolve {
            address: address,
            resolve: resolve,
            port: port,
        }
    }
    else {
        server_connection::ConnectionDetails::Address {
            address: address,
            port: port,
        }
    };

    let mut tls_config = ClientConfig::new();
    if let Some(cafile) = matches.value_of("cafile") {
        let mut pem = BufReader::new(fs::File::open(cafile)?);
        tls_config.root_store.add_pem_file(&mut pem).unwrap();
    }
    else {
        tls_config.root_store.add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);
    }

    if let Some(cert) = matches.value_of("cert") {
        tls_config.set_single_client_cert(load_certs(cert), load_keys(matches.value_of("key").unwrap()).remove(0));
    }

    // Sometimes, Tokio fails to initialize because a thread pool panics. The thread pool panics
    // because the random number generator is not initialized yet. However, Tokio swallows panics,
    // so the application keeps running, doing nothing. In order to work around this, manually try
    // the random number generator, so we can panic BEFORE we get to tokio::run.
    check_rng_initialized()?;

    tokio::run(connect(id, connection, tls_config));

    Ok(())

    /*
    let lazy = futures::lazy(|| {
        let stream = ssh_connection::SshConnection::new();
        let handle = stream.handle();
        let future = stream.for_each(|event| {
            let dt = chrono::Local::now();
            println!("{} {:?}!", dt.format("%Y-%m-%dT%H:%M:%S"), event);

            Ok(())
        })
            .map(|_| ())
            .map_err(|_| {
                println!("Error!");
            });

        tokio::spawn(future);

        let timeout_future = tokio_timer::Delay::new(::std::time::Instant::now() + ::std::time::Duration::from_millis(30_000))
            .and_then(move |_| {
                println!("Disconnecting!");
                handle.disconnect();

                Ok(())
            })
            .map_err(|_| {
                println!("Error!");
            });

        tokio::spawn(timeout_future);

        Ok(())
    });

    println!("Before run");

    tokio::run(lazy);

    println!("After run");
    */
}

fn check_rng_initialized() -> Result<(), std::io::Error> {
    // Sometimes, Tokio fails to initialize because a thread pool panics. The thread pool panics
    // because the random number generator is not initialized yet. However, Tokio swallows panics,
    // so the application keeps running, doing nothing. In order to work around this, manually try
    // the random number generator, so we can panic BEFORE we get to tokio::run.
    let mut x = [0; 1];
    rand::thread_rng().try_fill_bytes(&mut x)?;

    Ok(())
}

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

fn connect(id: String, connection_details: server_connection::ConnectionDetails, tls_config: ClientConfig) -> impl Future<Item=(), Error=()> {
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
