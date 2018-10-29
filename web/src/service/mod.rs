use chrono::{TimeZone, Utc};
use connectbot_shared::client::Client;
use connectbot_shared::protos::control;
// use connectbot_shared::state::{self, Pattern};
use std::sync::Arc;
use tokio::prelude::*;
use tower_web::{Serialize, Response, impl_web};
// These seem to be sub-macros that tower_web uses. Importing macros from crates is new as of Rust
// 1.30.0, so I'm guessing tower_web will clean this up and these won't be needed in the future.
use tower_web::{
    derive_resource,
    derive_resource_impl,
    impl_web_clean_nested,
    impl_web_clean_top_level,
};

/// This type will be part of the web service as a resource.
#[derive(Clone, Debug)]
pub struct ConnectBotWeb {
    client: Arc<Client>,
}

impl ConnectBotWeb {
    pub fn new(addr: &str) -> ConnectBotWeb {
        ConnectBotWeb {
            client: Arc::new(Client::new(addr)),
        }
    }
}

#[derive(Debug, Response)]
struct IndexResponse {
    test: &'static str,
}

#[derive(Debug, Response)]
struct DeviceResponse {
    device: Device,
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
        #[web(template = "index")]
        fn index(&self) -> Result<IndexResponse, ()> {
            let resp = IndexResponse {
                test: "test",
            };
            Ok(resp)
        }

        #[get("/d/:device_id")]
        #[content_type("html")]
        #[web(template = "device")]
        fn device(&self, device_id: String) -> impl Future<Item=DeviceResponse, Error=std::io::Error> + Send {
            self.client.get_clients().and_then(move |devices| {
                let devices_response: DevicesResponse = devices.into();
                let devices = devices_response.devices;
                let maybe_device = devices.into_iter().filter(|d| d.id == device_id).nth(0);

                if let Some(device) = maybe_device {
                    Ok(DeviceResponse {
                        device: device,
                    })
                }
                else {
                    Err(std::io::Error::new(std::io::ErrorKind::Other, "Device not found".to_string()))
                }
            })
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
