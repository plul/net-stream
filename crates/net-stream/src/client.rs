//! Client.

mod actor;
pub mod actor_handle;
pub mod event;

use crate::message_types::MessageTypes;
use actor_handle::ActorHandle;
use event::Event;
use futures::channel::mpsc;
use tokio::net::TcpStream;
use tokio::net::UdpSocket;

/// Error connecting to server
#[derive(thiserror::Error, Debug)]
pub enum ConnectError {
    /// IO error.
    #[error("IO Error")]
    Io(#[from] ::std::io::Error),

    /// Error resolving server host.
    #[error("Unknown host {}", host)]
    UnknownHost {
        /// Host
        host: String,
    },
}

/// Client status
///
/// Returned from [actor_handle::ActorHandle::get_status].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Status {
    /// Time of most recently recieved UDP heartbeat from server.
    pub latest_udp_heartbeat: Option<std::time::Instant>,
}

/// Connect.
pub async fn connect<M: MessageTypes>(server_host: &str) -> Result<(ActorHandle<M>, futures::channel::mpsc::Receiver<Event<M>>), ConnectError> {
    // Resolve host (DNS lookup)
    log::info!("Resolving host {server_host}...");
    let mut server_addr_iter = tokio::net::lookup_host(server_host).await?;
    let server_addr = server_addr_iter.next().ok_or_else(|| ConnectError::UnknownHost {
        host: server_host.to_owned(),
    })?;
    log::info!("OK: {server_host} resolved to {server_addr}");

    log::info!("Connecting TCP stream...");
    let tcp_stream = TcpStream::connect(&server_addr).await?;
    log::info!("OK: Connected TCP stream");
    log::info!("Getting TCP stream local address...");
    let local_addr = tcp_stream.local_addr()?;
    log::info!("OK: TCP stream local address {local_addr}");
    log::info!("Binding UDP socket at {local_addr}...");
    let udp_socket = UdpSocket::bind(local_addr).await?;
    log::info!("OK: Bound UDP socket");
    log::info!("Setting default destination for UDP socket...");
    udp_socket.connect(&server_addr).await?;
    log::info!("OK: Set default destination for UDP socket");

    let (actor_msg_sender, actor_msg_receiver) = mpsc::channel::<actor::Message<M>>(32);
    let (event_sender, event_receiver) = mpsc::channel::<Event<M>>(128);

    let join_handle = tokio::spawn(actor::actor(actor_msg_receiver, event_sender, tcp_stream, udp_socket, server_addr));
    let actor_handle = ActorHandle::new(join_handle, actor_msg_sender);

    Ok((actor_handle, event_receiver))
}
