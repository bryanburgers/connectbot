use ::std;
use ::std::process::{Command, Stdio};
use ::futures::stream::Stream;
use ::futures::Future;
use ::futures::Poll;
use ::futures::Async;
use ::futures::future::poll_fn;
use ::tokio_threadpool::blocking;
use ::tokio_timer::Delay;
use ::std::time::Instant;
use ::std::time::Duration;
use ::std::sync::Arc;
use ::std::sync::atomic::{AtomicBool, Ordering};

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
    disconnect: Arc<AtomicBool>,
    failures: usize,
    state: SshConnectionState,
}

impl SshConnection {
    /// Create a new SSH connection object
    pub fn new() -> SshConnection {
        SshConnection {
            disconnect: Arc::new(AtomicBool::new(false)),
            failures: 0,
            state: SshConnectionState::Requested,
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

/// Public-facing connection change events
#[derive(Debug, Eq, PartialEq)]
pub enum SshConnectionChange {
    Connecting,
    Connected,
    Disconnecting,
    Disconnected,
    Failed(usize),
}

/// Internal connection state. These seem to duplicate the public-facing change events, but contain
/// more data and internal state.
enum SshConnectionState {
    Requested,
    Connecting(Connect),
    Connected(Delay),
    Checking(Check),
    Disconnecting(Disconnect),
    Disconnected,
    Failed(Delay),
}

impl Stream for SshConnection {
    type Item = SshConnectionChange;
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        use self::SshConnectionState::*;

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
                                // TODO: We were already connected. Don't notify that again!
                                // return Ok(Async::Ready(Some(SshConnectionChange::Connected)));
                            }
                            else {
                                self.failures += 1;
                                let instant = Instant::now() + Duration::from_millis(self.failures as u64 * 1_000);
                                self.state = Failed(Delay::new(instant));
                                return Ok(Async::Ready(Some(SshConnectionChange::Failed(self.failures))));
                            }
                            // return Ok(Async::NotReady);
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

type CommandFuture = Box<dyn Future<Item=bool, Error=()> + Send>;

/// A future that attempts to establish an ssh connection
///
/// Returns true if the connection already exists or if a new connection succeeds, and false if the
/// connection cannot be established.
struct Connect {
    data: ConnectData
}

impl Connect {
    fn new() -> Connect {
        Connect {
            data: ConnectData::None,
        }
    }
}

/// Internal connection data for the Connect future.
enum ConnectData {
    None,
    CheckFuture(CommandFuture),
    ConnectFuture(CommandFuture),
}

impl ConnectData {
    /// Create a new half of a future that checks whether a connection is already active.
    fn new_check() -> ConnectData {
        let f = poll_fn(move || {
            blocking(|| {
                Command::new("ssh").args(&[
                                         "-O",
                                         "check",
                                         "-S",
                                         "/tmp/rssh-session-1",
                                         "bjb3@127.0.0.1",
                ])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .unwrap()
                    .success()
            }).map_err(|_| panic!("the threadpool shut down"))
        });

        ConnectData::CheckFuture(Box::new(f))
    }

    /// Create a new half of a future that establishes a new connection.
    fn new_connect() -> ConnectData {
        let f = poll_fn(move || {
            blocking(|| {
                Command::new("ssh").args(&[
                                         "-f",
                                         "-N",
                                         "-R",
                                         "0:localhost:22",
                                         "-M",
                                         "-S",
                                         "/tmp/rssh-session-1",
                                         "bjb3@127.0.0.1",
                ])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .unwrap()
                    .success()
            }).map_err(|_| panic!("the threadpool shut down"))
        });

        ConnectData::ConnectFuture(Box::new(f))
    }
}

impl Future for Connect {
    type Item = bool;
    type Error = ();

    fn poll(&mut self) -> Result<Async<Self::Item>, Self::Error> {
        loop {
            let val = std::mem::replace(&mut self.data, ConnectData::None);
            match val {
                ConnectData::None => {
                    self.data = ConnectData::new_check();
                },
                ConnectData::CheckFuture(mut future) => {
                    match future.poll()? {
                        Async::Ready(result) => {
                            if result {
                                // If the check returned true, we're already connected. So we can,
                                // in good conscience, say that we're connected.
                                return Ok(Async::Ready(true))
                            }

                            // If the check returned false, we're not already connected. Well,
                            // might as well get on connecting, then.
                            self.data = ConnectData::new_connect();
                        },
                        Async::NotReady => {
                            self.data = ConnectData::CheckFuture(future);

                            return Ok(Async::NotReady);
                        }
                    }
                },
                ConnectData::ConnectFuture(mut future) => {
                    let result = future.poll();
                    self.data = ConnectData::ConnectFuture(future);

                    return result;
                },
            }
        }
    }
}

/// A future that checks whether an ssh socket is still active
///
/// Returns true if the socket is still active, and false if the socket is no longer active.
struct Check {
    future: CommandFuture,
}

impl Check {
    fn new() -> Check {
        let f = poll_fn(move || {
            blocking(|| {
                Command::new("ssh").args(&[
                                         "-O",
                                         "check",
                                         "-S",
                                         "/tmp/rssh-session-1",
                                         "bjb3@127.0.0.1",
                ])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .unwrap()
                    .success()
            }).map_err(|_| panic!("the threadpool shut down"))
        });

        Check {
            future: Box::new(f),
        }
    }
}

impl Future for Check {
    type Item = bool;
    type Error = ();

    fn poll(&mut self) -> Result<Async<Self::Item>, Self::Error> {
        self.future.poll()
    }
}

/// A future that disconnects a ssh socket
struct Disconnect {
    future: CommandFuture,
}

impl Disconnect {
    fn new() -> Disconnect {
        let f = poll_fn(move || {
            blocking(|| {
                Command::new("ssh").args(&[
                                         "-O",
                                         "exit",
                                         "-S",
                                         "/tmp/rssh-session-1",
                                         "bjb3@127.0.0.1",
                ])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .unwrap()
                    .success()
            }).map_err(|_| panic!("the threadpool shut down"))
        });

        Disconnect {
            future: Box::new(f),
        }
    }
}

impl Future for Disconnect {
    type Item = bool;
    type Error = ();

    fn poll(&mut self) -> Result<Async<Self::Item>, Self::Error> {
        self.future.poll()
    }
}
