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
use std::time::Duration;

use tokio::net::TcpStream;
use comms_shared::codec::Codec;
use comms_shared::protos::client;
use comms_shared::timed_connection::{TimedConnection, TimedConnectionOptions, TimedConnectionItem};

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

        tokio::spawn(rx.map(|message| { println!("↑ {:?}", message); message }).forward(sink).then(|result| {
            if let Err(e) = result {
                panic!("failed to write to socket: {:?}", e)
            }
            Ok(())
        }));

        let stream = TimedConnection::new(stream, TimedConnectionOptions {
            warning_level: Duration::from_millis(60_000),
            disconnect_level: Duration::from_millis(120_000),
        });

        stream.for_each(move |message| -> Box<dyn Future<Item=(), Error=std::io::Error> + Send> {
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
