extern crate bytes;
extern crate clap;
extern crate chrono;
extern crate futures;
extern crate protobuf;
extern crate rand;
extern crate tokio;
extern crate tokio_dns;
extern crate tokio_io;
extern crate tokio_threadpool;
extern crate tokio_timer;

mod ssh_connection;

use ::futures::future::Future;
use ::futures::stream::Stream;

fn main() {
    let stream = ssh_connection::SshConnection::new();
    let future = stream.for_each(|event| {
        let dt = chrono::Local::now();
        println!("{} {:?}!", dt.format("%Y-%m-%dT%H:%M:%S"), event);

        Ok(())
    })
        .map(|_| ())
        .map_err(|_| {
            println!("Error!");
        });

    println!("Before run");

    tokio::run(future);

    println!("After run");
}
