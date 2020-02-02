//! Code that can be shared among projects. This mostly includes the protocol buffer files, and
//! also some key `connectbot-web` code (because it's shared with `connectbot-ctrl`).

extern crate bytes;
extern crate futures;
extern crate protobuf;
extern crate tokio;
extern crate tokio_codec;
extern crate tokio_dns;
extern crate tokio_io;
extern crate tokio_timer;

pub mod client;
pub mod codec;
/// Protocol buffer definitions.
pub mod protos;
pub mod timed_connection;
