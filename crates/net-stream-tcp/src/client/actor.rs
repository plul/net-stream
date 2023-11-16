//! Client actor

use crate::client::Event;
use crate::MessageTypes;
use crate::MsgFromServer;
use crate::MsgToServer;
use core::pin::Pin;
use futures::channel::mpsc;
use futures::stream;
use futures::FutureExt;
use futures::SinkExt;
use futures::Stream;
use futures::StreamExt;
use net_stream::io_actors::read_actor;
use net_stream::io_actors::read_actor::ReadActorShutdownReason;
use net_stream::io_actors::write_actor;
use net_stream::io_actors::write_actor::WriteActorShutdownReason;
use net_stream::io_actors::ActorShutdown;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use type_toppings::StreamExt as _;

#[derive(Debug)]
pub(crate) enum Message<M>
where
    M: MessageTypes,
{
    /// Send message to server.
    SendMessage {
        /// Message to server.
        msg: M::ToServer,
    },
}

pub(crate) async fn actor<M>(
    msg_receiver: mpsc::Receiver<Message<M>>,
    mut event_sender: mpsc::Sender<Event<M>>,
    tcp_stream: TcpStream,
    server_addr: SocketAddr,
) where
    M: MessageTypes,
{
    let (rx, tx) = crate::codec::new::<MsgFromServer<M>, MsgToServer<M>>(tcp_stream);

    // Channel used by reader actor to emit messages.
    let (reader_tx, reader_rx) = mpsc::channel::<read_actor::ReadActorEvent<Result<MsgFromServer<M>, crate::codec::Error>>>(64);

    // Start IO actors
    let read_actor = net_stream::io_actors::read_actor::spawn_actor(rx, reader_tx);
    let mut write_actor = net_stream::io_actors::write_actor::spawn_actor(tx);

    // Terminate incoming streams with an EndOfStream message
    let msg_receiver = msg_receiver
        .map(|x| Incoming::ActorMessage(x))
        .chain_ready(Incoming::EndOfStream(IncomingKind::ActorMessage));
    let reader_rx = reader_rx.map(|x| Incoming::Reader(x)).chain_future(
        read_actor
            .wait()
            .map(|shutdown_reason| Incoming::EndOfStream(IncomingKind::Reader(shutdown_reason))),
    );
    let writer_rx = stream::once(
        write_actor
            .wait()
            .map(|shutdown_reason| Incoming::EndOfStream(IncomingKind::Writer(shutdown_reason))),
    );

    let mut streams: stream::SelectAll<Pin<Box<dyn Stream<Item = Incoming<M>> + Send>>> =
        stream::select_all([msg_receiver.boxed(), reader_rx.boxed(), writer_rx.boxed()]);

    log::info!("Starting loop");
    while let Some(incoming) = streams.next().await {
        log::debug!("Processing incoming {incoming:?}");
        match incoming {
            Incoming::ActorMessage(msg) => {
                log::debug!("Actor msg {msg:?}");
                match msg {
                    Message::SendMessage { msg } => {
                        if let Err(err) = write_actor.send(MsgToServer::ApplicationLogic(msg)) {
                            match err {
                                write_actor::WriteActorError::ChannelFull => todo!(),
                                write_actor::WriteActorError::ChannelClosed => todo!(),
                            }
                        }
                    }
                }
            }
            Incoming::Reader(event) => {
                log::debug!("TCP reader event: {event:?}");
                match event {
                    read_actor::ReadActorEvent::StreamItem(item) => match item {
                        Ok(message) => match message {
                            MsgFromServer::ApplicationLogic(msg) => {
                                if let Err(err) = event_sender.send(Event::Message(msg)).await {
                                    log::error!("Failed to emit event: {err}");
                                    break;
                                }
                            }
                        },
                        Err(err) => match err {
                            crate::codec::Error::Io(_) | crate::codec::Error::Bincode(_) => {
                                log::error!("Received data from server on TCP that was not understood. {err}");
                                todo!()
                            }
                        },
                    },
                }
            }
            Incoming::EndOfStream(kind) => match kind {
                IncomingKind::ActorMessage => {
                    // This means actor handle is dropped
                    break;
                }
                IncomingKind::Reader(shutdown_reason) => match shutdown_reason {
                    ActorShutdown::Controlled(controlled_shutdown) => {
                        log::info!("TCP reader actor shutdown: {controlled_shutdown}");
                    }
                    ActorShutdown::Erratic(erratic_shutdown) => {
                        log::error!("TCP reader actor shutdown: {erratic_shutdown}");
                    }
                },
                IncomingKind::Writer(_shutdown_reason) => {
                    // TODO
                }
            },
        }
    }
}

#[derive(Debug)]
enum Incoming<M: MessageTypes> {
    /// Actor message received.
    ActorMessage(Message<M>),

    /// Message received from the TCP reader actor.
    Reader(read_actor::ReadActorEvent<Result<MsgFromServer<M>, crate::codec::Error>>),

    /// End of stream of one of the above.
    EndOfStream(IncomingKind),
}

#[derive(Debug, Clone, Copy)]
enum IncomingKind {
    ActorMessage,
    Reader(ActorShutdown<ReadActorShutdownReason>),
    Writer(ActorShutdown<WriteActorShutdownReason>),
}
