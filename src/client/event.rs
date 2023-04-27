//! Client output event types.

use super::MessageTypes;
use serde::Deserialize;
use serde::Serialize;

/// Client event.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Event<M: MessageTypes> {
    /// Received message on TCP.
    TcpMessage(M::TcpFromServer),

    /// Received message on UDP.
    UdpMessage(M::UdpFromServer),

    /// Client is ready to send UDP messages to server.
    CanSendUdpMessages,

    /// Client is ready to receive UDP messages from server.
    CanReceiveUdpMessages,
}
