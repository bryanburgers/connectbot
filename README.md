`connectbot` is a personal project built to connect a handful of my Raspberry
Pi projects to a central server, which allows me to establish reverse SSH
connections to the devices from wherever I am.

The clients attempt to maintain a constant connection to the server,
re-establishing the connection any time it gets severed. They are then able to
receive commands from the server and act on them.

The server and website allow me to click a few buttons to tell a client to
establish an SSH connection, with port forwards, to the server, allowing me to
access the forwarded ports.


## How it works

There are three projects that work together, and a fourth used for debugging.

`connectbot-client` goes on a remote device (Raspberry Pi) and runs as a daemon
to keep a constant TLS-encrypted TCP connection to the server. The client
responds to commands that are sent by the server; especially, the command to
initiate an SSH connection.

`connectbot-server` runs on the server and listens for connections from the
clients.

`connectbot-web` is the front-end web interface for the system, and
communicates with the server for information and to tell it to send commands to
the clients. Realistically, `connectbot-server` and `connectbot-web` could
easily be the same process.

`connecbot-ctrl` is a command-line tool that serves largely the same purpose as
`connectbot-web` but used as a debug tool.

The `shared` folder contains code tat can be shared among projects. This mostly
includes the protocol buffer files, and also some key `connectbot-web` code
(because it's shared with `connectbot-ctrl`).


## Gettings started

First, we need to create a few TLS certificates to protect communication
between the clients and the server. See CERTS.md to create a CA and server
certificate.

Second, we need to set up some configuration files. Run the following commands
to get default configuration files.

```
cargo run --bin connectbot-server -- config >server.config
cargo run --bin connectbot-web -- config >web.config
```

The server.config file needs its SSH information set up and must point to a
valid SSH key. Edit the server.config appropriately.

Now the server should be ready to go. In a new terminal, run

```
cargo run --bin connectbot-server -- --config server.config
```

The web service configuration should not need any changes. In a new terminal,
run

```
cargo run --bin connectbot-web -- --config web.config
```

At this point, `http://localhost:8080` should be serving a very basic webpage.

Finally, we want to launch a client. Because we're using self-signed
certificates, there are a few special parameters we need to pass here beyond
just `--host` and `--id`.

* `--host connectbot-server` to connect to the host at `connectbot-server` AND
  check that it's certificate is valid for that DNS name. (Yes, I realize
  `connectbot-server` will not resolve, we'll deal with that in a bit.

* `--id my-test-client` is the name of the device. Any name should work.

* `--resolve 127.0.0.1`, much like [curl's --resolve][curlresolve], tells the
  client to connect to connect to localhost instead of trying to resolve
  `connectbot-server` to an actual IP address.

* `--cafile ca.crt` is used to trust any certificate signed by the given CA.

```
cargo run --bin connectbot-client -- --host connectbot-server --resolve 127.0.0.1 --id my-test-client --cafile ca.crt
```

[curlresolve]: https://curl.haxx.se/docs/manpage.html#--resolve
