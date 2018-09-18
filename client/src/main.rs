extern crate bytes;
extern crate clap;
extern crate chrono;
extern crate futures;
extern crate protobuf;
extern crate rand;
// extern crate signal_hook;
extern crate tokio;
extern crate tokio_dns;
extern crate tokio_io;
extern crate tokio_threadpool;
extern crate tokio_timer;

mod ssh_connection;

use ::futures::future::Future;
use ::futures::stream::Stream;

fn main() {
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
}
