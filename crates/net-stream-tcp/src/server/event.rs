//! Event.

use super::PeerUid;
use crate::MessageTypes;
use serde::Deserialize;
use serde::Serialize;

/// Server output event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Event<M: MessageTypes> {
    /// New peer connected.
    NewPeer(NewPeer),

    /// TCP message received.
    Message(Message<M::ToServer>),

    /// Peer is disconnected.
    PeerDisconnect(PeerDisconnect),
}

/// New peer connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NewPeer {
    /// Peer unique ID.
    pub peer_uid: PeerUid,
}

/// Peer disconnect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PeerDisconnect {
    /// Peer unique ID.
    pub peer_uid: PeerUid,

    /// Reason for disconnect.
    pub disconnect_reason: DisconnectReason,
}

/// Reason for a peer disconnect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DisconnectReason {
    /// Peer closed connection.
    PeerClosedConnection,

    /// Peer submitted unintelligble data that could not be decoded.
    PeerSubmittedUnintelligibleData,
}

/// Received message from connected peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Message<T> {
    /// Sender.
    pub from: PeerUid,

    /// Message.
    pub message: T,
}
