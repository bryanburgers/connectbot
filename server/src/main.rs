extern crate bytes;
extern crate clap;
// extern crate chrono;
extern crate futures;
extern crate protobuf;
// extern crate rand;
// extern crate signal_hook;
extern crate tokio;
// extern crate tokio_dns;
extern crate tokio_io;
extern crate tokio_threadpool;
extern crate tokio_timer;
extern crate tokio_codec;
extern crate tokio_rustls;
extern crate uuid;

extern crate comms_shared;

mod control_server;
mod device_server;
mod world;

use clap::{Arg, App};
use tokio::net::TcpListener;
use futures::{Future, Stream};

use std::io::BufReader;
use std::fs::{self, File};

use tokio_rustls::{
    rustls::{
        Certificate, PrivateKey, ServerConfig, NoClientAuth, AllowAnyAnonymousOrAuthenticatedClient, RootCertStore,
        internal::pemfile::{ certs, rsa_private_keys }
    },
};

fn load_certs(path: &str) -> Vec<Certificate> {
    certs(&mut BufReader::new(File::open(path).unwrap())).unwrap()
}

fn load_keys(path: &str) -> Vec<PrivateKey> {
    rsa_private_keys(&mut BufReader::new(File::open(path).unwrap())).unwrap()
}

fn main() {
    let matches = App::new("comms-server")
        .version("1.0")
        .author("Bryan Burgers <bryan@burgers.io>")
        .about("Communications")
        .arg(Arg::with_name("address")
             .short("a")
             .long("address")
             .help("The address to use to accept client connections")
             .takes_value(true)
             .default_value("[::]:4004"))
        .arg(Arg::with_name("control-address")
             .short("c")
             .long("control-address")
             .help("The address to use to communicate on the control socket")
             .takes_value(true)
             .default_value("[::1]:12345"))
        .arg(Arg::with_name("ca")
             .long("ca")
             .value_name("FILE")
             .help("The location of the Certificate Authority pem/crt to use when authenticating clients. If this is not set, client-authentication is disabled")
             .takes_value(true)
             .required(false))
        .arg(Arg::with_name("cert")
             .long("cert")
             .value_name("FILE")
             .help("The location of the TLS certificate file (pem)")
             .takes_value(true)
             .required(true))
        .arg(Arg::with_name("key")
             .long("key")
             .value_name("FILE")
             .help("The location of the TLS key file (rsa)")
             .takes_value(true)
             .required(true))
        .get_matches();


    let world = world::World::shared();
    let control_server = control_server::Server::new(world.clone());
    let device_server = device_server::Server::new(world.clone());

    let device_server_future = {
        let addr = matches.value_of("address").unwrap();
        let socket_addr = addr.parse().unwrap();
        let cert_file = matches.value_of("cert").unwrap();
        let key_file = matches.value_of("key").unwrap();

        let mut config = if let Some(ca) = matches.value_of("ca") {
            let mut cert_store = RootCertStore::empty();
            let mut pem = BufReader::new(fs::File::open(ca).expect("Unable to open specified CA file"));
            cert_store.add_pem_file(&mut pem).unwrap();
            ServerConfig::new(AllowAnyAnonymousOrAuthenticatedClient::new(cert_store))
        }
        else {
            ServerConfig::new(NoClientAuth::new())
        };
        config.set_single_cert(load_certs(cert_file), load_keys(key_file).remove(0))
            .expect("invalid key or certificate");

        device_server.listen(socket_addr, config)
            .map(|_server| ())
    };

    let control_server_future = {
        let addr = matches.value_of("control-address").unwrap();
        let socket_addr = addr.parse().unwrap();
        let listener = TcpListener::bind(&socket_addr).unwrap();
        println!("Control channel listening on {}", &socket_addr);
        let server = control_server;
        let future = listener.incoming().for_each(move |connection| {
            let future = server.handle_control_connection(connection)
                .map_err(|e| println!("Warning: {}", e));
            tokio::spawn(future);

            Ok(())
        })
            .map_err(|e| println!("Error: {}", e));

        future
    };

    let lazy = futures::future::lazy(move || {
        tokio::spawn(device_server_future);
        tokio::spawn(control_server_future);

        Ok(())
    });

    tokio::run(lazy);
}

