//! UDP socket codec.

use bytes::Bytes;
use futures::future;
use futures::Sink;
use futures::SinkExt;
use futures::Stream;
use futures::StreamExt;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::net::SocketAddr;
use tokio::net::UdpSocket;
use tokio_util::codec::BytesCodec;
use tokio_util::udp::UdpFramed;

#[derive(thiserror::Error, Debug)]
pub(crate) enum Error {
    #[error("std io error")]
    Io(#[from] std::io::Error),

    #[error("bincode error")]
    Bincode(#[from] bincode::Error),
}

/// UDP transport.
///
/// UDP socket wrapped with framing and (de)serialization.
///
/// This is a one-to-many relationship suitable for a server. The stream emits
/// the socket address of the sender with every item. The sink requires the
/// socket address of the recipient with every item.
pub(crate) fn new<St, Sk>(
    udp_socket: UdpSocket,
) -> (
    impl Stream<Item = (Result<St, Error>, SocketAddr)>,
    impl Sink<(Sk, SocketAddr), Error = Error>,
)
where
    St: DeserializeOwned,
    Sk: Serialize + Send + 'static,
{
    // Stream/sink over UDP datagrams
    let framed = UdpFramed::new(udp_socket, BytesCodec::new());
    let (sink, stream) = framed.split();

    let stream = stream.map(|r| r.expect("BytesCodec never fails deserialization"));

    // Deserialize
    let deserializing_stream = stream.map(|(buf, socket_addr)| {
        let deserialized: Result<St, Error> = net_stream::deserialize(&buf).map_err(Error::from);
        (deserialized, socket_addr)
    });

    // Serialize
    let serializing_sink = sink.with(|(item, socket_addr): (Sk, SocketAddr)| {
        let serialized = match net_stream::serialize(&item) {
            Ok(v) => Ok((Bytes::from(v), socket_addr)),
            Err(e) => Err(Error::Bincode(e)),
        };
        future::ready(serialized)
    });

    (deserializing_stream, serializing_sink)
}
