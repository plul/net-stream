//! Event.

use crate::MessageTypes;
use serde::Deserialize;
use serde::Serialize;

/// Client event.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Event<M: MessageTypes> {
    /// Received message.
    Message(M::FromServer),

    /// Client has received its first UDP message from server, confirming that there is no firewall blocking incoming UDP messages.
    CanReceiveMessages,
}
