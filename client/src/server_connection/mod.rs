use connectbot_shared::codec::Codec;
use connectbot_shared::protos::device;
use connectbot_shared::timed_connection::{
    TimedConnection, TimedConnectionItem, TimedConnectionOptions,
};
use futures::{stream, Async, AsyncSink, Future, Poll, Sink, StartSend, Stream};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio_codec;
use tokio_dns;
use tokio_timer::Delay;

use tokio_rustls::{self, rustls::ClientConfig, webpki, TlsConnector};

/// Information about how to connect to the server.
#[derive(Clone)]
pub enum ConnectionDetails {
    /// Connect to the server using an address and port.
    Address { address: String, port: u16 },
    /// Connect to the server using an IP address and port, using the address to check the TLS
    /// certificate.
    AddressWithResolve {
        address: String,
        resolve: IpAddr,
        port: u16,
    },
}

impl ConnectionDetails {
    fn address(&self) -> &str {
        match self {
            ConnectionDetails::Address { address, .. } => &address,
            ConnectionDetails::AddressWithResolve { address, .. } => &address,
        }
    }
}

impl<'a> tokio_dns::ToEndpoint<'a> for &'a ConnectionDetails {
    fn to_endpoint(self) -> std::io::Result<tokio_dns::Endpoint<'a>> {
        match self {
            ConnectionDetails::Address { address, port } => {
                Ok(tokio_dns::Endpoint::Host(&address, *port))
            }
            ConnectionDetails::AddressWithResolve { resolve, port, .. } => Ok(
                tokio_dns::Endpoint::SocketAddr(SocketAddr::new(resolve.clone(), *port)),
            ),
        }
    }
}

/// A future for the server connection.
pub struct ServerConnection {
    connection_details: ConnectionDetails,
    arc_config: Arc<ClientConfig>,
    /// Whether a disconnect has been requested.
    _disconnect: Arc<AtomicBool>,
    /// The number of consecutive failures, used for backoff
    failures: usize,
    /// The state machine
    state: ServerConnectionStateMachine,
    /// If the server isn't connected, it can cache up to one message to send later. This is needed
    /// for the Sink implementation.
    sink_buffer: Option<device::ClientMessage>,
}

impl ServerConnection {
    pub fn new(
        connection_details: ConnectionDetails,
        tls_config: ClientConfig,
    ) -> ServerConnection {
        let disconnect = Arc::new(AtomicBool::new(false));
        let arc_config = Arc::new(tls_config);

        ServerConnection {
            connection_details,
            arc_config,
            _disconnect: disconnect,
            failures: 0,
            state: ServerConnectionStateMachine::Requested,
            sink_buffer: None,
        }
    }

    fn handle_err(&mut self, err: std::io::Error) -> ServerConnectionEvent {
        self.failures += 1;
        let duration = failure_count_to_timeout(self.failures);
        let instant = Instant::now() + duration;
        self.state = ServerConnectionStateMachine::Failed(Delay::new(instant));

        ServerConnectionEvent::ConnectionFailed(ConnectionFailed {
            err: err,
            failures: self.failures,
            duration: duration,
        })
    }
}

/// How long to wait after a given number of connection failures. Much like an exponential backoff,
/// but with hand-chosen round numbers as waits.
fn failure_count_to_timeout(failures: usize) -> Duration {
    match failures {
        0 => Duration::from_secs(5),
        1 => Duration::from_secs(5),
        2 => Duration::from_secs(5),
        3 => Duration::from_secs(10),
        4 => Duration::from_secs(20),
        5 => Duration::from_secs(30),
        6 => Duration::from_secs(60),
        7 => Duration::from_secs(60),
        _ => Duration::from_secs(120),
    }
}

/// Public events that get emitted when something happens with the connection.
pub enum ServerConnectionEvent {
    /// The connection is try to connect.
    Connecting,
    /// The TCP connection connected, and TLS establishment is next.
    TcpConnected,
    /// The TLS connection succeeded, so the connection is now active.
    TlsConnected,
    /// The connection failed for whatever reason.
    ConnectionFailed(ConnectionFailed),
    /// The connection is still connected, but we haven't heard from the other end in a while, so
    /// maybe we need to send a ping.
    TimeoutWarning,
    /// The other end sent us a message, and this is the message.
    Item(device::ServerMessage),
}

