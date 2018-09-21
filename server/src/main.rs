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

extern crate comms_shared;

mod server;


use clap::{Arg, App};
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use futures::Stream;
use futures::Sink;
use futures::Future;
use std::sync::Arc;

struct Server;

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


    let server = server::Server::new();
    let server_arc = Arc::new(server);

    let client_future = {
        let addr = matches.value_of("address").unwrap();
        let socket_addr = addr.parse().unwrap();
        let listener = TcpListener::bind(&socket_addr).unwrap();
        let server = server_arc.clone();
        let future = listener.incoming().for_each(move |connection| {
            let future = server.handle_client_connection(connection)
                .map_err(|e| println!("Warning: {}", e));
            tokio::spawn(future);

            Ok(())
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

