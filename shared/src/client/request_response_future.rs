use std;
use futures;
use futures::prelude::*;
use tokio::net::TcpStream;
use tokio_codec;
use tokio_dns;

use super::codec;
use super::protos;

type ClientCodec = codec::Codec<protos::control::ClientMessage, protos::control::ServerMessage>;

pub struct RequestResponseFuture {
    state: RequestResponseFutureState,
}

impl RequestResponseFuture {
    pub fn new(addr: &str, message: protos::control::ClientMessage) -> RequestResponseFuture {
        RequestResponseFuture {
            state: RequestResponseFutureState::new(message, addr)
        }
    }
}

enum RequestResponseFutureState {
    Uninitialized,
    Initial(String, protos::control::ClientMessage),
    Connecting(protos::control::ClientMessage, Box<dyn Future<Item=TcpStream, Error=std::io::Error> + Send + 'static>),
    Sending(futures::sink::Send<tokio_codec::Framed<TcpStream, ClientCodec>>),
    Waiting(futures::stream::StreamFuture<tokio_codec::Framed<TcpStream, ClientCodec>>),
}

// Holy cow, implementing manual futures is a huge pain! async/await should make this considerably
// easier, right?
impl RequestResponseFutureState {
    fn new(message: protos::control::ClientMessage, addr: &str) -> RequestResponseFutureState {
        RequestResponseFutureState::Initial(addr.to_string(), message)
    }

    fn connect(message: protos::control::ClientMessage, addr: &str) -> RequestResponseFutureState {
        RequestResponseFutureState::Connecting(message, tokio_dns::TcpStream::connect(&addr[..]))
    }

    fn send(message: protos::control::ClientMessage, stream: TcpStream) -> RequestResponseFutureState {
        let codec = ClientCodec::new();
        let framed = tokio_codec::Decoder::framed(codec, stream);

        let future = framed.send(message);
        RequestResponseFutureState::Sending(future)
    }

    fn wait(codec: tokio_codec::Framed<TcpStream, ClientCodec>) -> RequestResponseFutureState {
        let future = codec.into_future();

        RequestResponseFutureState::Waiting(future)
    }
}

impl Future for RequestResponseFuture {
    type Item = protos::control::ServerMessage;
    type Error = std::io::Error;

    fn poll(&mut self) -> Result<Async<Self::Item>, Self::Error> {
        use self::RequestResponseFutureState::*;

        loop {
            let state = std::mem::replace(&mut self.state, Uninitialized);

            match state {
                Uninitialized => {
                    unreachable!("WHAT");
                },
                Initial(addr, msg) => {
                    self.state = RequestResponseFutureState::connect(msg, &addr[..]);
                },
                Connecting(msg, mut f) => {
                    match f.poll()? {
                        Async::Ready(stream) => {
                            self.state = RequestResponseFutureState::send(msg, stream);
                        },
                        Async::NotReady => {
                            self.state = Connecting(msg, f);
                            return Ok(Async::NotReady);
                        }
                    }
                },
                Sending(mut f) => {
                    match f.poll()? {
                        Async::Ready(codec) => {
                            self.state = RequestResponseFutureState::wait(codec);
                        },
                        Async::NotReady => {
                            self.state = Sending(f);
                            return Ok(Async::NotReady);
                        }
                    }
                }
                Waiting(mut f) => {
                    match f.poll() {
                        Ok(Async::Ready((message, codec))) => {
                            if let Some(message) = message {
                                if message.get_in_response_to() == 1 {
                                    return Ok(Async::Ready(message));
                                }
                                else {
                                    self.state = RequestResponseFutureState::wait(codec);
                                }
                            }
                            else {
                                return Err(std::io::Error::new(std::io::ErrorKind::Other, "Stream ended unexpectedly"));
                            }
                        },
                        Ok(Async::NotReady) => {
                            self.state = Waiting(f);
                            return Ok(Async::NotReady);
                        },
                        Err((err, _)) => {
                            // We don't need to do anything with the stream anymore, right?
                            return Err(err);
                        },
                    }
                },
            }
        }
    }
}
