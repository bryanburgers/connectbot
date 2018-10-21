use std::io;
use std::io::Cursor;
use std::marker::PhantomData;
use bytes::{BytesMut, Buf, BufMut};
use tokio_io::codec::{Decoder, Encoder};
use protobuf;

// use super::protos::ServerMessage as ServerMessage;
// use super::protos::ClientMessage as ClientMessage;

pub struct Codec<E, D> {
    decode_type: PhantomData<D>,
    encode_type: PhantomData<E>,
}

impl<E, D> Codec<E, D> {
    pub fn new() -> Codec<E, D> {
        Codec {
            decode_type: PhantomData,
            encode_type: PhantomData,
        }
    }
}

impl<E, D: protobuf::Message> Decoder for Codec<E, D> {
    type Item = D;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<D>, io::Error> {
        if buf.len() < 4 {
            return Ok(None);
        }

        let size = {
            let mut cursor = Cursor::new(&mut *buf);
            cursor.get_uint_be(4) as usize
        };

        if buf.len() < 4 + size {
            return Ok(None);
        }

        let mut message_buf = buf.split_to(4 + size);
        message_buf.advance(4);

        let server_message = protobuf::parse_from_bytes(&message_buf)?;

        Ok(Some(server_message))
    }
}

impl<E: protobuf::Message, D> Encoder for Codec<E, D> {
    type Item = E;
    type Error = io::Error;

    fn encode(&mut self, client_message: E, buf: &mut BytesMut) -> Result<(), io::Error> {
        let previous_len = buf.len();
        let size = client_message.compute_size() as usize;
        buf.put_u32_be(size as u32);
        buf.reserve(size);

        unsafe {
            buf.set_len(previous_len + 4 + size);
        }

        {
            let mut coded_output_stream = protobuf::CodedOutputStream::bytes(&mut buf[previous_len + 4..]);
            client_message.write_to(&mut coded_output_stream)?;
        }

        Ok(())
    }
}
