mod actor;
mod actor_handle;

use crate::MessageTypes;
use actor_handle::ActorHandle;
use futures::channel::mpsc;
use serde::Deserialize;
use serde::Serialize;

/// Connect.
pub async fn connect<M: MessageTypes>(server_host: &str) -> Result<(ActorHandle<M>, futures::channel::mpsc::Receiver<Event<M>>), ConnectError> {
    // Resolve host (DNS lookup)
    log::info!("Resolving host {server_host}");
    let mut server_addr_iter = tokio::net::lookup_host(server_host).await?;
    let server_addr = server_addr_iter.next().ok_or_else(|| ConnectError::UnknownHost {
        host: server_host.to_owned(),
    })?;
    log::info!("Resolved {server_host} to {server_addr}");

    log::info!("Connecting TCP stream");
    let tcp_stream = tokio::net::TcpStream::connect(&server_addr).await?;
    log::info!("Connected TCP stream");

    let local_addr = tcp_stream.local_addr()?;
    log::info!("TCP stream local address: {local_addr}");

    let (actor_msg_sender, actor_msg_receiver) = mpsc::channel::<actor::Message<M>>(32);
    let (event_sender, event_receiver) = mpsc::channel::<Event<M>>(128);

    let join_handle = tokio::spawn(actor::actor(actor_msg_receiver, event_sender, tcp_stream, server_addr));
    let actor_handle = ActorHandle::new(join_handle, actor_msg_sender);

    Ok((actor_handle, event_receiver))
}

/// Client event.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Event<M: MessageTypes> {
    /// Received message.
    Message(M::FromServer),
}

/// Error connecting to server.
#[derive(thiserror::Error, Debug)]
pub enum ConnectError {
    /// IO error.
    #[error("IO Error")]
    Io(#[from] std::io::Error),

    /// Error resolving server host.
    #[error("Unknown host: {}", host)]
    UnknownHost {
        /// Host
        host: String,
    },
}
