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
extern crate comms_shared;

mod ssh_connection;

use futures::{Future, Stream, Sink};

use tokio::net::TcpStream;
use comms_shared::codec::Codec;
use comms_shared::protos::client;

fn main() {
    let addr = "[::1]:12321".parse().unwrap();
    let connect = TcpStream::connect(&addr);

    let f2 = connect.and_then(|stream| {
        println!("{:?}", stream);

        let codec: Codec<client::ClientMessage, client::ServerMessage> = Codec::new();
        let framed = tokio_codec::Decoder::framed(codec, stream);
        let (sink, stream) = framed.split();

        let (tx, rx) = futures::sync::mpsc::channel(0);

        let sink = sink.sink_map_err(|_| ());
        let rx = rx.map_err(|_| panic!());

        tokio::spawn(rx.forward(sink).then(|result| {
            if let Err(e) = result {
                panic!("failed to write to socket: {:?}", e)
            }
            Ok(())
        }));

        stream.for_each(move |message| -> Box<dyn Future<Item=(), Error=std::io::Error> + Send> {
            println!("{:?}", message);
            Box::new(futures::future::ok(()))
        })
    });


    tokio::run(f2.map_err(|e| println!("{:?}", e)));

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
