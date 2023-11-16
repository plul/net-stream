//! Server/client connection message types.

use core::fmt;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

/// Message types for the server and client communication.
///
/// This is a trait with no methods, used only to define strong types for TCP/UDP
/// serialization and deserialization.
pub trait MessageTypes: 'static + fmt::Debug {
    /// Type for peers to send to the server over TCP.
    type TcpToServer: Serialize + DeserializeOwned + fmt::Debug + Clone + Send + Sync + Unpin + 'static;

    /// Type for server to send to peers over TCP.
    type TcpFromServer: Serialize + DeserializeOwned + fmt::Debug + Clone + Send + Sync + Unpin + 'static;

    /// Type for peers to send to server over UDP.
    type UdpToServer: Serialize + DeserializeOwned + fmt::Debug + Clone + Send + Sync + Unpin + 'static;

    /// Type for server to send to peers over UDP.
    type UdpFromServer: Serialize + DeserializeOwned + fmt::Debug + Clone + Send + Sync + Unpin + 'static;
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum TcpToServer<M: MessageTypes> {
    /// Pass-through application level messages.
    ApplicationLogic(M::TcpToServer),
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum TcpFromServer<M: MessageTypes> {
    /// Pass-through application level messages.
    ApplicationLogic(M::TcpFromServer),

    /// Welcome message, containing a unique UUID that the client uses to identify itself
    /// when sending its first UDP message.
    Welcome { handshake_uuid: Uuid },

    /// An acknowledgement that an UDP Hello message carrying the handshake UUID has been
    /// received on the server side.
    UdpHandshakeAcknowledged,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum UdpToServer<M: MessageTypes> {
    /// Pass-through application level messages.
    ApplicationLogic(M::UdpToServer),

    /// Hello message, containing the same UUID from the TCP welcome message.
    ///
    /// Sending this message from the client to the server sets up NAT on any gateway, and
    /// allows the server to match the UDP socket address and port number to the TCP socket
    /// of the client.
    Hello { handshake_uuid: Uuid },
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum UdpFromServer<M: MessageTypes> {
    /// Pass-through application level messages.
    ApplicationLogic(M::UdpFromServer),

    /// One-time welcome UDP message from server.
    Welcome,

    /// Repeating heartbeat to keep firewall UPnP port forwards open from server to client.
    Heartbeat,
}

// Deriving Clone doesn't work because MessageTypes itself is not Clone, even though all the types in MessageTypes are.
impl<M: MessageTypes> Clone for TcpToServer<M> {
    fn clone(&self) -> Self {
        match self {
            Self::ApplicationLogic(msg) => Self::ApplicationLogic(msg.clone()),
        }
    }
}
impl<M: MessageTypes> Clone for TcpFromServer<M> {
    fn clone(&self) -> Self {
        match self {
            Self::ApplicationLogic(msg) => Self::ApplicationLogic(msg.clone()),
            Self::Welcome { handshake_uuid } => Self::Welcome {
                handshake_uuid: *handshake_uuid,
            },
            Self::UdpHandshakeAcknowledged => Self::UdpHandshakeAcknowledged,
        }
    }
}
impl<M: MessageTypes> Clone for UdpToServer<M> {
    fn clone(&self) -> Self {
        match self {
            Self::ApplicationLogic(msg) => Self::ApplicationLogic(msg.clone()),
            Self::Hello { handshake_uuid } => Self::Hello {
                handshake_uuid: *handshake_uuid,
            },
        }
    }
}
impl<M: MessageTypes> Clone for UdpFromServer<M> {
    fn clone(&self) -> Self {
        match self {
            Self::ApplicationLogic(msg) => Self::ApplicationLogic(msg.clone()),
            Self::Welcome => Self::Welcome,
            Self::Heartbeat => Self::Heartbeat,
        }
    }
}
