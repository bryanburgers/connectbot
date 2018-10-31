use std;
use futures::prelude::*;

use super::protos as protos;
use super::codec as codec;

mod request_response_future;

use self::request_response_future::RequestResponseFuture;

#[derive(Debug)]
pub struct Client {
    addr: String,
}

// For now, every request will establish a new TCP connection with the server, and then 
impl Client {
    pub fn new(addr: &str) -> Client {
        Client {
            addr: addr.to_string(),
        }
    }

    pub fn get_clients(&self) -> GetStateFuture {
        let mut message = protos::control::ClientMessage::new();
        let clients_request = protos::control::ClientsRequest::new();
        message.set_message_id(1);
        message.set_clients_request(clients_request);

        GetStateFuture {
            inner: RequestResponseFuture::new(&self.addr, message),
        }
    }

    pub fn connect_device(&self, device_id: &str, port: u16) -> impl Future<Item=protos::control::SshConnectionResponse, Error=std::io::Error> {
        let mut message = protos::control::ClientMessage::new();
        let mut ssh_connection = protos::control::SshConnection::new();
        let mut enable = protos::control::SshConnection_Enable::new();
        enable.set_forward_host("localhost".into());
        enable.set_forward_port(port as u32);
        enable.set_gateway_port(true);
        ssh_connection.set_device_id(device_id.into());
        ssh_connection.set_enable(enable);
        message.set_message_id(1);
        message.set_ssh_connection(ssh_connection);

        RequestResponseFuture::new(&self.addr, message)
            .map(|mut response| response.take_ssh_connection_response())
    }

    pub fn disconnect_connection(&self, device_id: &str, connection_id: &str) -> impl Future<Item=protos::control::SshConnectionResponse, Error=std::io::Error> {
        let mut message = protos::control::ClientMessage::new();
        let mut ssh_connection = protos::control::SshConnection::new();
        let mut disable = protos::control::SshConnection_Disable::new();
        disable.set_connection_id(connection_id.into());
        ssh_connection.set_device_id(device_id.into());
        ssh_connection.set_disable(disable);
        message.set_message_id(1);
        message.set_ssh_connection(ssh_connection);

        RequestResponseFuture::new(&self.addr, message)
            .map(|mut response| response.take_ssh_connection_response())
    }

    pub fn extend_connection(&self, device_id: &str, connection_id: &str) -> impl Future<Item=protos::control::SshConnectionResponse, Error=std::io::Error> {
        let mut message = protos::control::ClientMessage::new();
        let mut ssh_connection = protos::control::SshConnection::new();
        let mut extend = protos::control::SshConnection_ExtendTimeout::new();
        extend.set_connection_id(connection_id.into());
        ssh_connection.set_device_id(device_id.into());
        ssh_connection.set_extend_timeout(extend);
        message.set_message_id(1);
        message.set_ssh_connection(ssh_connection);

        RequestResponseFuture::new(&self.addr, message)
            .map(|mut response| response.take_ssh_connection_response())
    }

    pub fn create_device(&self, device_id: &str) -> impl Future<Item=protos::control::CreateDeviceResponse, Error=std::io::Error> {
        let mut message = protos::control::ClientMessage::new();
        let mut create_device = protos::control::CreateDevice::new();
        create_device.set_device_id(device_id.into());
        message.set_message_id(1);
        message.set_create_device(create_device);

        RequestResponseFuture::new(&self.addr, message)
            .map(|mut response| response.take_create_device_response())
    }

    pub fn remove_device(&self, device_id: &str) -> impl Future<Item=protos::control::RemoveDeviceResponse, Error=std::io::Error> {
        let mut message = protos::control::ClientMessage::new();
        let mut remove_device = protos::control::RemoveDevice::new();
        remove_device.set_device_id(device_id.into());
        message.set_message_id(1);
        message.set_remove_device(remove_device);

        RequestResponseFuture::new(&self.addr, message)
            .map(|mut response| response.take_remove_device_response())
    }

    pub fn set_name(&self, device_id: &str, name: &str) -> impl Future<Item=protos::control::SetNameResponse, Error=std::io::Error> {
        let mut message = protos::control::ClientMessage::new();
        let mut set_name = protos::control::SetName::new();
        set_name.set_device_id(device_id.into());
        set_name.set_name(name.into());
        message.set_message_id(1);
        message.set_set_name(set_name);

        RequestResponseFuture::new(&self.addr, message)
            .map(|mut response| response.take_set_name_response())
    }
}

pub struct GetStateFuture {
    inner: RequestResponseFuture,
}

impl Future for GetStateFuture {
    type Item = protos::control::ClientsResponse;
    type Error = std::io::Error;

    fn poll(&mut self) -> Result<Async<Self::Item>, Self::Error> {
        match self.inner.poll()? {
            Async::NotReady => Ok(Async::NotReady),
            Async::Ready(mut message) => {
                let state = message.take_clients_response();
                // let state = state::State::from_proto(&state)?;
                Ok(Async::Ready(state))
            }
        }
    }
}

/*
impl Future for SetResultFuture {
    type Item = Result<(), String>;
    type Error = std::io::Error;

    fn poll(&mut self) -> Result<Async<Self::Item>, Self::Error> {
        match self.inner.poll()? {
            Async::NotReady => Ok(Async::NotReady),
            Async::Ready(message) => {
                let response = message.get_response();
                let result = match response.get_result() {
                    protos::ResponseResult::SUCCESS => Ok(()),
                    protos::ResponseResult::ERROR => Err(response.get_error_message().into()),
                    _ => Err("Response did not include a result".to_string()),
                };
                Ok(Async::Ready(result))
            }
        }
    }
}
*/
