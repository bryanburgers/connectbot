use super::CommandFuture;

use ::std::process::{Command, Stdio};
use ::futures::Future;
use ::futures::Async;
use ::futures::future::poll_fn;
use ::tokio_threadpool::blocking;

/// A future that checks whether an ssh socket is still active
///
/// Returns true if the socket is still active, and false if the socket is no longer active.
pub struct Check {
    future: CommandFuture<bool>,
}

impl Check {
    pub fn new(id: String) -> Check {
        let f = poll_fn(move || {
            blocking(|| {
                Command::new("ssh").args(&[
                                         "-O",
                                         "check",
                                         "-S",
                                         &format!("/tmp/rssh-session-{}", id),
                                         "_",
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

