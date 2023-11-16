//! Server.

mod actor;
mod actor_handle;
pub mod event;

use crate::MessageTypes;
pub use actor_handle::ActorHandle;
use futures::channel::mpsc;
use serde::Deserialize;
use serde::Serialize;
use smart_default::SmartDefault;
use std::net::SocketAddr;
use tokio::net::UdpSocket;

/// Error starting server.
#[derive(thiserror::Error, Debug)]
pub enum StartServerError {
    /// IO error
    #[error("IO Error")]
    Io(#[from] ::std::io::Error),
}

// TODO: What does this mean for the UDP server?
const DEFAULT_MAX_CONNECTIONS: usize = 2;
const DEFAULT_UDP_HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

/// Server configuration
///
/// # Examples
/// ```
/// # use net_stream::server::Config;
/// let server_config = Config {
///    max_connections: 100,
///    ..Default::default()
/// };
/// ```
#[derive(Debug, Clone, SmartDefault)]
pub struct Config {
    /// Limit number of concurrently connected clients.
    #[default(DEFAULT_MAX_CONNECTIONS)]
    pub max_connections: usize,

    /// Interval between each UDP heartbeat message being emitted to connected peers.
    #[default(DEFAULT_UDP_HEARTBEAT_INTERVAL)]
    pub heartbeat_interval: std::time::Duration,
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
    socket_addr: SocketAddr,
    config: Config,
) -> Result<(ActorHandle<M>, mpsc::UnboundedReceiver<event::Event<M>>), StartServerError> {
    let udp_socket = UdpSocket::bind(socket_addr).await?;

    let (actor_msg_sender, actor_msg_receiver) = mpsc::unbounded::<actor::Message<M>>();
    let (event_sender, event_receiver) = mpsc::unbounded::<event::Event<M>>();

    let join_handle = tokio::spawn(actor::actor(actor_msg_receiver, udp_socket, event_sender, config));
    let actor_handle = ActorHandle::new(join_handle, actor_msg_sender);

    Ok((actor_handle, event_receiver))
}

/// Unique ID for a client connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default, derive_more::Deref, derive_more::From)]
pub struct PeerUid(pub u64);
impl PeerUid {
    pub(crate) fn increment(&mut self) {
        self.0 += 1;
    }
}
