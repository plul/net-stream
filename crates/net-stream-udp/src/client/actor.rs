//! Client actor

use super::event::Event;
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
use tokio::net::UdpSocket;
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

    GetStatus {
        /// Oneshot channel on which to send the reply.
        tx: futures::channel::oneshot::Sender<crate::client::Status>,
    },
}

#[derive(Debug, Default)]
struct State {
    /// Time of most recently recieved UDP heartbeat from server.
    latest_heartbeat: Option<std::time::Instant>,
}

pub(crate) async fn actor<M>(
    msg_receiver: mpsc::Receiver<Message<M>>,
    mut event_sender: mpsc::Sender<Event<M>>,
    udp_socket: UdpSocket,
    server_addr: SocketAddr,
    config: crate::client::Config,
) where
    M: MessageTypes,
{
    let mut state = State::default();

    // Codec
    let (rx, tx) = crate::codec::new::<MsgFromServer<M>, MsgToServer<M>>(udp_socket);

    // Throw away the socket addr.
    let rx = rx.map(|(x, _socket_addr): (_, SocketAddr)| x);

    // Set destination
    let tx = tx.with(move |x| futures::future::ok::<_, crate::codec::Error>((x, server_addr)));

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

    let heartbeat = {
        let interval = tokio::time::interval(config.heartbeat_interval);
        tokio_stream::wrappers::IntervalStream::new(interval).map(|_| Incoming::Heartbeat)
    };

    let mut streams: stream::SelectAll<Pin<Box<dyn Stream<Item = Incoming<M>> + Send>>> =
        stream::select_all([msg_receiver.boxed(), reader_rx.boxed(), writer_rx.boxed(), heartbeat.boxed()]);

    let mut has_received: bool = false;

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
                    Message::GetStatus { tx } => {
                        let status = crate::client::Status {
                            latest_heartbeat: state.latest_heartbeat,
                        };
                        let _ = tx.send(status);
                    }
                }
            }
            Incoming::Reader(event) => match event {
                read_actor::ReadActorEvent::StreamItem(item) => {
                    if !has_received {
                        log::info!("First message received on UDP");
                        has_received = true;
                        if let Err(err) = event_sender.send(Event::CanReceiveMessages).await {
                            log::error!("Failed to emit event: {err}");
                        }
                    }

                    match item {
                        Ok(message) => match message {
                            MsgFromServer::Heartbeat => {
                                log::trace!("Received server heartbeat on UDP");
                                state.latest_heartbeat = Some(std::time::Instant::now());
                            }
                            MsgFromServer::ApplicationLogic(msg) => {
                                if let Err(err) = event_sender.send(Event::Message(msg)).await {
                                    log::error!("Failed to emit event: {err}");
                                    break;
                                }
                            }
                        },
                        Err(err) => {
                            log::error!("Received data from server on UDP that was not understood. {err}");
                            todo!();
                        }
                    }
                }
            },
            Incoming::Heartbeat => {
                if let Err(err) = write_actor.send(MsgToServer::Heartbeat) {
                    match err {
                        write_actor::WriteActorError::ChannelFull => todo!(),
                        write_actor::WriteActorError::ChannelClosed => todo!(),
                    }
                }
            }
            Incoming::EndOfStream(kind) => match kind {
                IncomingKind::ActorMessage => {
                    // This means actor handle is dropped
                    break;
                }
                IncomingKind::Reader(shutdown_reason) => match shutdown_reason {
                    ActorShutdown::Controlled(controlled_shutdown) => {
                        log::info!("UDP reader actor shutdown: {controlled_shutdown}");
                    }
                    ActorShutdown::Erratic(erratic_shutdown) => {
                        log::error!("UDP reader actor shutdown: {erratic_shutdown}");
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
    /// Actor message received
    ActorMessage(Message<M>),

    /// Emit heartbeat
    Heartbeat,

    /// Message received from the UDP reader actor
    Reader(read_actor::ReadActorEvent<Result<MsgFromServer<M>, crate::codec::Error>>),

    /// End of stream of one of the above,
    EndOfStream(IncomingKind),
}

#[derive(Debug, Clone, Copy)]
enum IncomingKind {
    ActorMessage,
    Reader(ActorShutdown<ReadActorShutdownReason>),
    Writer(ActorShutdown<WriteActorShutdownReason>),
}
