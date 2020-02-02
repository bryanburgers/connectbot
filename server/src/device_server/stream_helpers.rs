use futures::{
    stream::{Fuse, Stream},
    sync::oneshot::{channel, Receiver, Sender},
    Async, Future, Poll,
};

/// I haven't found a good way with combinators to shut down the secondary stream once the primary
/// stream closes. So this is a manually implemented stream that will report as complete whenever
/// the primary reports as complete. This will let the stream end once the client disconnects.
pub struct PrimarySecondaryStream<S1, S2> {
    /// The important stream. Once this one ends, the combined stream ends.
    primary: Fuse<S1>,
    /// The less important stream. Even if this is still open, the combined stream will end.
    secondary: Fuse<S2>,
}

impl<S1, S2> PrimarySecondaryStream<S1, S2>
where
    S1: Stream,
    S2: Stream<Item = S1::Item, Error = S1::Error>,
{
    /// Create a new combined stream.
    pub fn new(primary: S1, secondary: S2) -> PrimarySecondaryStream<S1, S2> {
        PrimarySecondaryStream {
            primary: primary.fuse(),
            secondary: secondary.fuse(),
        }
    }
}

impl<S1, S2> Stream for PrimarySecondaryStream<S1, S2>
where
    S1: Stream,
    S2: Stream<Item = S1::Item, Error = S1::Error>,
{
    type Item = S1::Item;
    type Error = S1::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            match self.primary.poll() {
                Ok(Async::Ready(None)) => {
                    // The primary stream has ended. Return the combined stream as ended.
                    return Ok(Async::Ready(None));
                }
                Ok(Async::Ready(some)) => {
                    // We have data. Go ahead and return it.
                    return Ok(Async::Ready(some));
                }
                Ok(Async::NotReady) => {
                    // The primary stream is not ready. Maybe the secondary stream is. Fall
                    // through.
                }
                Err(e) => {
                    // An error occurred. Return it.
                    return Err(e);
                }
            }

            match self.secondary.poll() {
                Ok(Async::Ready(None)) => {
                    // Primary just said it was not ready. Secondary says it's done. So we're
                    // basically in primary-only mode, so return what primary returned.
                    return Ok(Async::NotReady);
                }
                Ok(Async::Ready(some)) => {
                    // Primary just said it was not ready. Secondary is ready. So return
                    // secondary's data.
                    return Ok(Async::Ready(some));
                }
                Ok(Async::NotReady) => {
                    // Neither primary nor secondary are ready. So... not ready.
                    return Ok(Async::NotReady);
                }
                Err(e) => {
                    // An error occurred. Return it. Even though this isn't the primary, all errors
                    // are important.
                    return Err(e);
                }
            }
        }
    }
}

/// Again, this seems like it should be a thing. But I can't find a good way to do it with
/// combinators. So, make one myself. A stream that can be cancelled.
pub struct CancelableStream<S> {
    stream: S,
    canceled: bool,
    receiver: Receiver<()>,
}

/// The thing that cancels a cancelable stream.
pub struct CancelHandle {
    sender: Sender<()>,
}

impl<S> CancelableStream<S>
where
    S: Stream,
{
    /// Create a new cancelable stream.
    pub fn new(stream: S) -> (CancelableStream<S>, CancelHandle) {
        let (sender, receiver) = channel();

        let stream = CancelableStream {
            stream: stream,
            canceled: false,
            receiver: receiver,
        };

        let handle = CancelHandle { sender: sender };

        (stream, handle)
    }
}

impl CancelHandle {
    /// Cancel the stream.
    pub fn cancel(self) -> Result<(), ()> {
        self.sender.send(())
    }
}

impl<S> Stream for CancelableStream<S>
where
    S: Stream,
{
    type Item = S::Item;
    type Error = S::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // If we've already seen that we're canceled, then we're cancelled
        if self.canceled {
            return Ok(Async::Ready(None));
        }

        // Check to se
        match self.receiver.poll() {
            Ok(Async::Ready(_)) => {
                self.canceled = true;
                return Ok(Async::Ready(None));
            }
            _ => {}
        }

        // If we aren't canceled yet, return whatever the stream returns
        self.stream.poll()
    }
}
