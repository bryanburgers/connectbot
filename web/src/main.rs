extern crate chrono;
extern crate clap;
extern crate connectbot_shared;
extern crate futures;
extern crate serde_derive;
extern crate tokio;
extern crate toml;
#[macro_use]
extern crate tower_web;

use chrono::{TimeZone, Utc};
use clap::{Arg, App, AppSettings, SubCommand};
use connectbot_shared::client::Client;
use connectbot_shared::protos::control;
// use connectbot_shared::state::{self, Pattern};
use std::path::Path;
use std::sync::Arc;
use tower_web::ServiceBuilder;
use tower_web::view::Handlebars;
use tokio::prelude::*;

mod config;

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
        .setting(AppSettings::SubcommandsNegateReqs)
        .subcommand(SubCommand::with_name("config")
                    .about("Generate an example config file"))
        .arg(Arg::with_name("config")
             .short("c")
             .long("config")
             .help("The location of the config file")
             .takes_value(true)
             .default_value("/etc/connectbot/web.conf"))
        .get_matches();

    if let Some(_matches) = matches.subcommand_matches("config") {
        let config: config::ApplicationConfig = std::default::Default::default();
        print!("{}", toml::to_string(&config).unwrap());
        return;
    }

    let config_file = Path::new(matches.value_of_os("config").unwrap());
    let config_base = config_file.parent().unwrap();

    let result = config::ApplicationConfig::from_file(config_file);
    let config = match result {
        Ok(config) => config,
        Err(string) => {
            println!("{}", string);
            std::process::exit(1);
        }
    };

    let address = (&config.address).parse().expect("address must be a valid socket address");
    let control_address = &config.control_address;

    println!("Listening on http://{}", address);

    let assets_path = config_base.join(config.assets.path);
    let templates_path = config_base.join(config.templates.path);

    ServiceBuilder::new()
        .resource(ConnectBotWeb::new(control_address))
        .serializer(Handlebars::new())
        .run(&address)
        .unwrap();
}
