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
extern crate comms_shared;
extern crate webpki_roots;

mod ssh_connection;

use clap::{Arg, App};

use futures::{Future, Stream, Sink};
use std::time::Duration;

use tokio::net::TcpStream;
use std::net::SocketAddr;
use comms_shared::codec::Codec;
use comms_shared::protos::client;
use comms_shared::timed_connection::{TimedConnection, TimedConnectionOptions, TimedConnectionItem};

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

fn main() {
    let matches = App::new("comms-client")
        .version("1.0")
        .author("Bryan Burgers <bryan@burgers.io>")
        .about("The client")
        .arg(Arg::with_name("ca")
             .long("ca")
             .value_name("FILE")
             .help("The certificate authority to use to validate the server. If this is not included, the server will be validated against the device's root certificate store")
             .takes_value(true)
             .requires("domain"))
        .arg(Arg::with_name("domain")
             .long("domain")
             .value_name("DOMAIN")
             .help("The DNS name to allow when validating the server's TLS connection (if --ca is specified)")
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
        .arg(Arg::with_name("id")
             .short("i")
             .long("id")
             .value_name("IDENTIFIER")
             .help("Set the unique identifier of the device")
             .takes_value(true)
             .required(true))
        .get_matches();

    let id = matches.value_of("id").unwrap().to_string();

    let addr = "[::1]:12321".parse().unwrap();
    // let addr = "167.99.112.36:443".parse().unwrap();
    let mut config = ClientConfig::new();
    if let Some(ca) = matches.value_of("ca") {
        let mut pem = BufReader::new(fs::File::open(ca).unwrap());
        config.root_store.add_pem_file(&mut pem).unwrap();
    }
    else {
        config.root_store.add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);
    }

    if let Some(cert) = matches.value_of("cert") {
        config.set_single_client_cert(load_certs(cert), load_keys(matches.value_of("key").unwrap()).remove(0));
    }
    let arc_config = Arc::new(config);
    // TODO: If --ca is not set, this should be the hostname of addr
    let domain = matches.value_of("domain").unwrap_or("comms-server").to_string();

    tokio::run(futures::future::lazy(move || {
        connect(id, addr, domain, arc_config);

        futures::future::ok(())
    }))

    /*
    let connect = TcpStream::connect(&addr);


    let f2 = connect
        .and_then(move |stream| {
            let domain = webpki::DNSNameRef::try_from_ascii_str(&domain).unwrap();
            let connector: TlsConnector = arc_config.clone().into();
            connector.connect(domain, stream)
        })
        .and_then(move |stream| {
            let codec: Codec<client::ClientMessage, client::ServerMessage> = Codec::new();
            let framed = tokio_codec::Decoder::framed(codec, stream);
            let (sink, stream) = framed.split();

            let (tx, rx) = futures::sync::mpsc::channel(0);

            let sink = sink.sink_map_err(|_| ());
            let rx = rx.map_err(|_| panic!());

            tokio::spawn(rx.map(|message| { println!("↑ {:?}", message); message }).forward(sink).then(|result| {
                if let Err(e) = result {
                    println!("Something happened: {:?}", e);
                    // panic!("failed to write to socket: {:?}", e)
                }
                Ok(())
            }));

            let initialize_future = {
                // Send the initialize message.
                let mut initialize = client::Initialize::new();
                initialize.set_id(id.into());
                initialize.set_comms_version("1.0".into());
                let mut client_message = client::ClientMessage::new();
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
                        println!("↓ {:?}", message);

                        if message.has_ping() {
                            let pong = client::Pong::new();
                            let mut client_message = client::ClientMessage::new();
                            client_message.set_pong(pong);

                            let f = tx.clone().send(client_message)
                                .map(|_| ())
                                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to send pong: {}", e)));

                            return Box::new(f);
                        }
                    },
                    TimedConnectionItem::Timeout => {
                        let ping = client::Ping::new();
                        let mut client_message = client::ClientMessage::new();
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
        });


    tokio::run(f2.map_err(|e| println!("Connectioned failed: {:?}", e)));
    */

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

fn connect(id: String, addr: SocketAddr, domain: String, arc_config: Arc<ClientConfig>) {
    let connect_id_clone = id.clone();
    let connect_addr_clone = addr.clone();
    let connect_domain_clone = domain.clone();
    let connect_arc_config = arc_config.clone();

    let connect_future = TcpStream::connect(&addr);

    let f2 = connect_future
        .and_then(move |stream| {
            println!("! TCP connection established.");
            let domain = webpki::DNSNameRef::try_from_ascii_str(&domain).unwrap();
            let connector: TlsConnector = arc_config.clone().into();
            connector.connect(domain, stream)
        })
        .and_then(move |stream| {
            println!("! TLS connection established.");
            let codec: Codec<client::ClientMessage, client::ServerMessage> = Codec::new();
            let framed = tokio_codec::Decoder::framed(codec, stream);
            let (sink, stream) = framed.split();

            let (tx, rx) = futures::sync::mpsc::channel(0);

            let sink = sink.sink_map_err(|_| ());
            let rx = rx.map_err(|_| panic!());

            tokio::spawn(rx.map(|message| { println!("↑ {:?}", message); message }).forward(sink).then(|result| {
                if let Err(e) = result {
                    println!("Something happened: {:?}", e);
                    // panic!("failed to write to socket: {:?}", e)
                }
                Ok(())
            }));

            let initialize_future = {
                // Send the initialize message.
                let mut initialize = client::Initialize::new();
                initialize.set_id(id.into());
                initialize.set_comms_version("1.0".into());
                let mut client_message = client::ClientMessage::new();
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
                        println!("↓ {:?}", message);

                        if message.has_ping() {
                            let pong = client::Pong::new();
                            let mut client_message = client::ClientMessage::new();
                            client_message.set_pong(pong);

                            let f = tx.clone().send(client_message)
                                .map(|_| ())
                                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to send pong: {}", e)));

                            return Box::new(f);
                        }
                    },
                    TimedConnectionItem::Timeout => {
                        let ping = client::Ping::new();
                        let mut client_message = client::ClientMessage::new();
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
                    connect(connect_id_clone, connect_addr_clone, connect_domain_clone, connect_arc_config);

                    Ok(())
                })
                .map(|_| ())
                .map_err(|_| ())
        });

    println!("! Opening connection.");
    tokio::spawn(f2);
}
