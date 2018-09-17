use ::std;
use ::std::process::Command;
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
    state: SshConnectionState,
}

impl SshConnection {
    pub fn new() -> impl Stream<Item=SshConnectionChange, Error=()> {
        SshConnection {
            disconnect: Arc::new(AtomicBool::new(false)),
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

/// Public-facing connection change events
#[derive(Debug, Eq, PartialEq)]
pub enum SshConnectionChange {
    Connecting,
    Connected,
    _Disconnecting,
    _Disconnected,
    Failed,
}

/// Internal connection state. These seem to duplicate the public-facing change events, but contain
/// more data and internal state.
enum SshConnectionState {
    Requested,
    Connecting(Connect),
    Connected(Delay),
    Checking(Check),
    _Disconnecting,
    _Disconnected,
    Failed(Delay),
}

impl Stream for SshConnection {
    type Item = SshConnectionChange;
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        use self::SshConnectionState::*;

        loop {
            let state = std::mem::replace(&mut self.state, Requested);

            match state {
                Requested => {
                    self.state = Connecting(Connect::new());
                    return Ok(Async::Ready(Some(SshConnectionChange::Connecting)))
                },
                Connecting(mut delay) => {
                    match delay.poll().map_err(|_| ())? {
                        Async::Ready(result) => {
                            if result {
                                let instant = Instant::now() + Duration::from_millis(10_000);
                                self.state = Connected(Delay::new(instant));
                                return Ok(Async::Ready(Some(SshConnectionChange::Connected)));
                            }
                            else {
                                let instant = Instant::now() + Duration::from_millis(1_000);
                                self.state = Failed(Delay::new(instant));
                                return Ok(Async::Ready(Some(SshConnectionChange::Failed)));
                            }
                        },
                        Async::NotReady => {
                            self.state = Connecting(delay);
                            return Ok(Async::NotReady);
                        }
                    }
                },
                Connected(mut delay) => {
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
                Checking(mut delay) => {
                    match delay.poll().map_err(|_| ())? {
                        Async::Ready(result) => {
                            if result {
                                let instant = Instant::now() + Duration::from_millis(10_000);
                                self.state = Connected(Delay::new(instant));
                                // TODO: We were already connected. Don't notify that again!
                                // return Ok(Async::Ready(Some(SshConnectionChange::Connected)));
                            }
                            else {
                                let instant = Instant::now() + Duration::from_millis(1_000);
                                self.state = Failed(Delay::new(instant));
                                return Ok(Async::Ready(Some(SshConnectionChange::Failed)));
                            }
                            // return Ok(Async::NotReady);
                        },
                        Async::NotReady => {
                            self.state = Checking(delay);
                            return Ok(Async::NotReady);
                        }
                    }
                },
                Failed(mut delay) => {
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
                _ => {
                    return Ok(Async::Ready(None));
                }
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
                // Command::new("/bin/bash").args(&["-c", "echo 'checking for existing connection!' ; [ $RANDOM -lt 16383 ]"]).status().unwrap().success()
                Command::new("ssh").args(&[
                                         "-O",
                                         "check",
                                         "-S",
                                         "/tmp/rssh-session-1",
                                         "bjb3@127.0.0.1",
                ]).status().unwrap().success()
            }).map_err(|_| panic!("the threadpool shut down"))
        });

        ConnectData::CheckFuture(Box::new(f))
    }

    fn new_connect() -> ConnectData {
        let f = poll_fn(move || {
            blocking(|| {
                // Command::new("/bin/bash").args(&["-c", "echo 'connect command running!' ; [ $RANDOM -lt 16383 ]"]).status().unwrap().success()
                Command::new("ssh").args(&[
                                         "-f",
                                         "-N",
                                         "-R",
                                         "0:localhost:22",
                                         "-M",
                                         "-S",
                                         "/tmp/rssh-session-1",
                                         "bjb3@137.0.0.1",
                ]).status().unwrap().success()
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
                // Command::new("/bin/bash").args(&["-c", "echo 'check command running!' ; [ $RANDOM -lt 16383 ]"]).status().unwrap().success()
                Command::new("ssh").args(&[
                                         "-O",
                                         "check",
                                         "-S",
                                         "/tmp/rssh-session-1",
                                         "bjb3@127.0.0.1",
                ]).status().unwrap().success()
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
