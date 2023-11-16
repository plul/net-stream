use super::actor::Message;
use super::peer_uid::PeerUid;
use crate::message_types::MessageTypes;
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

    /// Send message to client on TCP.
    pub fn message_peer_tcp(&self, peer_uid: PeerUid, msg: M::TcpFromServer) {
        log::debug!("Message {peer_uid:?} TCP");
        self.sender
            .unbounded_send(Message::ToPeerTcp { peer_uid, msg })
            .expect("Actor is not accepting any new messages")
    }

    /// Send message to client on UDP (lossy).
    pub fn message_peer_udp(&self, peer_uid: PeerUid, msg: M::UdpFromServer) {
        log::debug!("Message {peer_uid:?} UDP");
        self.sender
            .unbounded_send(Message::ToPeerUdp { peer_uid, msg })
            .expect("Actor is not accepting any new messages")
    }

    /// Send announcement to all connected clients on TCP.
    pub fn announce_tcp(&self, msg: M::TcpFromServer) {
        log::debug!("Announce TCP");
        self.sender
            .unbounded_send(Message::AnnounceTcp { msg })
            .expect("Actor is not accepting any new messages")
    }

    /// Send announcement to all connected clients on UDP (lossy).
    pub fn announce_udp(&self, msg: M::UdpFromServer) {
        log::debug!("Announce UDP");
        self.sender
            .unbounded_send(Message::AnnounceUdp { msg })
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
