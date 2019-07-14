use super::CommandFuture;
use super::SshConnectionSettings;

use std;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
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
    settings: SshConnectionSettings,
    data: ConnectData,
}

impl Connect {
    pub fn new(settings: SshConnectionSettings) -> Connect {
        Connect {
            settings: settings,
            data: ConnectData::None,
        }
    }
}

type WriteKeyFuture = Box<dyn Future<Item=(), Error=String> + Send>;

/// Internal connection data for the Connect future.
enum ConnectData {
    /// Initial state.
    None,
    /// Run the command to check whether a connection is already active.
    CheckFuture(CommandFuture<bool>),
    /// Write the SSH private key to a file so it can be used in the connection future.
    WriteKeyFuture(WriteKeyFuture),
    /// Run the command to establish the connection.
    ConnectFuture(CommandFuture<bool>),
}

impl ConnectData {
    /// Create a new half of a future that checks whether a connection is already active.
    fn new_check(id: String) -> ConnectData {
        let f = poll_fn(move || {
            blocking(|| {
                Command::new("ssh").args(&[
                                         "-O",
                                         "check",
                                         "-S",
                                         &format!("/tmp/connectbot-ssh-{}", id),
                                         "_@localhost",
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

    fn new_write_key(settings: SshConnectionSettings) -> ConnectData {
        let f = poll_fn(move || {
            blocking(|| {
                let mut open_options = std::fs::OpenOptions::new();
                open_options
                    .write(true)
                    .truncate(true)
                    .create(true);

                open_options.mode(0o600);

                let mut file = open_options
                    .open(settings.private_key_file())
                    .map_err(|err| println!("Failed to open file: {}", err))
                    .unwrap();
                file.write_all(settings.private_key.as_bytes())
                    .map_err(|err| println!("Failed to write file: {}", err))
                    .unwrap();
            }).map_err(|_| panic!("the threadpool shut down"))
        });

        ConnectData::WriteKeyFuture(Box::new(f))
    }

    /// Create a new half of a future that establishes a new connection.
    fn new_connect(settings: SshConnectionSettings) -> ConnectData {
        let f = poll_fn(move || {
            let connection = format!("{}{}:{}:{}",
                                     match settings.gateway_port {
                                         true => ":",
                                         false => "",
                                     },
                                     settings.remote_port,
                                     settings.forward_host,
                                     settings.forward_port);
            blocking(|| {
                Command::new("ssh").args(&[
                                         "-f",
                                         "-N",
                                         "-R", &connection,
                                         "-o", "BatchMode=yes",
                                         "-o", "StrictHostKeyChecking=no",
                                         "-o", "UserKnownHostsFile=/dev/null",
                                         "-i", &settings.private_key_file(),
                                         "-M",
                                         "-S", &format!("/tmp/connectbot-ssh-{}", settings.id),
                                         "-p", &format!("{}", settings.port),
                                         &format!("{}@{}", settings.username, settings.host),
                ])
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
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
                    self.data = ConnectData::new_check(self.settings.id.clone());
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
                            self.data = ConnectData::new_write_key(self.settings.clone());
                        },
                        Async::NotReady => {
                            self.data = ConnectData::CheckFuture(future);

                            return Ok(Async::NotReady);
                        }
                    }
                },
                ConnectData::WriteKeyFuture(mut future) => {
                    match future.poll().map_err(|_| ())? {
                        Async::Ready(_) => {
                            self.data = ConnectData::new_connect(self.settings.clone());
                        },
                        Async::NotReady => {
                            self.data = ConnectData::WriteKeyFuture(future);

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

impl Drop for Connect {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.settings.private_key_file());
    }
}

