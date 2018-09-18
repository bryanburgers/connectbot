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

pub struct SshConnection {
    disconnect: Arc<AtomicBool>,
    failures: usize,
    state: SshConnectionState,
}

impl SshConnection {
    pub fn new() -> SshConnection {
        SshConnection {
            disconnect: Arc::new(AtomicBool::new(false)),
            failures: 0,
            state: SshConnectionState::Requested,
        }
    }

    pub fn handle(&self) -> SshConnectionHandle {
        SshConnectionHandle {
            disconnect: self.disconnect.clone(),
        }
    }
}

#[derive(Clone)]
pub struct SshConnectionHandle {
    disconnect: Arc<AtomicBool>,
}

impl SshConnectionHandle {
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

enum ConnectData {
    None,
    CheckFuture(CommandFuture),
    ConnectFuture(CommandFuture),
}

impl ConnectData {
    fn new() -> ConnectData {
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
                    self.data = ConnectData::new();
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
