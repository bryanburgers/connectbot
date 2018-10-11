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

mod ssh_connection;

use clap::{Arg, App};

use futures::{Future, Stream, Sink};
use std::time::Duration;

use std::net::SocketAddr;
use std::net::IpAddr;
use connectbot_shared::codec::Codec;
use connectbot_shared::protos::device;
use connectbot_shared::timed_connection::{TimedConnection, TimedConnectionOptions, TimedConnectionItem};

use std::io::BufReader;
use tokio_rustls::{
    TlsConnector,
    rustls::{
        Certificate, ClientConfig, PrivateKey,
        internal::pemfile::{ certs, rsa_private_keys },
    },
    webpki
};
use std::fs::{self, File};
use std::sync::Arc;

fn load_certs(path: &str) -> Vec<Certificate> {
    certs(&mut BufReader::new(File::open(path).unwrap())).unwrap()
}

fn load_keys(path: &str) -> Vec<PrivateKey> {
    rsa_private_keys(&mut BufReader::new(File::open(path).unwrap())).unwrap()
}

#[derive(Clone)]
enum Connection {
    Address { address: String, port: u16 },
    AddressWithResolve { address: String, resolve: IpAddr, port: u16 },
}

impl Connection {
    fn address(&self) -> &str {
        match self {
            Connection::Address { address, .. } => &address,
            Connection::AddressWithResolve { address, .. } => &address,
        }
    }
}

impl<'a> tokio_dns::ToEndpoint<'a> for &'a Connection {
    fn to_endpoint(self) -> std::io::Result<tokio_dns::Endpoint<'a>> {
        match self {
            Connection::Address { address, port } => Ok(tokio_dns::Endpoint::Host(&address, *port)),
            Connection::AddressWithResolve { resolve, port, .. } => Ok(tokio_dns::Endpoint::SocketAddr(SocketAddr::new(resolve.clone(), *port))),
        }
    }
}

fn main() {
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
        let resolve = resolve.parse().unwrap();

        Connection::AddressWithResolve {
            address: address,
            resolve: resolve,
            port: port,
        }
    }
    else {
        Connection::Address {
            address: address,
            port: port,
        }
    };

    let mut config = ClientConfig::new();
    if let Some(cafile) = matches.value_of("cafile") {
        let mut pem = BufReader::new(fs::File::open(cafile).unwrap());
        config.root_store.add_pem_file(&mut pem).unwrap();
    }
    else {
        config.root_store.add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);
    }

    if let Some(cert) = matches.value_of("cert") {
        config.set_single_client_cert(load_certs(cert), load_keys(matches.value_of("key").unwrap()).remove(0));
    }
    let arc_config = Arc::new(config);

    tokio::run(futures::future::lazy(move || {
        connect(id, connection, arc_config);

        futures::future::ok(())
    }))

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

fn connect(id: String, connection: Connection, arc_config: Arc<ClientConfig>) {
    let connect_id_clone = id.clone();
    let connect_connection_clone = connection.clone();
    let connect_arc_config = arc_config.clone();

    let connect_future = tokio_dns::TcpStream::connect(&connection);

    let f2 = connect_future
        .and_then(move |stream| {
            let domain = connection.address();
            println!("! TCP connection established.");
            let domain = webpki::DNSNameRef::try_from_ascii_str(&domain).unwrap();
            let connector: TlsConnector = arc_config.clone().into();
            connector.connect(domain, stream)
        })
        .and_then(move |stream| {
            println!("! TLS connection established.");
            let codec: Codec<device::ClientMessage, device::ServerMessage> = Codec::new();
            let framed = tokio_codec::Decoder::framed(codec, stream);
            let (sink, stream) = framed.split();

            let (tx, rx): (futures::sync::mpsc::Sender<device::ClientMessage>, futures::sync::mpsc::Receiver<device::ClientMessage>) = futures::sync::mpsc::channel(0);

            let sink = sink.sink_map_err(|_| ());
            let rx = rx.map_err(|_| panic!());

            tokio::spawn(rx.map(|message| {
                if !message.has_ping() && !message.has_pong() {
                    println!("↑ {:?}", message);
                }
                message
            })
                .forward(sink)
                .then(|result| {
                    if let Err(e) = result {
                        println!("Something happened: {:?}", e);
                        // panic!("failed to write to socket: {:?}", e)
                    }
                    Ok(())
                }));

            let initialize_future = {
                // Send the initialize message.
                let mut initialize = device::Initialize::new();
                initialize.set_id(id.into());
                initialize.set_comms_version("1.0".into());
                let mut client_message = device::ClientMessage::new();
                client_message.set_initialize(initialize);

                let f = tx.clone().send(client_message)
                    .map(|_| ())
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to send initialize: {}", e)));

                f
            };

            let stream = TimedConnection::new(stream, TimedConnectionOptions {
                warning_level: Duration::from_millis(60_000),
                disconnect_level: Duration::from_millis(120_000),
            });

            let stream_future = stream.for_each(move |message| -> Box<dyn Future<Item=(), Error=std::io::Error> + Send> {
                match message {
                    TimedConnectionItem::Item(message) => {
                        if !message.has_ping() && !message.has_pong() {
                            println!("↓ {:?}", message);
                        }

                        if message.has_ping() {
                            let pong = device::Pong::new();
                            let mut client_message = device::ClientMessage::new();
                            client_message.set_pong(pong);

                            let f = tx.clone().send(client_message)
                                .map(|_| ())
                                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to send pong: {}", e)));

                            return Box::new(f);
                        }
                    },
                    TimedConnectionItem::Timeout => {
                        let ping = device::Ping::new();
                        let mut client_message = device::ClientMessage::new();
                        client_message.set_ping(ping);

                        let f = tx.clone().send(client_message)
                            .map(|_| ())
                            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to send ping: {}", e)));

                        return Box::new(f);
                    },
                }
                Box::new(futures::future::ok(()))
            });

            initialize_future.join(stream_future)
                .map(|_| ())
        })
        .map(|_| println!("! Disconnected."))
        .map_err(|e| println!("! Connectioned failed: {:?}", e))
        .then(move |_| {
            use std::time::{Instant, Duration};

            tokio_timer::Delay::new(Instant::now() + Duration::from_millis(15000))
                .and_then(move |_| {
                    connect(connect_id_clone, connect_connection_clone, connect_arc_config);

                    Ok(())
                })
                .map(|_| ())
                .map_err(|_| ())
        });

    println!("! Opening connection.");
    tokio::spawn(f2);
}
