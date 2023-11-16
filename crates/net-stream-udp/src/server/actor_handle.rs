use super::actor::Message;
use super::PeerUid;
use crate::MessageTypes;
use futures::channel::mpsc;
use futures::channel::oneshot;
use std::sync::Arc;
use tokio::task::JoinHandle;

/// Handle to the server actor task.
#[derive(Debug, Clone)]
pub struct ActorHandle<M: MessageTypes> {
    sender: mpsc::UnboundedSender<Message<M>>,
    _join_handle: Arc<JoinHandle<()>>,
}

impl<M> ActorHandle<M>
where
    M: MessageTypes,
{
    /// Create new handle
    pub(crate) fn new(join_handle: JoinHandle<()>, sender: mpsc::UnboundedSender<Message<M>>) -> ActorHandle<M> {
        Self {
            _join_handle: Arc::new(join_handle),
            sender,
        }
    }

    /// Send message to client on UDP (lossy).
    pub fn message_peer(&self, peer_uid: PeerUid, msg: M::FromServer) {
        log::debug!("Message {peer_uid:?} UDP");
        self.sender
            .unbounded_send(Message::ToPeer { peer_uid, msg })
            .expect("Actor is not accepting any new messages")
    }

    /// Send announcement to all connected clients on UDP (lossy).
    pub fn announce(&self, msg: M::FromServer) {
        log::debug!("Announce UDP");
        self.sender
            .unbounded_send(Message::Announce { msg })
            .expect("Actor is not accepting any new messages")
    }

    /// Get number of connected peers.
    pub async fn get_number_of_connected_peers(&self) -> usize {
        let (tx, rx) = oneshot::channel();
        self.sender
            .unbounded_send(Message::GetNumberOfConnectedPeers { tx })
            .expect("Actor is not accepting any new messages");

        rx.await.expect("Expected response from actor")
    }
}
