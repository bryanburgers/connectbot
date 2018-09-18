use super::CommandFuture;

use ::std::process::{Command, Stdio};
use ::futures::Future;
use ::futures::Async;
use ::futures::future::poll_fn;
use ::tokio_threadpool::blocking;

/// A future that attempts to establish an ssh connection
///
/// Returns true if the connection already exists or if a new connection succeeds, and false if the
/// connection cannot be established.
pub struct Connect {
    data: ConnectData
}

impl Connect {
    pub fn new() -> Connect {
        Connect {
            data: ConnectData::None,
        }
    }
}

/// Internal connection data for the Connect future.
enum ConnectData {
    None,
    CheckFuture(CommandFuture<bool>),
    ConnectFuture(CommandFuture<bool>),
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
            let val = ::std::mem::replace(&mut self.data, ConnectData::None);
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

