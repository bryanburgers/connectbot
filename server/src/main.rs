extern crate bytes;
extern crate clap;
extern crate chrono;
extern crate futures;
extern crate protobuf;
// extern crate rand;
extern crate serde;
// extern crate signal_hook;
extern crate tokio;
// extern crate tokio_dns;
extern crate tokio_io;
extern crate tokio_threadpool;
extern crate tokio_timer;
extern crate tokio_codec;
extern crate tokio_rustls;
extern crate toml;
extern crate uuid;

#[macro_use]
extern crate serde_derive;

extern crate connectbot_shared;

mod config;
mod control_server;
mod device_server;
mod world;

use clap::{Arg, App, AppSettings, SubCommand};
use tokio::net::TcpListener;
use futures::{Future, Stream};
use tokio_timer::Interval;
use std::path::Path;
use std::time::Duration;
use chrono::Utc;

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
    let matches = App::new("connectbot-server")
        .version("1.0")
        .author("Bryan Burgers <bryan@burgers.io>")
        .about("Communications")
        .setting(AppSettings::SubcommandsNegateReqs)
        .subcommand(SubCommand::with_name("config")
                    .about("Generate an example config file"))
        .arg(Arg::with_name("config")
             .short("c")
             .long("config")
             .help("The location of the config file")
             .takes_value(true)
             .default_value("/etc/connectbot/server.config"))
        .get_matches();

    if let Some(_matches) = matches.subcommand_matches("config") {
        let config: config::ApplicationConfig = std::default::Default::default();
        print!("{}", toml::to_string(&config).unwrap());
        return;
    }

    let result = config::ApplicationConfig::from_file(Path::new(matches.value_of_os("config").unwrap()));
    let config = match result {
        Ok(config) => config,
        Err(string) => {
            println!("{}", string);
            std::process::exit(1);
        }
    };

    let world = world::World::shared();
    let control_server = control_server::Server::new(world.clone());
    let device_server = device_server::Server::new(world.clone());

    let device_server_future = {
        let addr = config.address;
        let socket_addr = addr.parse().expect("address must be a valid socket address");
        let cert_file = config.tls.certificate;
        let key_file = config.tls.key;

        let mut config = match config.client_authentication {
            Some(config::ClientAuthentication { ref required, ref ca }) if *required => {
                let mut cert_store = RootCertStore::empty();
                let mut pem = BufReader::new(fs::File::open(ca).expect("Unable to open specified CA file"));
                cert_store.add_pem_file(&mut pem).unwrap();
                ServerConfig::new(AllowAnyAnonymousOrAuthenticatedClient::new(cert_store))
            },
            _ => ServerConfig::new(NoClientAuth::new()),
        };

        config.set_single_cert(load_certs(&cert_file), load_keys(&key_file).remove(0))
            .expect("invalid key or certificate");

        device_server.listen(socket_addr, config)
            .map(|_server| ())
    };

    let control_server_future = {
        let addr = config.control_address;
        let socket_addr = addr.parse().expect("control_address must be a valid socket address");
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

    let cleanup_future = {
        let _world = world.clone();
        Interval::new_interval(Duration::from_secs(30)).for_each(move |_| {
            let mut world = world.write().unwrap();

            world.cleanup(Utc::now());

            Ok(())
        })
            .map_err(|e| println!("Failed to cleanup: {}", e))
    };

    let lazy = futures::future::lazy(move || {
        tokio::spawn(device_server_future);
        tokio::spawn(control_server_future);
        tokio::spawn(cleanup_future);

        Ok(())
    });

    tokio::run(lazy);
}

