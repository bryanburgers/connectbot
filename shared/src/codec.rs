//! A tokio_io::codec that is a stream of protocol buffers prefixed by a 4-byte length.

use bytes::{Buf, BufMut, BytesMut};
use protobuf;
use std::io;
use std::io::Cursor;
use std::marker::PhantomData;
use tokio_io::codec::{Decoder, Encoder};

/// A codec type where each message (both encoded and decoded) is a protocol buffer, prefixed by a
/// 4-byte length.
pub struct Codec<E, D> {
    decode_type: PhantomData<D>,
    encode_type: PhantomData<E>,
}

impl<E, D> Codec<E, D> {
    /// Create a new codec.
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
        buf.reserve(size + 4);
        buf.put_u32_be(size as u32);

        unsafe {
            buf.set_len(previous_len + 4 + size);
        }

        {
            let mut coded_output_stream =
                protobuf::CodedOutputStream::bytes(&mut buf[previous_len + 4..]);
            client_message.write_to(&mut coded_output_stream)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protos::device;
    use bytes::BytesMut;
    use protobuf::Message;
    use tokio_io::codec::{Decoder, Encoder};

    const SHORT_MESSAGE_LEN: usize = 20;
    const LONG_MESSAGE_LEN: usize = 1699;

    type TestCodec = Codec<device::ServerMessage, device::ServerMessage>;

    #[derive(Debug, Clone, Eq, PartialEq)]
    struct TestMessage {
        host: String,
        port: usize,
        user: String,
        key: String,
    }

    impl TestMessage {
        fn short(n: usize) -> TestMessage {
            TestMessage {
                host: format!("test-{}", n).into(),
                port: n,
                user: "test".into(),
                key: "".into(),
            }
        }

        fn long(n: usize) -> TestMessage {
            let key = r#"-----BEGIN RSA PRIVATE KEY-----
MIIEogIBAAKCAQEA1Zn+5guNN696dlTthowErcUxcrFf6CGM1QD3MQPsAWLjWTK/
4rSNjM9RlHnxWMwICsE5VNfUZtp/UtY87gx0EYXFF6el1quZ8V6fwt2rtGkNtICi
UIj7sIjG9T/Cl2+KsevVvJf3L1lBGYrIMQTyCvgXkFpkNoOjYcfHXLyvlh9liSkX
xWi3pwdm/YBHFMuSJJDhDdxiZDmAGxkI9/MkWr6V9iZI9l1YCfugLmEoVtSlN+0q
46DTPUgoPzepLC8e4FHNPJ8DQepbACareEfHzLfKsc/mohpEpRNBvWsXLX31/Svv
4csW/evlUlAus+gyoJI1XNd5A0dMD+sy9ICikwIDAQABAoIBAB8HL6/bHbhpFTD7
RUW2MTfM3VH70iK2PO70JPRJzY6l/sCGTrlv4OADfaZD0HtFqCVnzBw2/fOy6avu
0wsBZBrng6ncAIsegk49oJd9++NJH2SJCwsH2wfZ1ozppiq5WTxfNb0flhiaroo3
Tr1QKpjNUR73Aneox6L8kkk2X4s+4Bq0VedJYpXa3RYWElrb4yylC64ZksYILmn3
BiKwG8G/u+JrRTJni5uIBgM8vqFlkZ9b4X8yi2hBZgaXh100XYpyjKUVp4v3tDiw
/Q3rgKi1AaXpNfbjZD9rVrhIRMhkOQxdif5/5RyTNU7piETpWJ8cihr+AouAHhB6
3Oph6IECgYEA/9U9IVB2v+A4TTBf47PE6boJtlwkXeodlk2cHRJSPvaZkx6/SspO
j+/8eYX118+7/kuL9DDlWB+12dcvb6dTQxhibNgrX/ZVygv5WGitXo21jcs82WTO
WoNjmBVmwfbcyxRTxjKCeUHlPPxpftzx9vZtz+uTlEgwYgaUOVEByMUCgYEA1b2y
uQxpviSsK9iojRajwLYIBL9YemBt+M3FoaHpOGFBCocu2vQ2axWYtaEH4EgqpDWW
+N/M1gndpYhd5lpg8p3El0JmZAEquBibvqdAhtIAqy6QlMQk5HZ0+7yyO8+OoMhl
+ZpdmfUGm173tC8evjvb3bXjJyoF4I4MLmsqA3cCgYBlYaiG8i8M3JsTI69sOco3
4SyGIr+ao/Mzo+/QqXkEUI8NeSrPRZqaebzwn4CMFFtoa6G7lEDeijpzaE35DjL1
rM0cWxHdRm460kHuohTKGpgu57JmaAdKYTTviNOe2+glZhnIui1wRgfFAjYAOyh7
+K4NrkpegbkCr56/k/WEDQKBgBBvIIHH6Y18JlzMsNEAT6Dunhk3WSc3qNz7fVmb
KGJ0X9reYATnyBNdurskYYWmJtkvYadLFeXTJl6m6Ilgo5mj9cynh1XjHRTAl6EG
HRkAppqC3w0BM9D5Jq+AZ7ffkpjcL7MMYmwHAfYKTENnaBa6ZYJbjNajDYahhWBA
Tx+rAoGAZ0rySXpgcYrbzBHf820rNO8j786P8MyePX9R0mUp17ZBv/hpXOD3Ku0m
CJTsAkf7vaZX9TVEKebKsyNxG6I5T8e2r1s12jlIOSBHt8kY1ur4eY5y4fPxen/m
6idEY7MIbsRm0MmuTysS0ppq7WWXpPcdIqA5ss4lOQkROWoQjJ4=
-----END RSA PRIVATE KEY-----"#;

            TestMessage {
                host: format!("test-{}", n).into(),
                port: n,
                user: "test".into(),
                key: key.into(),
            }
        }
    }

    impl From<device::ServerMessage> for TestMessage {
        fn from(mut item: device::ServerMessage) -> Self {
            let mut ssh_connection = item.take_ssh_connection();
            let enable = ssh_connection.take_enable();

            TestMessage {
                host: enable.get_ssh_host().into(),
                port: enable.get_ssh_port() as usize,
                user: enable.get_ssh_username().into(),
                key: enable.get_ssh_key().into(),
            }
        }
    }

    impl From<TestMessage> for device::ServerMessage {
        fn from(item: TestMessage) -> Self {
            let mut ssh_enable = device::SshConnection_Enable::new();
            ssh_enable.set_ssh_host(item.host.into());
            ssh_enable.set_ssh_port(item.port as u32);
            ssh_enable.set_ssh_username(item.user.into());
            ssh_enable.set_ssh_key(item.key.into());

            let mut ssh_connection = device::SshConnection::new();
            ssh_connection.set_enable(ssh_enable);

            let mut server_message = device::ServerMessage::new();
            server_message.set_ssh_connection(ssh_connection);

            server_message
        }
    }

    #[test]
    fn message_size_short() {
        let message: device::ServerMessage = TestMessage::short(1).into();
        assert_eq!(message.compute_size() as usize, SHORT_MESSAGE_LEN);
    }

    #[test]
    fn message_size_long() {
        let message: device::ServerMessage = TestMessage::long(1).into();
        assert_eq!(message.compute_size() as usize, LONG_MESSAGE_LEN);
    }

    #[test]
    fn short_write_read_1() {
        let mut codec = TestCodec::new();

        let mut buf = BytesMut::new();

        let message_1 = TestMessage::short(1);

        let write_result = codec.encode(message_1.clone().into(), &mut buf);
        assert!(write_result.is_ok());

        let read_result = codec.decode(&mut buf);
        assert!(read_result.is_ok());
        let read_result = read_result.unwrap();
        assert_eq!(read_result.map(Into::<TestMessage>::into), Some(message_1));

        let read_result = codec.decode(&mut buf);
        assert!(read_result.is_ok());
        let read_result = read_result.unwrap();
        assert_eq!(read_result.map(Into::<TestMessage>::into), None);
    }

    #[test]
    fn short_write_read_2() {
        let mut codec = TestCodec::new();

        let mut buf = BytesMut::new();

        let message_1 = TestMessage::short(1);
        let message_2 = TestMessage::short(2);

        let write_result = codec.encode(message_1.clone().into(), &mut buf);
        assert!(write_result.is_ok());
        let write_result = codec.encode(message_2.clone().into(), &mut buf);
        assert!(write_result.is_ok());

        let read_result = codec.decode(&mut buf);
        assert!(read_result.is_ok());
        let read_result = read_result.unwrap();
        assert_eq!(read_result.map(Into::<TestMessage>::into), Some(message_1));

        let read_result = codec.decode(&mut buf);
        assert!(read_result.is_ok());
        let read_result = read_result.unwrap();
        assert_eq!(read_result.map(Into::<TestMessage>::into), Some(message_2));

        let read_result = codec.decode(&mut buf);
        assert!(read_result.is_ok());
        let read_result = read_result.unwrap();
        assert_eq!(read_result.map(Into::<TestMessage>::into), None);
    }

    #[test]
    fn short_write_read_3() {
        let mut codec = TestCodec::new();

        let mut buf = BytesMut::new();

        let message_1 = TestMessage::short(1);
        let message_2 = TestMessage::short(2);
        let message_3 = TestMessage::short(3);

        let write_result = codec.encode(message_1.clone().into(), &mut buf);
        assert!(write_result.is_ok());
        let write_result = codec.encode(message_2.clone().into(), &mut buf);
        assert!(write_result.is_ok());
        let write_result = codec.encode(message_3.clone().into(), &mut buf);
        assert!(write_result.is_ok());

        let read_result = codec.decode(&mut buf);
        assert!(read_result.is_ok());
        let read_result = read_result.unwrap();
        assert_eq!(read_result.map(Into::<TestMessage>::into), Some(message_1));

        let read_result = codec.decode(&mut buf);
        assert!(read_result.is_ok());
        let read_result = read_result.unwrap();
        assert_eq!(read_result.map(Into::<TestMessage>::into), Some(message_2));

        let read_result = codec.decode(&mut buf);
        assert!(read_result.is_ok());
        let read_result = read_result.unwrap();
        assert_eq!(read_result.map(Into::<TestMessage>::into), Some(message_3));

        let read_result = codec.decode(&mut buf);
        assert!(read_result.is_ok());
        let read_result = read_result.unwrap();
        assert_eq!(read_result.map(Into::<TestMessage>::into), None);
    }

    #[test]
    fn long_write_read_1() {
        let mut codec = TestCodec::new();

        let mut buf = BytesMut::new();

        let message_1 = TestMessage::long(1);

        let write_result = codec.encode(message_1.clone().into(), &mut buf);
        assert!(write_result.is_ok());

        let read_result = codec.decode(&mut buf);
        assert!(read_result.is_ok());
        let read_result = read_result.unwrap();
        assert_eq!(read_result.map(Into::<TestMessage>::into), Some(message_1));

        let read_result = codec.decode(&mut buf);
        assert!(read_result.is_ok());
        let read_result = read_result.unwrap();
        assert_eq!(read_result.map(Into::<TestMessage>::into), None);
    }

    #[test]
    fn long_write_read_2() {
        let mut codec = TestCodec::new();

        let mut buf = BytesMut::new();

        let message_1 = TestMessage::long(1);
        let message_2 = TestMessage::long(2);

        let write_result = codec.encode(message_1.clone().into(), &mut buf);
        assert!(write_result.is_ok());
        let write_result = codec.encode(message_2.clone().into(), &mut buf);
        assert!(write_result.is_ok());

        let read_result = codec.decode(&mut buf);
        assert!(read_result.is_ok());
        let read_result = read_result.unwrap();
        assert_eq!(read_result.map(Into::<TestMessage>::into), Some(message_1));

        let read_result = codec.decode(&mut buf);
        assert!(read_result.is_ok());
        let read_result = read_result.unwrap();
        assert_eq!(read_result.map(Into::<TestMessage>::into), Some(message_2));

        let read_result = codec.decode(&mut buf);
        assert!(read_result.is_ok());
        let read_result = read_result.unwrap();
        assert_eq!(read_result.map(Into::<TestMessage>::into), None);
    }

    #[test]
    fn long_write_read_3() {
        let mut codec = TestCodec::new();

        let mut buf = BytesMut::new();

        let message_1 = TestMessage::long(1);
        let message_2 = TestMessage::long(2);
        let message_3 = TestMessage::long(3);

        let write_result = codec.encode(message_1.clone().into(), &mut buf);
        assert!(write_result.is_ok());
        let write_result = codec.encode(message_2.clone().into(), &mut buf);
        assert!(write_result.is_ok());
        let write_result = codec.encode(message_3.clone().into(), &mut buf);
        assert!(write_result.is_ok());

        let read_result = codec.decode(&mut buf);
        assert!(read_result.is_ok());
        let read_result = read_result.unwrap();
        assert_eq!(read_result.map(Into::<TestMessage>::into), Some(message_1));

        let read_result = codec.decode(&mut buf);
        assert!(read_result.is_ok());
        let read_result = read_result.unwrap();
        assert_eq!(read_result.map(Into::<TestMessage>::into), Some(message_2));

        let read_result = codec.decode(&mut buf);
        assert!(read_result.is_ok());
        let read_result = read_result.unwrap();
        assert_eq!(read_result.map(Into::<TestMessage>::into), Some(message_3));

        let read_result = codec.decode(&mut buf);
        assert!(read_result.is_ok());
        let read_result = read_result.unwrap();
        assert_eq!(read_result.map(Into::<TestMessage>::into), None);
    }
}
