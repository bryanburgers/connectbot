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

mod server;
mod world;

use clap::{Arg, App};
use tokio::net::TcpListener;
use futures::{Future, Stream};
use std::sync::Arc;

use std::io::BufReader;
use std::fs::{self, File};

use tokio_rustls::{
    ServerConfigExt,
    rustls::{
        Certificate, PrivateKey, ServerConfig, AllowAnyAnonymousOrAuthenticatedClient, RootCertStore,
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
             .default_value("[::0]:12321"))
        .arg(Arg::with_name("control-address")
             .short("c")
             .long("control-address")
             .help("The address to use to communicate on the control socket")
             .takes_value(true)
             .default_value("[::0]:12345"))
        .get_matches();


    let world = world::World::shared();
    let server = server::Server::new(world.clone());
    let server_arc = Arc::new(server);

    let client_future = {
        let addr = matches.value_of("address").unwrap();
        let socket_addr = addr.parse().unwrap();

        // let mut config = ServerConfig::new(NoClientAuth::new());
        let mut cert_store = RootCertStore::empty();
        let mut pem = BufReader::new(fs::File::open("../ca.crt").unwrap());
        cert_store.add_pem_file(&mut pem).unwrap();
        let mut config = ServerConfig::new(AllowAnyAnonymousOrAuthenticatedClient::new(cert_store));
        config.set_single_cert(load_certs("../server2.pem"), load_keys("../server.key").remove(0))
            .expect("invalid key or certificate");

        let arc_config = Arc::new(config);

        let listener = TcpListener::bind(&socket_addr).unwrap();
        let server = server_arc.clone();
        let future = listener.incoming().for_each(move |connection| {
            let addr = connection.peer_addr().unwrap();
            let server = server.clone();
            arc_config.accept_async(connection)
                .and_then(move |stream| {
                    println!("{:?}", stream);
                    // {
                    //     let (_, s) = stream.get_ref();
                    //     let certs = s.get_peer_certificates();
                    //     println!("{:?}", certs);
                    // }
                    let future = server.handle_client_connection(addr, stream)
                        .map_err(|e| println!("Warning: {}", e));

                    tokio::spawn(future);

                    Ok(())
                })
                .or_else(|err| {
                    println!("Connection failed: {}", err);

                    futures::future::ok(())
                })
        })
            .map_err(|e| println!("Error: {}", e));

        future
    };

    let control_future = {
        let addr = matches.value_of("control-address").unwrap();
        let socket_addr = addr.parse().unwrap();
        let listener = TcpListener::bind(&socket_addr).unwrap();
        let server = server_arc.clone();
        let future = listener.incoming().for_each(move |connection| {
            let future = server.handle_control_connection(connection)
                .map_err(|e| println!("Warning: {}", e));
            tokio::spawn(future);

            Ok(())
        })
            .map_err(|e| println!("Error: {}", e));

        future
    };

    let cleanup_future = {
        let server = server_arc.clone();

        server.periodic_cleanup()
            .map_err(|e| println!("Failed to cleanup: {}", e))
    };


    let lazy = futures::future::lazy(move || {
        tokio::spawn(client_future);
        tokio::spawn(control_future);
        tokio::spawn(cleanup_future);

        Ok(())
    });

    tokio::run(lazy);
}

