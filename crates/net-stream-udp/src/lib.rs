//! UDP client and server.

#![deny(rust_2018_idioms, nonstandard_style, future_incompatible)]
#![deny(clippy::mod_module_files)]
#![warn(missing_docs)]

mod client;
mod codec;
mod server;

use core::fmt;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Serialize;

/// Message types for the server and client communication.
///
/// This is a trait with no methods, used only to define strong types for
/// serialization and deserialization.
pub trait MessageTypes: 'static + fmt::Debug {
    /// Type for peers to send to the server.
    type ToServer: Serialize + DeserializeOwned + fmt::Debug + Clone + Send + Sync + Unpin + 'static;

    /// Type for server to send to peers.
    type FromServer: Serialize + DeserializeOwned + fmt::Debug + Clone + Send + Sync + Unpin + 'static;
}

/// Message to server.
#[derive(Debug, Serialize, Deserialize)]
enum MsgToServer<M: MessageTypes> {
    /// Pass-through application level messages.
    ApplicationLogic(M::ToServer),
}
// Deriving Clone doesn't work because MessageTypes itself is not Clone, even though all the types in MessageTypes are.
impl<M: MessageTypes> Clone for MsgToServer<M> {
    fn clone(&self) -> Self {
        match self {
            Self::ApplicationLogic(msg) => Self::ApplicationLogic(msg.clone()),
        }
    }
}

/// Message from server.
#[derive(Debug, Serialize, Deserialize)]
enum MsgFromServer<M: MessageTypes> {
    /// Pass-through application level messages.
    ApplicationLogic(M::FromServer),

    /// Repeating heartbeat to keep firewall UPnP port forwards open from server to client.
    Heartbeat,
}
// Deriving Clone doesn't work because MessageTypes itself is not Clone, even though all the types in MessageTypes are.
impl<M: MessageTypes> Clone for MsgFromServer<M> {
    fn clone(&self) -> Self {
        match self {
            Self::ApplicationLogic(msg) => Self::ApplicationLogic(msg.clone()),
            Self::Heartbeat => Self::Heartbeat,
        }
    }
}
