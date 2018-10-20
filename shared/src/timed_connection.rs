//! Timed connection
//!
//! We need a way to drop connections that have timed out. This structure takes a stream and wraps
//! it up with an interval, so that we can determine if it has been too long since we last received
//! a message.
//!
//! This is _fairly_ general, and could probably become more general. Except that it works for this
//! project, so I'm stopping here for now.

use std;
use futures::{Async, Poll, Stream};
use std::time::{Instant, Duration};
use tokio_timer::Interval;

#[derive(Debug)]
pub struct TimedConnection<T> {
    stream: Option<T>,
    check_interval: Option<Interval>,
    warning_level: Duration,
    disconnect_level: Duration,
    last_message: Instant,
    warning_sent: bool,
}

impl<T> TimedConnection<T> {
    pub fn new(stream: T, options: TimedConnectionOptions) -> TimedConnection<T> {
        TimedConnection {
            stream: Some(stream),
            check_interval: Some(Interval::new(Instant::now(), Duration::from_millis(1000))),
            warning_level: options.warning_level,
            disconnect_level: options.disconnect_level,
            last_message: Instant::now(),
            warning_sent: false,
        }
    }
}

pub struct TimedConnectionOptions {
    pub warning_level: Duration,
    pub disconnect_level: Duration,
}

impl Default for TimedConnectionOptions {
    fn default() -> TimedConnectionOptions {
        TimedConnectionOptions {
            warning_level: Duration::from_millis(10_000),
            disconnect_level: Duration::from_millis(30_000),
        }
    }
}

#[derive(Debug)]
pub enum TimedConnectionItem<T> {
    Item(T),
    Timeout,
}

impl<T: Stream> Stream for TimedConnection<T> {
    type Item = TimedConnectionItem<T::Item>;
    type Error = T::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // We already closed this. We're done.
        if self.stream.is_none() {
            self.check_interval = None;
            return Ok(Async::Ready(None));
        }

        if self.check_interval.is_none() {
            panic!("Unexpected check_interval is none");
        }

        // Get the stream, temporarily setting self's to None. If we want to keep stream, we'll set
        // it back.
        let mut stream = std::mem::replace(&mut self.stream, None).unwrap();

        match stream.poll() {
            Ok(Async::Ready(Some(item))) => {
                // There's an item available. Return it!
                self.last_message = Instant::now();
                self.warning_sent = false;
                self.stream = Some(stream);
                return Ok(Async::Ready(Some(TimedConnectionItem::Item(item))));
            },
            Ok(Async::Ready(None)) => {
                // The stream ended. We're done.
                self.stream = None;
                self.check_interval = None;

                return Ok(Async::Ready(None));
            },
            Ok(Async::NotReady) => {
                // Nothing was available. Fall through.
                self.stream = Some(stream);
            },
            Err(e) => {
                // There was an error. Return it.
                self.stream = None;
                self.check_interval = None;
                return Err(e);
            },
        };

        loop {
            // Get the stream, temporarily setting self's to None. If we want to keep stream, we'll
            // set it back.
            let mut check_interval = std::mem::replace(&mut self.check_interval, None).unwrap();

            match check_interval.poll() {
                Ok(Async::Ready(Some(_))) => {
                    // Time to check if something happened.
                    let now = Instant::now();

                    if self.last_message + self.disconnect_level < now {
                        // Drop the stream!
                        self.stream = None;
                        self.check_interval = None;
                        // And, this stream is done.
                        return Ok(Async::Ready(None));
                    }
                    if self.last_message + self.warning_level < now && self.warning_sent == false {
                        self.warning_sent = true;
                        self.check_interval = Some(check_interval);
                        return Ok(Async::Ready(Some(TimedConnectionItem::Timeout)));
                    }

                    // Nothing to do, so fall through.
                    self.check_interval = Some(check_interval);
                },
                Ok(Async::Ready(None)) => {
                    panic!("Interval stream unexpectedly ended!");
                },
                Ok(Async::NotReady) => {
                    // Nothing was available. Fall through.

                    self.check_interval = Some(check_interval);
                    break;
                },
                Err(_) => {
                    panic!("Didn't expect interval to return an error");
                },
            };
        }

        // Both of our futures returned NotReady, so we may return NotReady.
        Ok(Async::NotReady)
    }
}
