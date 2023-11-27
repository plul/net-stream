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
use tokio::net::TcpListener;

/// Error starting server.
#[derive(thiserror::Error, Debug)]
pub enum StartServerError {
    /// IO error
    #[error("IO Error")]
    Io(#[from] ::std::io::Error),
}

const DEFAULT_MAX_CONNECTIONS: usize = 2;

/// Server configuration
///
/// # Examples
/// ```
/// # use net_stream_tcp::server::Config;
/// let server_config = Config {
///    max_connections: 100,
/// };
/// ```
#[derive(Debug, Clone, SmartDefault)]
pub struct Config {
    /// Limit number of concurrently connected clients.
    #[default(DEFAULT_MAX_CONNECTIONS)]
    pub max_connections: usize,
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
pub async fn start<M: MessageTypes>(socket_addr: SocketAddr, config: Config) -> Result<Server<M>, StartServerError> {
    let tcp_listener = TcpListener::bind(socket_addr).await?;
    let local_addr = tcp_listener.local_addr()?;

    let (actor_msg_sender, actor_msg_receiver) = mpsc::unbounded::<actor::Message<M>>();
    let (event_sender, event_receiver) = mpsc::unbounded::<event::Event<M>>();

    let join_handle = tokio::spawn(actor::actor(actor_msg_receiver, tcp_listener, event_sender, config));
    let actor_handle = ActorHandle::new(join_handle, actor_msg_sender);

    Ok(Server {
        actor_handle,
        event_receiver,
        local_addr,
    })
}

/// Server returned from the [start()] method.
pub struct Server<M: MessageTypes> {
    /// Actor handle.
    pub actor_handle: ActorHandle<M>,

    /// Event receiver.
    pub event_receiver: mpsc::UnboundedReceiver<event::Event<M>>,

    /// Local address.
    pub local_addr: SocketAddr,
}

/// Unique ID for a client connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default, derive_more::Deref, derive_more::From)]
pub struct PeerUid(pub u64);
impl PeerUid {
    pub(crate) fn increment(&mut self) {
        self.0 += 1;
    }
}
