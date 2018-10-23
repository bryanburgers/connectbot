use ::std;
use ::futures::stream::Stream;
use ::futures::Future;
use ::futures::Poll;
use ::futures::Async;
use ::tokio_timer::Delay;
use ::std::time::Instant;
use ::std::time::Duration;
use ::std::sync::Arc;
use ::std::sync::atomic::{AtomicBool, Ordering};

mod check;
use self::check::Check;
mod connect;
use self::connect::Connect;
mod disconnect;
use self::disconnect::Disconnect;

/// A stream that handles a "persistent" SSH connection.
///
/// This stream will attempt to continually keep an SSH connection open. If the connection is not
/// open, it will try to re-establish the connection. If the connection is open, it will
/// periodically check to see if it remains open.
///
/// This stream will continue emitting values until it is specifically told to disconnect, at which
/// point it will attempt to disconnect the existing connection if necessary, and then end the
/// stream.
pub struct SshConnection {
    /// Whether a disconnect has been requested.
    disconnect: Arc<AtomicBool>,
    /// The number of consecutive failures, used for backoff
    failures: usize,
    /// The state machine
    state: SshConnectionStateMachine,
}

impl SshConnection {
    /// Create a new SSH connection object
    pub fn new() -> SshConnection {
        SshConnection {
            disconnect: Arc::new(AtomicBool::new(false)),
            failures: 0,
            state: SshConnectionStateMachine::Requested,
        }
    }

    /// Create a handle for the SSH connection that can be used to disconnect the connection in the
    /// future.
    ///
    /// As many handles can be requested as needed, and existing handles are clonable. Once *any*
    /// handle requests a disconnect, the SshConnection stream will begin disconnecting.
    pub fn handle(&self) -> SshConnectionHandle {
        SshConnectionHandle {
            disconnect: self.disconnect.clone(),
        }
    }
}

/// A handle that can be used to disconnect the SSH connection stream.
///
/// This is the only way to stop the connection. Without specifically requesting a disconnect, the
/// SshConnection will continue trying to reconnect forever.
#[derive(Clone)]
pub struct SshConnectionHandle {
    disconnect: Arc<AtomicBool>,
}

impl SshConnectionHandle {
    /// Disconnect the SshConnection.
    pub fn disconnect(&self) {
        self.disconnect.store(true, Ordering::Relaxed);
    }
}

/// Public-facing connection change events.
///
/// When a key event occurs in the connection lifecycle, one of these will be emitted from the
/// stream.
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum SshConnectionChange {
    /// The connection has started connecting
    Connecting,
    /// A connection has been established
    Connected,
    /// The connection has started disconnecting
    Disconnecting,
    /// The connection has disconnected (this is the end state)
    Disconnected,
    /// A connection failed (either failed to connect initially, or was connected and has dropped).
    /// The value is how many times *consecutively* the failure has occurred (i.e., this value is
    /// reset to 0 every time a successful connection has been made).
    Failed(usize),
}

/// Internal connection state.
///
/// The SshConnection essentially amounts to a state machine that transitions through the following
/// states.
enum SshConnectionStateMachine {
    /// The connection has been requested, but the state machine has not acted on it
    Requested, // -> Connecting
    /// The connection is attempting to be established
    Connecting(Connect), // -> Connected, Failed
    /// The connection is established, and will be checked after the given delay
    Connected(Delay), // -> Checking
    /// The connection is being checked to see if it still active
    Checking(Check), // -> Connected, Failed
    /// The connection is being disconnected. Note that we can get to this state from any other
    /// state when SshConnection's disconnect member is true.
    Disconnecting(Disconnect), // -> Disconnected
    /// The connection has been disconnected.
    Disconnected, // *end state*
    /// The connection has failed, and will be retried after the given delay
    Failed(Delay), // -> Connecting, Failed
}

impl Stream for SshConnection {
    type Item = SshConnectionChange;
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        use self::SshConnectionStateMachine::*;

        loop {
            let state = std::mem::replace(&mut self.state, Requested);
            let disconnecting = self.disconnect.load(Ordering::Relaxed);

            match (disconnecting, state) {
                (false, Requested) => {
                    self.state = Connecting(Connect::new());
                    return Ok(Async::Ready(Some(SshConnectionChange::Connecting)))
                },
                (false, Connecting(mut delay)) => {
                    match delay.poll().map_err(|_| ())? {
                        Async::Ready(result) => {
                            if result {
                                let instant = Instant::now() + Duration::from_millis(10_000);
                                self.failures = 0;
                                self.state = Connected(Delay::new(instant));
                                return Ok(Async::Ready(Some(SshConnectionChange::Connected)));
                            }
                            else {
                                self.failures += 1;
                                let instant = Instant::now() + Duration::from_millis(self.failures as u64 * 1_000);
                                self.state = Failed(Delay::new(instant));
                                return Ok(Async::Ready(Some(SshConnectionChange::Failed(self.failures))));
                            }
                        },
                        Async::NotReady => {
                            self.state = Connecting(delay);
                            return Ok(Async::NotReady);
                        }
                    }
                },
                (false, Connected(mut delay)) => {
                    match delay.poll().map_err(|_| ())? {
                        Async::Ready(_) => {
                            self.state = Checking(Check::new());
                        },
                        Async::NotReady => {
                            self.state = Connected(delay);
                            return Ok(Async::NotReady);
                        }
                    }
                },
                (false, Checking(mut future)) => {
                    match future.poll().map_err(|_| ())? {
                        Async::Ready(result) => {
                            if result {
                                self.failures = 0;
                                let instant = Instant::now() + Duration::from_millis(10_000);
                                self.state = Connected(Delay::new(instant));
                            }
                            else {
                                self.failures += 1;
                                let instant = Instant::now() + Duration::from_millis(self.failures as u64 * 1_000);
                                self.state = Failed(Delay::new(instant));
                                return Ok(Async::Ready(Some(SshConnectionChange::Failed(self.failures))));
                            }
                        },
                        Async::NotReady => {
                            self.state = Checking(future);
                            return Ok(Async::NotReady);
                        }
                    }
                },
                (false, Failed(mut delay)) => {
                    match delay.poll().map_err(|_| ())? {
                        Async::Ready(_) => {
                            self.state = Connecting(Connect::new());
                            return Ok(Async::Ready(Some(SshConnectionChange::Connecting)));
                        },
                        Async::NotReady => {
                            self.state = Failed(delay);
                            return Ok(Async::NotReady);
                        }
                    }
                }
                (_, Disconnecting(mut future)) => {
                    match future.poll().map_err(|_| ())? {
                        Async::Ready(_) => {
                            self.state = Disconnected;
                            return Ok(Async::Ready(Some(SshConnectionChange::Disconnected)));
                        },
                        Async::NotReady => {
                            self.state = Disconnecting(future);
                            return Ok(Async::NotReady);
                        }
                    }
                },
                (_, Disconnected) => {
                    return Ok(Async::Ready(None));
                }
                (true, _) => {
                    self.state = Disconnecting(Disconnect::new());
                    return Ok(Async::Ready(Some(SshConnectionChange::Disconnecting)));
                },
            }
        }
    }
}

type CommandFuture<T> = Box<dyn Future<Item=T, Error=()> + Send>;
