use std;
use tokio;
use tokio::net::TcpStream;
use tokio_codec;
use futures::{self, Stream, Sink, Future};

use comms_shared::codec::Codec;
use comms_shared::protos::control;

use super::world::{self, SharedWorld};

pub struct Server {
    world: SharedWorld,
}

impl Server {
    pub fn new(world: world::SharedWorld) -> Server {
        Server {
            world: world,
        }
    }

    pub fn handle_control_connection(&self, conn: TcpStream) -> impl Future<Item=(), Error=std::io::Error> {
        // Process socket here.
        let codec: Codec<control::ServerMessage, control::ClientMessage> = Codec::new();
        let framed = tokio_codec::Decoder::framed(codec, conn);
        let (sink, stream) = framed.split();

        let (tx, rx) = futures::sync::mpsc::channel(0);

        let sink = sink.sink_map_err(|_| ());
        let rx = rx.map_err(|_| panic!());

        tokio::spawn(rx.forward(sink).then(|result| {
            if let Err(e) = result {
                panic!("failed to write to socket: {:?}", e)
            }
            Ok(())
        }));

        let world = self.world.clone();

        stream.for_each(move |message| -> Box<dyn Future<Item=(), Error=std::io::Error> + Send> {
            println!("{:?}", message);
            if message.has_clients_request() {
                let mut clients = Vec::new();
                {
                    let world = world.read().unwrap();
                    for device in world.devices.values() {
                        // println!("{:?} {:?}", key, value);
                        let mut client_data = control::ClientsResponse_Client::new();
                        client_data.set_id(device.id.clone().into());
                        if let world::ConnectionStatus::Connected { ref address } = device.connection_status {
                            client_data.set_address(address.to_string().into());
                        }
                        clients.push(client_data);
                    }
                }

                let mut clients_response = control::ClientsResponse::new();
                clients_response.set_clients(clients.into());

                let mut response = control::ServerMessage::new();
                response.set_clients_response(clients_response);
                response.set_in_response_to(message.get_message_id());

                let f = tx.clone().send(response)
                    .map(|_| ())
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e)));

                return Box::new(f);
            }

            // message_handler::handle_message(message, tx.clone(), new_state.clone())
            Box::new(futures::future::ok(()))
        })
    }
}