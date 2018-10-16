extern crate clap;
extern crate connectbot_shared;
#[macro_use]
extern crate serde_derive;
extern crate tokio;
extern crate futures;
#[macro_use]
extern crate tower_web;
extern crate chrono;

use chrono::{TimeZone, Utc};
use clap::{Arg, App};
use connectbot_shared::client::Client;
use connectbot_shared::protos::control;
// use connectbot_shared::state::{self, Pattern};
use std::sync::Arc;
use tower_web::ServiceBuilder;
use tower_web::view::Handlebars;
use tokio::prelude::*;

/// This type will be part of the web service as a resource.
#[derive(Clone, Debug)]
struct ConnectBotWeb {
    client: Arc<Client>,
}

impl ConnectBotWeb {
    fn new(addr: &str) -> ConnectBotWeb {
        ConnectBotWeb {
            client: Arc::new(Client::new(addr)),
        }
    }
}

#[derive(Debug, Response)]
struct IndexResponse {
    test: &'static str,
}

#[derive(Response, Debug)]
struct DevicesResponse {
    devices: Vec<Device>,
}

impl From<control::ClientsResponse> for DevicesResponse {
    fn from(mut response: control::ClientsResponse) -> Self {
        let devices: Vec<control::ClientsResponse_Client> = response.take_clients().into();
        let devices: Vec<Device> = devices.into_iter().map(|client| client.into()).collect();
        DevicesResponse {
            devices,
        }
    }
}

#[derive(Serialize, Debug)]
struct Device {
    id: String,
    name: String,
    address: Option<String>,
    connection_history: Vec<DeviceHistoryItem>,
}

impl From<control::ClientsResponse_Client> for Device {
    fn from(mut client: control::ClientsResponse_Client) -> Self {
        let id = client.take_id().to_string();
        let connection_history = client.take_connection_history()
            .into_iter()
            .map(Into::into)
            .collect();

        let address = client.take_address().to_string();
        let address = if address == "" {
            None
        }
        else {
            Some(address)
        };

        Device {
            name: id.clone(),
            id: id,
            address,
            connection_history,
        }
    }
}

#[derive(Serialize, Debug)]
struct DeviceHistoryItem {
    connected_at: String,
    last_message: Option<String>,
    address: String,
}

impl From<control::ClientsResponse_ConnectionHistoryItem> for DeviceHistoryItem {
    fn from(client: control::ClientsResponse_ConnectionHistoryItem) -> Self {
        let history_type = client.get_field_type();
        let connected_at = Utc.timestamp(client.get_connected_at() as i64, 0);
        let last_message = Utc.timestamp(client.get_last_message() as i64, 0);
        let address = client.get_address().to_string();

        match history_type {
            control::ClientsResponse_ConnectionHistoryType::UNKNOWN_CONNECTION_HISTORY_TYPE => {
                panic!("Invalid response!");
            },
            control::ClientsResponse_ConnectionHistoryType::CLOSED => {
                DeviceHistoryItem {
                    connected_at: connected_at.to_rfc3339(),
                    last_message: Some(last_message.to_rfc3339()),
                    address,
                }
            },
            control::ClientsResponse_ConnectionHistoryType::OPEN => {
                DeviceHistoryItem {
                    connected_at: connected_at.to_rfc3339(),
                    last_message: None,
                    address,
                }
            },
        }
    }
}

impl_web! {
    impl ConnectBotWeb {
        #[get("/")]
        #[content_type("html")]
        // #[web(template = "index")]
        fn index(&self) -> Result<&'static str, ()> {
            Ok(include_str!("../templates/index.hbs"))
            /*
            let resp = IndexResponse {
                test: "test",
            };
            Ok(resp)
            */
        }

        #[get("/devices")]
        #[content_type("json")]
        fn devices(&self) -> impl Future<Item=DevicesResponse, Error=std::io::Error> + Send {
            self.client.get_clients().and_then(|devices| {
                let devices = devices.into();
                Ok(devices)
            })
        }
    }
}

pub fn main() {
    let matches = App::new("connectbot-web")
        .version("1.0")
        .author("Bryan Burgers <bryan@burgers.io>")
        .arg(Arg::with_name("address")
             .short("a")
             .long("address")
             .help("The address to use to communicate with the connectbot server")
             .takes_value(true)
             .default_value("[::1]:12345"))
        .arg(Arg::with_name("listen")
             .short("l")
             .long("listen")
             .help("The address for the website to listen on")
             .takes_value(true)
             .default_value("[::]:8080"))
        .get_matches();

    let listen = matches.value_of("listen").unwrap().parse().expect("Invalid address");
    println!("Listening on http://{}", listen);
    let daemon_address = matches.value_of("address").unwrap();

    ServiceBuilder::new()
        .resource(ConnectBotWeb::new(daemon_address))
        .serializer(Handlebars::new())
        .run(&listen)
        .unwrap();
}
