//! Server.

mod actor;
mod actor_handle;
pub mod event;
mod peer_uid;

use self::event::Event;
use crate::message_types::MessageTypes;
pub use actor_handle::ActorHandle;
use futures::channel::mpsc;
pub use peer_uid::PeerUid;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::net::UdpSocket;

/// Error starting server.
#[derive(thiserror::Error, Debug)]
pub enum StartServerError {
    /// IO error
    #[error("IO Error")]
    Io(#[from] ::std::io::Error),
}

const DEFAULT_MAX_CONNECTIONS: usize = 2;
const DEFAULT_UDP_HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

/// Server configuration
#[derive(Debug, Clone, smart_default::SmartDefault, typed_builder::TypedBuilder)]
pub struct Config {
    /// Limit number of concurrently connected clients.
    #[default(DEFAULT_MAX_CONNECTIONS)]
    #[builder(default = DEFAULT_MAX_CONNECTIONS)]
    pub max_connections: usize,

    /// Interval between each UDP heartbeat message being emitted to connected peers.
    #[default(DEFAULT_UDP_HEARTBEAT_INTERVAL)]
    #[builder(default = DEFAULT_UDP_HEARTBEAT_INTERVAL)]
    pub udp_heartbeat_interval: std::time::Duration,
}

/// Start server.
///
/// This returns a handle to the server actor task and an event stream.
/// The [ActorHandle] is used to command the server.
/// The event stream receiver is used to listen to server events.
///
/// The actor handles can be freely cloned. When the last handle to the server actor is
/// dropped, or the receiver is closed or dropped, the server task will begin graceful
/// shut down.
pub async fn start<M: MessageTypes>(
    tcp_socket_addr: SocketAddr,
    udp_socket_addr: SocketAddr,
    config: Config,
) -> Result<(ActorHandle<M>, mpsc::UnboundedReceiver<Event<M>>), StartServerError> {
    let tcp_listener = TcpListener::bind(tcp_socket_addr).await?;
    let udp_socket = UdpSocket::bind(udp_socket_addr).await?;

    let (actor_msg_sender, actor_msg_receiver) = mpsc::unbounded::<actor::Message<M>>();
    let (event_sender, event_receiver) = mpsc::unbounded::<Event<M>>();

    let join_handle = tokio::spawn(actor::actor(actor_msg_receiver, tcp_listener, udp_socket, event_sender, config));
    let actor_handle = ActorHandle::new(join_handle, actor_msg_sender);

    Ok((actor_handle, event_receiver))
}