/// Information about the failed connection.
pub struct ConnectionFailed {
    /// Why the connection failed
    pub err: std::io::Error,
    /// The number of consecutive errors
    pub failures: usize,
    /// How long to wait before trying again
    pub duration: Duration,
}

type StreamType = TimedConnection<
    stream::SplitStream<
        tokio_codec::Framed<
            tokio_rustls::TlsStream<TcpStream, tokio_rustls::rustls::ClientSession>,
            Codec<device::ClientMessage, device::ServerMessage>,
        >,
    >,
>;
type SinkType = stream::SplitSink<
    tokio_codec::Framed<
        tokio_rustls::TlsStream<TcpStream, tokio_rustls::rustls::ClientSession>,
        Codec<device::ClientMessage, device::ServerMessage>,
    >,
>;

/// The state machine that we iterate through every time poll is called on the future
enum ServerConnectionStateMachine {
    /// The connection has been requested, but the state machine has not acted on it
    Requested, // -> TcpConnecting
    /// The connection is attempting to be established
    TcpConnecting(tokio_dns::IoFuture<TcpStream>), // -> TlsConnecting, Failed
    /// The TCP connection has succeeded, and TLS is negotiating
    TlsConnecting(tokio_rustls::Connect<TcpStream>), // -> Connected, Failed
    /// The connection is established, and will be checked after the given delay
    Connected(StreamType, SinkType), // -> Checking
    /// The connection has failed, and will be retried after the given delay
    Failed(Delay), // -> Connecting, Failed
}

impl Stream for ServerConnection {
    type Item = ServerConnectionEvent;
    type Error = std::io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        use self::ServerConnectionStateMachine::*;

        let state = std::mem::replace(&mut self.state, Requested);

