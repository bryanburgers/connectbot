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
