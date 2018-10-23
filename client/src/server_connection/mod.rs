use futures::{Async, Future, Poll, Stream, Sink, StartSend, AsyncSink, stream};
use std;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio_codec;
use tokio_dns;
use tokio_timer::Delay;

use connectbot_shared::codec::Codec;
use connectbot_shared::protos::device;
use connectbot_shared::timed_connection::{TimedConnection, TimedConnectionOptions, TimedConnectionItem};

use tokio_rustls::{
    self,
    TlsConnector,
    rustls::ClientConfig,
    webpki
};

#[derive(Clone)]
pub enum ConnectionDetails {
    Address { address: String, port: u16 },
    AddressWithResolve { address: String, resolve: IpAddr, port: u16 },
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
            ConnectionDetails::Address { address, port } => Ok(tokio_dns::Endpoint::Host(&address, *port)),
            ConnectionDetails::AddressWithResolve { resolve, port, .. } => Ok(tokio_dns::Endpoint::SocketAddr(SocketAddr::new(resolve.clone(), *port))),
        }
    }
}

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
    pub fn new(connection_details: ConnectionDetails, tls_config: ClientConfig) -> ServerConnection {
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

    fn handle_err(&mut self, err: std::io::Error) -> ServerConnectionEvent 
    {
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

pub enum ServerConnectionEvent {
    Connecting,
    TcpConnected,
    TlsConnected,
    ConnectionFailed(ConnectionFailed),
    TimeoutWarning,
    Item(device::ServerMessage),
}

pub struct ConnectionFailed {
    pub err: std::io::Error,
    pub failures: usize,
    pub duration: Duration,
}

type StreamType = TimedConnection<stream::SplitStream<tokio_codec::Framed<tokio_rustls::TlsStream<TcpStream, tokio_rustls::rustls::ClientSession>, Codec<device::ClientMessage, device::ServerMessage>>>>;
type SinkType = stream::SplitSink<tokio_codec::Framed<tokio_rustls::TlsStream<TcpStream, tokio_rustls::rustls::ClientSession>, Codec<device::ClientMessage, device::ServerMessage>>>;

enum ServerConnectionStateMachine {
    /// The connection has been requested, but the state machine has not acted on it
    Requested, // -> TcpConnecting
    /// The connection is attempting to be established
    TcpConnecting(tokio_dns::IoFuture<TcpStream>), // -> TlsConnecting, Failed
    TlsConnecting(tokio_rustls::Connect<TcpStream>), // -> Connected, Failed
    /// The connection is established, and will be checked after the given delay
    Connected(StreamType, SinkType), // -> Checking
    /// The connection has failed, and will be retried after the given delay
    Failed(Delay), // -> Connecting, Failed
}

impl Stream for ServerConnection
{
    type Item = ServerConnectionEvent;
    type Error = std::io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        use self::ServerConnectionStateMachine::*;
        // let connect_future = tokio_dns::TcpStream::connect(&connection);

        let state = std::mem::replace(&mut self.state, Requested);
        // let disconnecting = self.disconnect.load(Ordering::Relaxed);

        match state {
            Requested => {
                self.state = TcpConnecting(tokio_dns::TcpStream::connect(&self.connection_details));
                return Ok(Async::Ready(Some(ServerConnectionEvent::Connecting)))
            },
            TcpConnecting(mut future) => {
                match future.poll() {
                    Ok(Async::Ready(tcp_stream)) => {
                        let domain = self.connection_details.address();
                        let domain = webpki::DNSNameRef::try_from_ascii_str(&domain).unwrap();
                        let connector: TlsConnector = self.arc_config.clone().into();
                        let future = connector.connect(domain, tcp_stream);

                        self.state = TlsConnecting(future);
                        return Ok(Async::Ready(Some(ServerConnectionEvent::TcpConnected)));
                    },
                    Ok(Async::NotReady) => {
                        self.state = TcpConnecting(future);
                        return Ok(Async::NotReady);
                    },
                    Err(err) => {
                        let event = self.handle_err(err);
                        return Ok(Async::Ready(Some(event)));
                    },
                }
            },
            TlsConnecting(mut future) => {
                match future.poll() {
                    Ok(Async::Ready(tls_stream)) => {
                        let codec: Codec<device::ClientMessage, device::ServerMessage> = Codec::new();
                        let framed = tokio_codec::Decoder::framed(codec, tls_stream);
                        let (mut sink, stream) = framed.split();
                        let stream = TimedConnection::new(stream, TimedConnectionOptions {
                            warning_level: Duration::from_millis(60_000),
                            disconnect_level: Duration::from_millis(120_000),
                        });

                        let sink_buffer = std::mem::replace(&mut self.sink_buffer, None);
                        if let Some(item) = sink_buffer {
                            match sink.start_send(item) {
                                Ok(AsyncSink::Ready) => {
                                    // The sink took our buffered item! Yay.
                                    self.sink_buffer = None;
                                },
                                Ok(AsyncSink::NotReady(item)) => {
                                    // The sink didn't take our buffered item.
                                    self.sink_buffer = Some(item);
                                },
                                Err(err) => {
                                    // Is this the right way to handle this?
                                    let event = self.handle_err(err);
                                    return Ok(Async::Ready(Some(event)));
                                }
                            }
                        }

                        self.failures = 0;
                        self.state = Connected(stream, sink);
                        return Ok(Async::Ready(Some(ServerConnectionEvent::TlsConnected)));
                    },
                    Ok(Async::NotReady) => {
                        self.state = TlsConnecting(future);
                        return Ok(Async::NotReady);
                    },
                    Err(err) => {
                        let event = self.handle_err(err);
                        return Ok(Async::Ready(Some(event)));
                    },
                }
            },
            Connected(mut stream, sink) => {
                match stream.poll() {
                    Ok(Async::Ready(None)) => {
                        // Our connection ran out. Must be done.
                        let event = self.handle_err(std::io::ErrorKind::ConnectionReset.into());
                        return Ok(Async::Ready(Some(event)));
                    },
                    Ok(Async::Ready(Some(item))) => {
                        self.state = Connected(stream, sink);
                        let event = match item {
                            TimedConnectionItem::Item(message) => ServerConnectionEvent::Item(message),
                            TimedConnectionItem::Timeout => ServerConnectionEvent::TimeoutWarning,
                        };
                        return Ok(Async::Ready(Some(event)));
                    },
                    Ok(Async::NotReady) => {
                        self.state = Connected(stream, sink);
                        return Ok(Async::NotReady);
                    },
                    Err(err) => {
                        let event = self.handle_err(err);
                        return Ok(Async::Ready(Some(event)));
                    }
                }
            },
            Failed(mut delay) => {
                match delay.poll() {
                    Ok(Async::Ready(_)) => {
                        self.state = TcpConnecting(tokio_dns::TcpStream::connect(&self.connection_details));
                        return Ok(Async::Ready(Some(ServerConnectionEvent::Connecting)))
                    },
                    Ok(Async::NotReady) => {
                        self.state = Failed(delay);
                        return Ok(Async::NotReady);
                    },
                    Err(err) => {
                        return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("{}", err)));
                    },
                }
            }
        }
    }
}

impl Sink for ServerConnection {
    type SinkItem = device::ClientMessage;
    type SinkError = std::io::Error;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        use self::ServerConnectionStateMachine::*;

        if self.sink_buffer.is_some() {
            Ok(AsyncSink::NotReady(item))
        }
        else {
            let state = std::mem::replace(&mut self.state, Requested);

            match state {
                Connected(stream, mut sink) => {
                    let r = sink.start_send(item);
                    self.state = Connected(stream, sink);
                    r
                },
                state => {
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
                let r = sink.poll_complete();
                self.state = Connected(stream, sink);
                r
            },
            state => {
                self.state = state;
                Ok(Async::NotReady)
            }
        }
    }
}
