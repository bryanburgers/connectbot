//! A client that can talk to the server to get information from the server or to tell the server
//! to send messages to the client.

use futures::prelude::*;
use std;

use super::codec;
use super::protos;

mod request_response_future;

use self::request_response_future::RequestResponseFuture;

/// A client that can talk to the server to get information from the server or to tell the server
/// to send messages to the client.
#[derive(Debug)]
pub struct Client {
    addr: String,
}

// For now, every request will establish a new TCP connection with the server, send a message, and
// wait for the response. You can imagine a future where we keep a connection open for a while if
// there is more communcation that happens with the server, but we're not doing that right now.
impl Client {
    /// Create a new client connecting to the given address.
    pub fn new(addr: &str) -> Client {
        Client {
            addr: addr.to_string(),
        }
    }

    /// Get a list of clients that the server knows about.
    pub fn get_clients(&self) -> GetStateFuture {
        let mut message = protos::control::ClientMessage::new();
        let clients_request = protos::control::ClientsRequest::new();
        message.set_message_id(1);
        message.set_clients_request(clients_request);

        GetStateFuture {
            inner: RequestResponseFuture::new(&self.addr, message),
        }
    }

    /// Tell the server to establish an SSH connection to a specific device.
    pub fn connect_device(
        &self,
        device_id: &str,
        forward_host: &str,
        port: u16,
    ) -> impl Future<Item = protos::control::SshConnectionResponse, Error = std::io::Error> {
        let mut message = protos::control::ClientMessage::new();
        let mut ssh_connection = protos::control::SshConnection::new();
        let mut enable = protos::control::SshConnection_Enable::new();
        enable.set_forward_host(forward_host.into());
        enable.set_forward_port(port as u32);
        enable.set_gateway_port(true);
        ssh_connection.set_device_id(device_id.into());
        ssh_connection.set_enable(enable);
        message.set_message_id(1);
        message.set_ssh_connection(ssh_connection);

        RequestResponseFuture::new(&self.addr, message)
            .map(|mut response| response.take_ssh_connection_response())
    }

    /// Tell the server to disconnect and stop establishing a specific SSH connection.
    pub fn disconnect_connection(
        &self,
        device_id: &str,
        connection_id: &str,
    ) -> impl Future<Item = protos::control::SshConnectionResponse, Error = std::io::Error> {
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

    /// Tell the server to extend a specific SSH connection.
    pub fn extend_connection(
        &self,
        device_id: &str,
        connection_id: &str,
    ) -> impl Future<Item = protos::control::SshConnectionResponse, Error = std::io::Error> {
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

    /// Tell the server to add a new device to the server, even though the server has not seen the
    /// device.
    pub fn create_device(
        &self,
        device_id: &str,
    ) -> impl Future<Item = protos::control::CreateDeviceResponse, Error = std::io::Error> {
        let mut message = protos::control::ClientMessage::new();
        let mut create_device = protos::control::CreateDevice::new();
        create_device.set_device_id(device_id.into());
        message.set_message_id(1);
        message.set_create_device(create_device);

        RequestResponseFuture::new(&self.addr, message)
            .map(|mut response| response.take_create_device_response())
    }

    /// Tell the server to remove a device from its list. Note that if the device checks in again,
    /// it will be re-added.
    pub fn remove_device(
        &self,
        device_id: &str,
    ) -> impl Future<Item = protos::control::RemoveDeviceResponse, Error = std::io::Error> {
        let mut message = protos::control::ClientMessage::new();
        let mut remove_device = protos::control::RemoveDevice::new();
        remove_device.set_device_id(device_id.into());
        message.set_message_id(1);
        message.set_remove_device(remove_device);

        RequestResponseFuture::new(&self.addr, message)
            .map(|mut response| response.take_remove_device_response())
    }

    /// Tell the server to change the name of a device.
    pub fn set_name(
        &self,
        device_id: &str,
        name: &str,
    ) -> impl Future<Item = protos::control::SetNameResponse, Error = std::io::Error> {
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

/// A future that resolves to a list of clients and their states.
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
