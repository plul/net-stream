//! TCP stream codec.

use futures::Sink;
use futures::SinkExt;
use futures::Stream;
use futures::TryStreamExt;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::net::TcpStream;
use tokio_util::codec::FramedRead;
use tokio_util::codec::FramedWrite;
use tokio_util::codec::LengthDelimitedCodec;

#[derive(::thiserror::Error, Debug)]
pub(crate) enum Error {
    #[error("std io error")]
    Io(#[from] std::io::Error),

    #[error("bincode error")]
    Bincode(#[from] bincode::Error),
}

/// Transform a TCP Stream into a stream and a sink of Rust types.
///
/// This takes care of framing the TCP Stream, and serialization/deserialization
/// of the messages.
///
/// Messages are framed with Tokio util's length delimited codec (LV encoding) and
/// serialized with bincode, but these are implementation details subject to
/// change.
pub(crate) fn new<St, Sk>(tcp_stream: TcpStream) -> (impl Stream<Item = Result<St, Error>>, impl Sink<Sk, Error = Error>)
where
    St: DeserializeOwned,
    Sk: Serialize + Send + 'static,
{
    // Split TCP Stream into read and write halves so they can be used mutably at
    // the same time.
    let (reader, writer) = tcp_stream.into_split();

    // Delimit frames using a length header
    let framed_read = FramedRead::new(reader, LengthDelimitedCodec::new());
    let framed_write = FramedWrite::new(writer, LengthDelimitedCodec::new());

    let framed_read = framed_read.map_err(Error::Io);

    // Deserialize
    let deserialized_read = framed_read.and_then(|bytes_mut| {
        let deserialized = net_stream::deserialize(&bytes_mut).map_err(Error::Bincode);
        std::future::ready(deserialized)
    });

    // Serialize
    let serialized_write = framed_write.with(|x: Sk| {
        // bincode::serialize returns Vec<u8>.
        // Map to bytes::Bytes, because LengthDelimitedCodec implements,
        // the Encoder trait for Bytes, which makes FramedWrite a Sink
        // of Bytes as well.
        let serialized = match net_stream::serialize(&x) {
            Ok(v) => Ok(bytes::Bytes::from(v)),
            Err(e) => Err(Error::Bincode(e)),
        };
        std::future::ready(serialized)
    });

    (deserialized_read, serialized_write)
}
