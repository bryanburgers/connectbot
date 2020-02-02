use futures;
use futures::prelude::*;
use std;
use tokio::net::TcpStream;
use tokio_codec;
use tokio_dns;

use super::codec;
use super::protos;

type ClientCodec = codec::Codec<protos::control::ClientMessage, protos::control::ServerMessage>;

/// A single future that represents sending a request to the control server and waiting for a
/// response.
pub struct RequestResponseFuture {
    state: RequestResponseFutureState,
}

impl RequestResponseFuture {
    /// Create a new future that will send the given message, and will return when a response is
    /// received.
    pub fn new(addr: &str, message: protos::control::ClientMessage) -> RequestResponseFuture {
        RequestResponseFuture {
            state: RequestResponseFutureState::new(message, addr),
        }
    }
}

/// Internal state machine of the RequestResponse future.
enum RequestResponseFutureState {
    Uninitialized,
    /// Initial state
    Initial(String, protos::control::ClientMessage),
    /// Connecting to the server
    Connecting(
        protos::control::ClientMessage,
        Box<dyn Future<Item = TcpStream, Error = std::io::Error> + Send + 'static>,
    ),
    /// Sending the request to the server
    Sending(futures::sink::Send<tokio_codec::Framed<TcpStream, ClientCodec>>),
    /// Waiting for the server to respond
    Waiting(futures::stream::StreamFuture<tokio_codec::Framed<TcpStream, ClientCodec>>),
}

// Holy cow, implementing manual futures is a huge pain! async/await should make this considerably
// easier, right?
impl RequestResponseFutureState {
    /// Transition the internal state to the initial state.
    fn new(message: protos::control::ClientMessage, addr: &str) -> RequestResponseFutureState {
        RequestResponseFutureState::Initial(addr.to_string(), message)
    }

    /// Transition the internal state to Connecting by connecting to the given address. We need to
    /// keep the client message around somewhere, so that gets passed here, too.
    fn connect(message: protos::control::ClientMessage, addr: &str) -> RequestResponseFutureState {
        RequestResponseFutureState::Connecting(message, tokio_dns::TcpStream::connect(&addr[..]))
    }

    /// Transition the internal state to Sending by sending the message across the stream.
    fn send(
        message: protos::control::ClientMessage,
        stream: TcpStream,
    ) -> RequestResponseFutureState {
        let codec = ClientCodec::new();
        let framed = tokio_codec::Decoder::framed(codec, stream);

        let future = framed.send(message);
        RequestResponseFutureState::Sending(future)
    }

    /// Transition the internal state to waiting.
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
                }
                Initial(addr, msg) => {
                    // First time this future is polled. Connect.
                    self.state = RequestResponseFutureState::connect(msg, &addr[..]);
                }
                Connecting(msg, mut f) => {
                    match f.poll()? {
                        Async::Ready(stream) => {
                            // We're connected! Send the message.
                            self.state = RequestResponseFutureState::send(msg, stream);
                        }
                        Async::NotReady => {
                            // Not connected yet. Keep waiting.
                            self.state = Connecting(msg, f);
                            return Ok(Async::NotReady);
                        }
                    }
                }
                Sending(mut f) => {
                    match f.poll()? {
                        Async::Ready(codec) => {
                            // The message has been sent! Wait for the response.
                            self.state = RequestResponseFutureState::wait(codec);
                        }
                        Async::NotReady => {
                            // Still sending. Keep waiting.
                            self.state = Sending(f);
                            return Ok(Async::NotReady);
                        }
                    }
                }
                Waiting(mut f) => {
                    match f.poll() {
                        Ok(Async::Ready((message, codec))) => {
                            // The server responded with a message.
                            if let Some(message) = message {
                                // Did it respond to the message we sent?
                                if message.get_in_response_to() == 1 {
                                    // It did! Resolve the future with this message!
                                    return Ok(Async::Ready(message));
                                } else {
                                    // No. Keep waiting for a new message.
                                    self.state = RequestResponseFutureState::wait(codec);
                                }
                            } else {
                                // Oh, something else happened. Oops.
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::Other,
                                    "Stream ended unexpectedly",
                                ));
                            }
                        }
                        Ok(Async::NotReady) => {
                            // Still no message. Keep waiting.
                            self.state = Waiting(f);
                            return Ok(Async::NotReady);
                        }
                        Err((err, _)) => {
                            // We don't need to do anything with the stream anymore, right?
                            return Err(err);
                        }
                    }
                }
            }
        }
    }
}
