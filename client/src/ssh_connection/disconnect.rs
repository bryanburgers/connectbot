use super::CommandFuture;

use ::std::process::{Command, Stdio};
use ::futures::Future;
use ::futures::Async;
use ::futures::future::poll_fn;
use ::tokio_threadpool::blocking;

/// A future that disconnects a ssh socket
pub struct Disconnect {
    future: CommandFuture<()>,
}

impl Disconnect {
    pub fn new(id: String) -> Disconnect {
        let f = poll_fn(move || {
            blocking(|| {
                Command::new("ssh").args(&[
                                         "-O",
                                         "exit",
                                         "-S",
                                         &format!("/tmp/connectbot-ssh-{}", id),
                                         "_",
                ])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .unwrap();

                ()
            }).map_err(|_| panic!("the threadpool shut down"))
        });

        Disconnect {
            future: Box::new(f),
        }
    }
}

impl Future for Disconnect {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Result<Async<Self::Item>, Self::Error> {
        self.future.poll()
    }
}
