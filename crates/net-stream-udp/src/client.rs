//! UDP client.

mod actor;
mod actor_handle;
pub mod event;

use crate::MessageTypes;
use actor_handle::ActorHandle;
use futures::channel::mpsc;
use smart_default::SmartDefault;
use tokio::net::UdpSocket;

/// Client configuration.
///
/// # Examples
/// ```
/// # use net_stream_udp::client::Config;
/// # use std::time::Duration;
/// let server_config = Config {
///    heartbeat_interval: Duration::from_secs(1),
/// };
/// ```
#[derive(Debug, Clone, SmartDefault)]
pub struct Config {
    /// Interval between each UDP heartbeat message sent to server.
    #[default(crate::DEFAULT_UDP_HEARTBEAT_INTERVAL)]
    pub heartbeat_interval: std::time::Duration,
}

/// Connect.
// TODO: What is meant by "connect"? This is UDP. What it means to connect in this case is that we resolve the server address, start sending heartbeats and listen for incoming UDP messages from the server.
pub async fn connect<M: MessageTypes>(
    server_host: &str,
    config: Config,
) -> Result<(ActorHandle<M>, futures::channel::mpsc::Receiver<event::Event<M>>), ConnectError> {
    // Resolve host (DNS lookup)
    log::debug!("Resolving host {server_host}");
    let mut server_addr_iter = tokio::net::lookup_host(server_host).await?;
    let server_addr = server_addr_iter.next().ok_or_else(|| ConnectError::UnknownHost {
        host: server_host.to_owned(),
    })?;
    log::info!("Resolved {server_host} to {server_addr}");

    log::debug!("Binding UDP socket");
    let udp_socket = UdpSocket::bind("0.0.0.0:0").await?;
    let local_addr = udp_socket.local_addr()?;
    log::info!("Bound UDP socket at {local_addr}");

    log::debug!("Setting default destination for UDP socket");
    udp_socket.connect(&server_addr).await?;
    log::debug!("Set default destination for UDP socket");

    let (actor_msg_sender, actor_msg_receiver) = mpsc::channel::<actor::Message<M>>(32);
    let (event_sender, event_receiver) = mpsc::channel::<event::Event<M>>(128);

    let join_handle = tokio::spawn(actor::actor(actor_msg_receiver, event_sender, udp_socket, server_addr, config));
    let actor_handle = ActorHandle::new(join_handle, actor_msg_sender);

    Ok((actor_handle, event_receiver))
}

/// Client status
///
/// Returned from [actor_handle::ActorHandle::get_status].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Status {
    /// Time of most recently recieved heartbeat from server.
    pub latest_heartbeat: Option<std::time::Instant>,
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
