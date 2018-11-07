use connectbot_shared::client::Client;
use connectbot_shared::protos::control;
use http;
// use connectbot_shared::state::{self, Pattern};
use std::sync::Arc;
use tokio::prelude::*;
use tower_web::{Deserialize, Serialize, Extract, Response, impl_web};
// These seem to be sub-macros that tower_web uses. Importing macros from crates is new as of Rust
// 1.30.0, so I'm guessing tower_web will clean this up and these won't be needed in the future.
use tower_web::{
    derive_resource,
    derive_resource_impl,
    impl_web_clean_nested,
    impl_web_clean_top_level,
};

mod device;
use self::device::Device;

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

#[derive(Extract, Debug)]
struct CreateConnection {
    host: String,
    host_value: String,
    port: u16,
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

        #[get("/devices.json")]
        #[content_type("json")]
        fn devices_json(&self) -> impl Future<Item=DevicesResponse, Error=std::io::Error> + Send {
            self.client.get_clients().and_then(|devices| {
                let devices = devices.into();
                Ok(devices)
            })
        }

        #[get("/d/:device_id/json")]
        #[content_type("json")]
        fn device_json(&self, device_id: String) -> impl Future<Item=DeviceResponse, Error=std::io::Error> + Send {
            // For now, this is the same exact response as we hand to the template.
            self.device(device_id)
        }

        #[post("/d/:device_id/connections")]
        fn post_connections(&self, device_id: String, body: CreateConnection) -> impl Future<Item=http::Response<&'static str>, Error=std::io::Error> + Send {
            let host = match body.host.as_ref() {
                "remote" => body.host_value,
                "localhost" | _ => "localhost".to_string(),
            };
            self.client.connect_device(&device_id, &host, body.port).and_then(move |_| {
                let response = http::Response::builder()
                    .header("location", format!("/d/{}", device_id))
                    .status(http::StatusCode::SEE_OTHER)
                    .body("")
                    .unwrap();

                Ok(response)
            })
        }

        #[post("/d/:device_id/connections/:connection_id/delete")]
        fn delete_connection(&self, device_id: String, connection_id: String) -> impl Future<Item=http::Response<&'static str>, Error=std::io::Error> + Send {
            self.client.disconnect_connection(&device_id, &connection_id).and_then(move |_| {
                let response = http::Response::builder()
                    .header("location", format!("/d/{}", device_id))
                    .status(http::StatusCode::SEE_OTHER)
                    .body("")
                    .unwrap();

                Ok(response)
            })
        }

        #[post("/d/:device_id/connections/:connection_id/extend")]
        fn extend_connection(&self, device_id: String, connection_id: String) -> impl Future<Item=http::Response<&'static str>, Error=std::io::Error> + Send {
            self.client.extend_connection(&device_id, &connection_id).and_then(move |_| {
                let response = http::Response::builder()
                    .header("location", format!("/d/{}", device_id))
                    .status(http::StatusCode::SEE_OTHER)
                    .body("")
                    .unwrap();

                Ok(response)
            })
        }
    }
}
