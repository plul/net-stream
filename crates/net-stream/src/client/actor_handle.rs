//! Client actor task handle.

use super::actor::Message;
use crate::message_types::MessageTypes;
use assert_let_bind::assert_let;
use futures::channel::mpsc;
use std::sync::Arc;
use tokio::task::JoinHandle;

/// Handle to the client actor task.
#[derive(Debug, Clone)]
pub struct ActorHandle<M: MessageTypes> {
    sender: mpsc::Sender<Message<M>>,
    _join_handle: Arc<JoinHandle<()>>,
}

/// Error communicating message to actor. The message is returned.
#[derive(thiserror::Error, Debug)]
pub enum Error<M> {
    /// Actor cannot keep up, channel is currently full.
    #[error("Actor cannot keep up, channel is currently full.")]
    ChannelFull(M),

    /// Actor is unreachable - channel is disconnected.
    #[error("Actor is unreachable, channel disconnected.")]
    ActorUnreachable(M),
}

impl<M> ActorHandle<M>
where
    M: MessageTypes,
{
    /// Create new actor handle.
    pub(crate) fn new(join_handle: JoinHandle<()>, sender: mpsc::Sender<Message<M>>) -> ActorHandle<M> {
        Self {
            _join_handle: Arc::new(join_handle),
            sender,
        }
    }

    /// Send message to server on TCP.
    pub fn send_message_tcp(&mut self, msg: M::TcpToServer) -> Result<(), Error<M::TcpToServer>> {
        self.sender.try_send(Message::TcpToServer { msg }).map_err(|err| {
            if err.is_full() {
                assert_let!(Message::TcpToServer { msg }, err.into_inner());
                Error::ChannelFull(msg)
            } else if err.is_disconnected() {
                assert_let!(Message::TcpToServer { msg }, err.into_inner());
                Error::ActorUnreachable(msg)
            } else {
                unreachable!()
            }
        })
    }

    /// Send message to server on UDP (lossy).
    pub fn send_message_udp(&mut self, msg: M::UdpToServer) -> Result<(), Error<M::UdpToServer>> {
        self.sender.try_send(Message::UdpToServer { msg }).map_err(|err| {
            if err.is_full() {
                assert_let!(Message::UdpToServer { msg }, err.into_inner());
                Error::ChannelFull(msg)
            } else if err.is_disconnected() {
                assert_let!(Message::UdpToServer { msg }, err.into_inner());
                Error::ActorUnreachable(msg)
            } else {
                unreachable!()
            }
        })
    }

    /// Get status
    pub async fn get_status(&mut self) -> Result<crate::client::Status, Error<()>> {
        let (tx, rx) = futures::channel::oneshot::channel();
        self.sender.try_send(Message::GetStatus { tx }).map_err(|err| {
            if err.is_full() {
                Error::ChannelFull(())
            } else if err.is_disconnected() {
                Error::ActorUnreachable(())
            } else {
                unreachable!()
            }
        })?;

        let status = rx.await.expect("Expected response from actor");
        Ok(status)
    }
}