        match state {
            Requested => {
                self.state = TcpConnecting(tokio_dns::TcpStream::connect(&self.connection_details));
                return Ok(Async::Ready(Some(ServerConnectionEvent::Connecting)));
            }
            TcpConnecting(mut future) => {
                match future.poll() {
                    Ok(Async::Ready(tcp_stream)) => {
                        // TCP connection successfully established. Try to negotiate TLS.
                        let domain = self.connection_details.address();
                        let domain = webpki::DNSNameRef::try_from_ascii_str(&domain).unwrap();
                        let connector: TlsConnector = self.arc_config.clone().into();
                        let future = connector.connect(domain, tcp_stream);

                        self.state = TlsConnecting(future);
                        return Ok(Async::Ready(Some(ServerConnectionEvent::TcpConnected)));
                    }
                    Ok(Async::NotReady) => {
                        // TCP is still connecting.
                        self.state = TcpConnecting(future);
                        return Ok(Async::NotReady);
                    }
                    Err(err) => {
                        // TCP connection failed. Ahh!
                        let event = self.handle_err(err);
                        return Ok(Async::Ready(Some(event)));
                    }
                }
            }
            TlsConnecting(mut future) => {
                match future.poll() {
                    Ok(Async::Ready(tls_stream)) => {
                        // TLS connection successfully established. Woop!
                        let codec: Codec<device::ClientMessage, device::ServerMessage> =
                            Codec::new();
                        let framed = tokio_codec::Decoder::framed(codec, tls_stream);
                        let (mut sink, stream) = framed.split();
                        // TimedConnection is something that wraps a regular stream, and notifies
                        // when we haven't heard from the other side for 60s or 120s.
                        let stream = TimedConnection::new(
                            stream,
                            TimedConnectionOptions {
                                warning_level: Duration::from_millis(60_000),
                                disconnect_level: Duration::from_millis(120_000),
                            },
                        );

                        // If we had something in the buffer that needs to be sent, try to send it.
                        let sink_buffer = std::mem::replace(&mut self.sink_buffer, None);
                        if let Some(item) = sink_buffer {
                            match sink.start_send(item) {
                                Ok(AsyncSink::Ready) => {
                                    // The sink took our buffered item! Yay.
                                    self.sink_buffer = None;
                                }
                                Ok(AsyncSink::NotReady(item)) => {
                                    // The sink didn't take our buffered item.
                                    self.sink_buffer = Some(item);
                                }
                                Err(err) => {
                                    // Is this the right way to handle this?
                                    let event = self.handle_err(err);
                                    return Ok(Async::Ready(Some(event)));
                                }
                            }
                        }

                        // Reset the number of failures we've seen, since we successfully
                        // connected.
                        self.failures = 0;
                        self.state = Connected(stream, sink);
                        return Ok(Async::Ready(Some(ServerConnectionEvent::TlsConnected)));
                    }
                    Ok(Async::NotReady) => {
                        // TLS is still trying to connect
                        self.state = TlsConnecting(future);
                        return Ok(Async::NotReady);
                    }
                    Err(err) => {
                        // TLS connection failed. Ahh!
                        let event = self.handle_err(err);
                        return Ok(Async::Ready(Some(event)));
                    }
                }
            }
            Connected(mut stream, sink) => {
                match stream.poll() {
                    Ok(Async::Ready(None)) => {
                        // Our connection ran out. Must be done.
                        let event = self.handle_err(std::io::ErrorKind::ConnectionReset.into());
                        return Ok(Async::Ready(Some(event)));
                    }
                    Ok(Async::Ready(Some(item))) => {
                        // The stream gave us something. It's either a message from the other end
                        // or a warning that we haven't heard from the server for a while. We'll
                        // pass that information up the chain.
                        self.state = Connected(stream, sink);
                        let event = match item {
                            TimedConnectionItem::Item(message) => {
                                ServerConnectionEvent::Item(message)
                            }
                            TimedConnectionItem::Timeout => ServerConnectionEvent::TimeoutWarning,
                        };
                        return Ok(Async::Ready(Some(event)));
                    }
                    Ok(Async::NotReady) => {
                        // Nothing happened.
                        self.state = Connected(stream, sink);
                        return Ok(Async::NotReady);
                    }
                    Err(err) => {
                        // Our connection failed. Ahh!
                        let event = self.handle_err(err);
                        return Ok(Async::Ready(Some(event)));
                    }
                }
            }
            Failed(mut delay) => {
                // If we're in the failed state, delay is the future that represents a wait before
                // we try again.
                match delay.poll() {
                    Ok(Async::Ready(_)) => {
                        // Timer expired. We can try to connect again!
                        self.state =
                            TcpConnecting(tokio_dns::TcpStream::connect(&self.connection_details));
                        return Ok(Async::Ready(Some(ServerConnectionEvent::Connecting)));
                    }
                    Ok(Async::NotReady) => {
                        // Timer is still running. We'll keep waiting.
                        self.state = Failed(delay);
                        return Ok(Async::NotReady);
                    }
                    Err(err) => {
                        // I'm not even sure how this failed.
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("{}", err),
                        ));
                    }
                }
            }
        }
    }
}

// Sink for ServerConnection handles when we try to send a message from the connection to the
// server.
impl Sink for ServerConnection {
    type SinkItem = device::ClientMessage;
    type SinkError = std::io::Error;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        use self::ServerConnectionStateMachine::*;

        if self.sink_buffer.is_some() {
            // We already have a message that is buffered to send. And since our buffer is only one
            // slot large, we can't handle another one. Sorry, caller.
            Ok(AsyncSink::NotReady(item))
        } else {
            // We don't have anything in the buffer, so let's see what's going to happen.
            let state = std::mem::replace(&mut self.state, Requested);

            match state {
                Connected(stream, mut sink) => {
                    // Oh good! We're actively connected. Let's send this item down the connection
                    // to the server.
                    let r = sink.start_send(item);
                    self.state = Connected(stream, sink);
                    r
                }
                state => {
                    // We're not connected right now. Let's put this item in the buffer and wait
                    // until we are connected. We can send the item then.
                    self.state = state;
                    self.sink_buffer = Some(item);
                    Ok(AsyncSink::Ready)
                }
            }
        }
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        use self::ServerConnectionStateMachine::*;

        let state = std::mem::replace(&mut self.state, Requested);

        match state {
            Connected(stream, mut sink) => {
                // Oh good! We're actively connected. That means we're basically proxying this
                // request to the underlying TLS connection's sink.
                let r = sink.poll_complete();
                self.state = Connected(stream, sink);
                r
            }
            state => {
                // We're not connected, so we can't make any progress on sending this item.
                self.state = state;
                Ok(Async::NotReady)
            }
        }
    }
}
