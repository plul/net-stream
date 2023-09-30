use super::ActorShutdown;
use crate::stream_ext::StreamExt as _;
use core::fmt::Display;
use core::future::Future;
use core::pin::Pin;
use futures::channel::mpsc;
use futures::stream;
use futures::FutureExt;
use futures::Sink;
use futures::SinkExt;
use futures::Stream;
use futures::StreamExt;

/// Spawn new actor
pub(crate) fn spawn_actor<T, S, Tx>(stream: S, tx: Tx) -> ReadActorHandle
where
    T: Send + 'static,
    S: Stream<Item = T> + Send + Unpin + 'static,
    Tx: Sink<ReadActorEvent<T>> + Unpin + Send + 'static,
{
    let (actor_msg_tx, actor_msg_rx) = mpsc::channel(8);
    let join_handle = tokio::spawn(actor(actor_msg_rx, stream, tx));

    let shutdown_reason = join_handle.map_into::<ActorShutdown<ReadActorShutdownReason>>().boxed().shared();

    ReadActorHandle::new(actor_msg_tx, shutdown_reason)
}

#[derive(thiserror::Error, Debug)]
pub(crate) enum ReadActorError {
    #[error("Actor cannot keep up.")]
    ChannelFull,

    #[error("Actor no longer accepting messages.")]
    ChannelClosed,
}

/// Actor output type.
#[derive(Debug, Clone)]
pub(crate) enum ReadActorEvent<T> {
    StreamItem(T),
}

/// Reason for actor shutdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadActorShutdownReason {
    /// Actor was signalled to shutdown.
    Signalled,

    /// End of stream.
    EndOfStream,

    /// All actor handles were dropped.
    ActorHandlesDropped,

    /// Receiver closed channel.
    ReceiverClosedChannel,
}
impl Display for ReadActorShutdownReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadActorShutdownReason::Signalled => write!(f, "Signalled shutdown"),
            ReadActorShutdownReason::EndOfStream => write!(f, "End of stream"),
            ReadActorShutdownReason::ActorHandlesDropped => write!(f, "All actor handles dropped"),
            ReadActorShutdownReason::ReceiverClosedChannel => write!(f, "Receiver closed channel"),
        }
    }
}

// TODO: When type_alias_impl_trait is stabilized, this becomes:
// type ShutdownReasonFuture = futures::future::Shared<impl Future<Output = ActorShutdown<ReadActorShutdownReason>>>;
// For now, in stable Rust this is the workaround (the Send requirement is there only to help it match the return type of the `.boxed()` method):
type ShutdownReasonFuture = futures::future::Shared<Pin<Box<dyn Future<Output = ActorShutdown<ReadActorShutdownReason>> + Send>>>;

/// Actor handle.
#[derive(Debug, Clone)]
pub(crate) struct ReadActorHandle {
    actor_msg_tx: mpsc::Sender<ActorMessage>,
    shutdown_reason: ShutdownReasonFuture,
}

impl ReadActorHandle {
    fn new(actor_msg_tx: mpsc::Sender<ActorMessage>, shutdown_reason: ShutdownReasonFuture) -> Self {
        Self {
            actor_msg_tx,
            shutdown_reason,
        }
    }

    /// Signal actor to shutdown.
    pub fn signal_shutdown(&mut self) -> Result<(), ReadActorError> {
        self.send_actor_message(ActorMessage::Shutdown)
    }

    /// Wait for actor to finish and return reason for shutdown.
    pub fn wait(&self) -> impl Future<Output = ActorShutdown<ReadActorShutdownReason>> {
        self.shutdown_reason.clone()
    }

    /// Checks if the actor task has shut down.
    ///
    /// This returns None if the actor is still running, and a Some variant if
    /// the actor has shut down.
    #[allow(dead_code)]
    pub fn is_finished(&self) -> Option<ActorShutdown<ReadActorShutdownReason>> {
        self.shutdown_reason.peek().cloned()
    }

    fn send_actor_message(&mut self, msg: ActorMessage) -> Result<(), ReadActorError> {
        self.actor_msg_tx.try_send(msg).map_err(|err| {
            if err.is_full() {
                ReadActorError::ChannelFull
            } else if err.is_disconnected() {
                ReadActorError::ChannelClosed
            } else {
                unreachable!()
            }
        })
    }
}

#[derive(Debug, Clone)]
enum ActorMessage {
    Shutdown,
}

async fn actor<S, T, Tx>(actor_msg_rx: mpsc::Receiver<ActorMessage>, stream: S, mut tx: Tx) -> ReadActorShutdownReason
where
    S: Stream<Item = T> + Unpin,
    Tx: Sink<ReadActorEvent<T>> + Unpin,
{
    let actor_msg_rx = actor_msg_rx
        .map(|x| Received::ActorMessage(x))
        .chain_ready(Received::EndOfStream(Kind::ActorMessage));

    let stream = stream.map(|x| Received::StreamItem(x)).chain_ready(Received::EndOfStream(Kind::Stream));

    let mut rx = stream::select(actor_msg_rx, stream);

    let shutdown_reason = loop {
        match rx.next().await.unwrap() {
            Received::ActorMessage(msg) => match msg {
                ActorMessage::Shutdown => {
                    break ReadActorShutdownReason::Signalled;
                }
            },
            Received::StreamItem(item) => {
                if (tx.send(ReadActorEvent::StreamItem(item)).await).is_err() {
                    break ReadActorShutdownReason::ReceiverClosedChannel;
                }
            }
            Received::EndOfStream(kind) => match kind {
                Kind::ActorMessage => {
                    // All actor handles have been dropped.
                    break ReadActorShutdownReason::ActorHandlesDropped;
                }
                Kind::Stream => {
                    break ReadActorShutdownReason::EndOfStream;
                }
            },
        }
    };

    log::debug!("Shutting down: {shutdown_reason:?}");
    shutdown_reason
}

enum Received<T> {
    ActorMessage(ActorMessage),
    StreamItem(T),
    EndOfStream(Kind),
}

enum Kind {
    ActorMessage,
    Stream,
}
