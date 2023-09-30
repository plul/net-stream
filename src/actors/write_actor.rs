use super::ActorShutdown;
use core::fmt::Debug;
use core::future::Future;
use core::pin::Pin;
use futures::channel::mpsc;
use futures::FutureExt;
use futures::Sink;
use futures::SinkExt;
use futures::StreamExt;

/// Spawn new actor
pub(crate) fn spawn_actor<S, T>(sink: S) -> WriteActorHandle<T>
where
    T: Send + 'static,
    S: Sink<T> + Send + Unpin + 'static,
    S::Error: std::fmt::Debug,
{
    let (actor_msg_tx, actor_msg_rx) = mpsc::channel(32);
    let join_handle = tokio::spawn(actor(actor_msg_rx, sink));

    let shutdown_reason = join_handle.map_into::<ActorShutdown<WriteActorShutdownReason>>().boxed().shared();

    WriteActorHandle::new(actor_msg_tx, shutdown_reason)
}

#[derive(thiserror::Error, Debug)]
pub(crate) enum WriteActorError {
    #[error("Actor could not keep up, channel is full.")]
    ChannelFull,

    #[error("Actor no longer accepting messages.")]
    ChannelClosed,
}

/// Reason for actor shutdown.
#[derive(Debug, Clone, Copy)]
pub(crate) enum WriteActorShutdownReason {
    /// Actor was signalled to shutdown.
    Signalled,

    /// Error operating on sink.
    SinkError,

    /// All actor handles were dropped.
    ActorHandlesDropped,
}

// TODO: When type_alias_impl_trait is stabilized, this becomes:
// type ShutdownReasonFuture = futures::future::Shared<impl Future<Output = ActorShutdown<WriteActorShutdownReason>>>;
// For now, in stable Rust this is the workaround (the Send requirement is there only to help it match the return type of the `.boxed()` method):
type ShutdownReasonFuture = futures::future::Shared<Pin<Box<dyn Future<Output = ActorShutdown<WriteActorShutdownReason>> + Send>>>;

/// Actor handle.
#[derive(Debug, Clone)]
pub(crate) struct WriteActorHandle<T> {
    actor_msg_tx: mpsc::Sender<ActorMessage<T>>,
    shutdown_reason: ShutdownReasonFuture,
}

impl<T> WriteActorHandle<T>
where
    T: Send + 'static,
{
    fn new(actor_msg_tx: mpsc::Sender<ActorMessage<T>>, shutdown_reason: ShutdownReasonFuture) -> Self {
        Self {
            actor_msg_tx,
            shutdown_reason,
        }
    }

    /// Signal actor to call [futures::SinkExt::send] on sink.
    pub fn send(&mut self, item: T) -> Result<(), WriteActorError> {
        self.send_actor_message(ActorMessage::Send(item))
    }

    /// Signal actor to call [futures::SinkExt::feed] on sink.
    pub fn feed(&mut self, item: T) -> Result<(), WriteActorError> {
        self.send_actor_message(ActorMessage::Feed(item))
    }

    /// Signal actor to call [futures::SinkExt::flush] on sink.
    pub fn flush(&mut self) -> Result<(), WriteActorError> {
        self.send_actor_message(ActorMessage::Flush)
    }

    /// Signal actor to shutdown.
    pub fn signal_shutdown(&mut self) -> Result<(), WriteActorError> {
        self.send_actor_message(ActorMessage::Shutdown)
    }

    /// Wait for actor to finish and return reason for shutdown.
    pub fn wait(&self) -> impl Future<Output = ActorShutdown<WriteActorShutdownReason>> {
        self.shutdown_reason.clone()
    }

    /// Checks if the actor task has shut down.
    ///
    /// This returns None if the actor is still running, and a Some variant if
    /// the actor has shut down.
    #[allow(dead_code)]
    pub fn is_finished(&self) -> Option<ActorShutdown<WriteActorShutdownReason>> {
        self.shutdown_reason.peek().cloned()
    }

    fn send_actor_message(&mut self, msg: ActorMessage<T>) -> Result<(), WriteActorError> {
        self.actor_msg_tx.try_send(msg).map_err(|err| {
            if err.is_full() {
                WriteActorError::ChannelFull
            } else if err.is_disconnected() {
                WriteActorError::ChannelClosed
            } else {
                unreachable!()
            }
        })
    }
}

#[derive(Debug, Clone)]
enum ActorMessage<T> {
    Send(T),
    Feed(T),
    Flush,
    Shutdown,
}

async fn actor<S, T>(mut actor_msg_rx: mpsc::Receiver<ActorMessage<T>>, mut sink: S) -> WriteActorShutdownReason
where
    S: Sink<T> + Unpin,
    S::Error: Debug,
{
    let shutdown_reason = loop {
        match actor_msg_rx.next().await {
            Some(msg) => match msg {
                ActorMessage::Send(item) => {
                    if let Err(err) = sink.send(item).await {
                        log::warn!("Sink error {err:?}");
                        break WriteActorShutdownReason::SinkError;
                    }
                }
                ActorMessage::Feed(item) => {
                    if let Err(err) = sink.feed(item).await {
                        log::warn!("Sink error {err:?}");
                        break WriteActorShutdownReason::SinkError;
                    }
                }
                ActorMessage::Flush => {
                    if let Err(err) = sink.flush().await {
                        log::warn!("Sink error {err:?}");
                        break WriteActorShutdownReason::SinkError;
                    }
                }
                ActorMessage::Shutdown => {
                    break WriteActorShutdownReason::Signalled;
                }
            },
            None => {
                // All actor handles have been dropped.
                break WriteActorShutdownReason::ActorHandlesDropped;
            }
        }
    };

    // close sink, flushing all remaining output.
    if let Err(err) = sink.close().await {
        log::warn!("Sink error {err:?}");
    }

    log::debug!("Shutting down: {shutdown_reason:?}");
    shutdown_reason
}
