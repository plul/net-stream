//! Client actor task handle.

use crate::client::actor::Message;
use crate::MessageTypes;
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

// TODO: candidate for dedup?
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

    /// Send message to server on UDP (lossy).
    pub fn send_message_udp(&mut self, msg: M::ToServer) -> Result<(), Error<M::ToServer>> {
        self.sender.try_send(Message::SendMessage { msg }).map_err(|err| {
            if err.is_full() {
                assert_let!(Message::SendMessage { msg }, err.into_inner());
                Error::ChannelFull(msg)
            } else if err.is_disconnected() {
                assert_let!(Message::SendMessage { msg }, err.into_inner());
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
