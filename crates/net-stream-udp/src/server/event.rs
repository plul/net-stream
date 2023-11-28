//! Server output event types.

use super::MessageTypes;
use super::PeerUid;
use serde::Deserialize;
use serde::Serialize;

/// Server output event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Event<M: MessageTypes> {
    /// New peer connected.
    NewPeer(NewPeer),

    /// UDP message received.
    Message(Message<M::ToServer>),

    /// Un-decodable payload was received from peer.
    ///
    /// If additional decode errors happen for this peer, no more events will be emitted.
    UnintelligiblePeer(UnintelligiblePeer),

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

/// Un-decodable payload was received from peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UnintelligiblePeer {
    /// Peer unique ID.
    pub peer_uid: PeerUid,
}

/// Reason for a peer disconnect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DisconnectReason {
    // TODO
}

/// Received message from connected peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Message<T> {
    /// Sender.
    pub from: PeerUid,

    /// Message.
    pub message: T,
}
