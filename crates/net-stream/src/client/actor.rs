//! Client actor

use super::event::Event;
use crate::io_actors::read_actor;
use crate::io_actors::read_actor::ReadActorShutdownReason;
use crate::io_actors::write_actor;
use crate::io_actors::write_actor::WriteActorShutdownReason;
use crate::io_actors::ActorShutdown;
use crate::message_types::MessageTypes;
use crate::message_types::TcpFromServer;
use crate::message_types::TcpToServer;
use crate::message_types::UdpFromServer;
use crate::message_types::UdpToServer;
use core::pin::Pin;
use futures::channel::mpsc;
use futures::stream;
use futures::FutureExt;
use futures::SinkExt;
use futures::Stream;
use futures::StreamExt;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio::net::UdpSocket;
use type_toppings::StreamExt as _;

#[derive(Debug)]
pub(crate) enum Message<M>
where
    M: MessageTypes,
{
    TcpToServer {
        msg: M::TcpToServer,
    },
    UdpToServer {
        msg: M::UdpToServer,
    },
    GetStatus {
        /// Oneshot channel on which to send the reply.
        tx: futures::channel::oneshot::Sender<crate::client::Status>,
    },
}

#[derive(Debug, Default)]
struct State {
    /// Time of most recently recieved UDP heartbeat from server.
    latest_udp_heartbeat: Option<std::time::Instant>,
}

pub(crate) async fn actor<M>(
    msg_receiver: mpsc::Receiver<Message<M>>,
    mut event_sender: mpsc::Sender<Event<M>>,
    tcp_stream: TcpStream,
    udp_socket: UdpSocket,
    server_addr: SocketAddr,
) where
    M: MessageTypes,
{
    let mut state = State::default();

    let (tcp_rx, tcp_tx) = tcp::new::<TcpFromServer<M>, TcpToServer<M>>(tcp_stream);
    let (udp_rx, udp_tx) = udp::new::<UdpFromServer<M>, UdpToServer<M>>(udp_socket);

    // Throw away the socket addr.
    let udp_rx = udp_rx.map(|(x, _socket_addr): (_, SocketAddr)| x);

    // Set UDP destination
    let udp_tx = udp_tx.with(move |x| futures::future::ok::<_, udp::Error>((x, server_addr)));

    // Channels used by TCP/UDP reader actors to emit messages.
    let (tcp_reader_tx, tcp_reader_rx) = mpsc::channel::<read_actor::ReadActorEvent<Result<TcpFromServer<M>, tcp::Error>>>(64);
    let (udp_reader_tx, udp_reader_rx) = mpsc::channel::<read_actor::ReadActorEvent<Result<UdpFromServer<M>, udp::Error>>>(64);

    // Start IO actors
    let tcp_read_actor = crate::io_actors::read_actor::spawn_actor(tcp_rx, tcp_reader_tx);
    let mut tcp_write_actor = crate::io_actors::write_actor::spawn_actor(tcp_tx);
    let udp_read_actor = crate::io_actors::read_actor::spawn_actor(udp_rx, udp_reader_tx);
    let mut udp_write_actor = crate::io_actors::write_actor::spawn_actor(udp_tx);

    // Terminate incoming streams with an EndOfStream message
    let msg_receiver = msg_receiver
        .map(|x| Incoming::ActorMessage(x))
        .chain_ready(Incoming::EndOfStream(IncomingKind::ActorMessage));
    let tcp_reader_rx = tcp_reader_rx.map(|x| Incoming::TcpReader(x)).chain_future(
        tcp_read_actor
            .wait()
            .map(|shutdown_reason| Incoming::EndOfStream(IncomingKind::TcpReader(shutdown_reason))),
    );
    let tcp_writer_rx = stream::once(
        tcp_write_actor
            .wait()
            .map(|shutdown_reason| Incoming::EndOfStream(IncomingKind::TcpWriter(shutdown_reason))),
    );
    let udp_reader_rx = udp_reader_rx.map(|x| Incoming::UdpReader(x)).chain_future(
        udp_read_actor
            .wait()
            .map(|shutdown_reason| Incoming::EndOfStream(IncomingKind::UdpReader(shutdown_reason))),
    );
    let udp_writer_rx = stream::once(
        udp_write_actor
            .wait()
            .map(|shutdown_reason| Incoming::EndOfStream(IncomingKind::UdpWriter(shutdown_reason))),
    );

    let mut streams: stream::SelectAll<Pin<Box<dyn Stream<Item = Incoming<M>> + Send>>> = stream::select_all([
        msg_receiver.boxed(),
        tcp_reader_rx.boxed(),
        tcp_writer_rx.boxed(),
        udp_reader_rx.boxed(),
        udp_writer_rx.boxed(),
    ]);

    let mut has_received_on_udp: bool = false;

    log::info!("Starting loop");
    while let Some(incoming) = streams.next().await {
        log::debug!("Processing incoming {incoming:?}");
        match incoming {
            Incoming::ActorMessage(msg) => {
                log::debug!("Actor msg {msg:?}");
                match msg {
                    Message::TcpToServer { msg } => {
                        if let Err(err) = tcp_write_actor.send(TcpToServer::ApplicationLogic(msg)) {
                            match err {
                                write_actor::WriteActorError::ChannelFull => todo!(),
                                write_actor::WriteActorError::ChannelClosed => todo!(),
                            }
                        }
                    }
                    Message::UdpToServer { msg } => {
                        if let Err(err) = udp_write_actor.send(UdpToServer::ApplicationLogic(msg)) {
                            match err {
                                write_actor::WriteActorError::ChannelFull => todo!(),
                                write_actor::WriteActorError::ChannelClosed => todo!(),
                            }
                        }
                    }
                    Message::GetStatus { tx } => {
                        let status = crate::client::Status {
                            latest_udp_heartbeat: state.latest_udp_heartbeat,
                        };
                        let _ = tx.send(status);
                    }
                }
            }
            Incoming::TcpReader(event) => {
                log::debug!("TCP reader event: {event:?}");
                match event {
                    read_actor::ReadActorEvent::StreamItem(item) => match item {
                        Ok(message) => match message {
                            TcpFromServer::Welcome { handshake_uuid: uuid } => {
                                if let Err(err) = udp_write_actor.send(UdpToServer::Hello { handshake_uuid: uuid }) {
                                    log::error!("Send UDP Hello message: {err:?}");
                                };
                            }
                            TcpFromServer::UdpHandshakeAcknowledged => {
                                if let Err(err) = event_sender.send(Event::CanSendUdpMessages).await {
                                    log::error!("Failed to emit event: {err}");
                                }
                            }
                            TcpFromServer::ApplicationLogic(msg) => {
                                if let Err(err) = event_sender.send(Event::TcpMessage(msg)).await {
                                    log::error!("Failed to emit event: {err}");
                                    break;
                                }
                            }
                        },
                        Err(err) => match err {
                            tcp::Error::Io(_) | tcp::Error::Bincode(_) => {
                                log::error!("Received data from server on TCP that was not understood. {err}");
                                todo!()
                            }
                        },
                    },
                }
            }
            Incoming::UdpReader(event) => match event {
                read_actor::ReadActorEvent::StreamItem(item) => {
                    if !has_received_on_udp {
                        log::info!("First message received on UDP");
                        has_received_on_udp = true;
                        if let Err(err) = event_sender.send(Event::CanReceiveUdpMessages).await {
                            log::error!("Failed to emit event: {err}");
                        }
                    }

                    match item {
                        Ok(message) => match message {
                            UdpFromServer::Welcome => {
                                log::trace!("Received server Hello message on UDP");
                            }
                            UdpFromServer::Heartbeat => {
                                log::trace!("Received server heartbeat on UDP");
                                state.latest_udp_heartbeat = Some(std::time::Instant::now());
                            }
                            UdpFromServer::ApplicationLogic(msg) => {
                                if let Err(err) = event_sender.send(Event::UdpMessage(msg)).await {
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
            Incoming::EndOfStream(kind) => match kind {
                IncomingKind::ActorMessage => {
                    // This means actor handle is dropped
                    break;
                }
                IncomingKind::TcpReader(shutdown_reason) => match shutdown_reason {
                    ActorShutdown::Controlled(controlled_shutdown) => {
                        log::info!("TCP reader actor shutdown: {controlled_shutdown}");
                    }
                    ActorShutdown::Erratic(erratic_shutdown) => {
                        log::error!("TCP reader actor shutdown: {erratic_shutdown}");
                    }
                },
                IncomingKind::TcpWriter(_shutdown_reason) => {
                    // TODO
                }
                IncomingKind::UdpReader(shutdown_reason) => match shutdown_reason {
                    ActorShutdown::Controlled(controlled_shutdown) => {
                        log::info!("UDP reader actor shutdown: {controlled_shutdown}");
                    }
                    ActorShutdown::Erratic(erratic_shutdown) => {
                        log::error!("UDP reader actor shutdown: {erratic_shutdown}");
                    }
                },
                IncomingKind::UdpWriter(_shutdown_reason) => {
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
    /// Message received from the TCP reader actor
    TcpReader(read_actor::ReadActorEvent<Result<TcpFromServer<M>, tcp::Error>>),
    /// Message received from the UDP reader actor
    UdpReader(read_actor::ReadActorEvent<Result<UdpFromServer<M>, udp::Error>>),

    /// End of stream of one of the above,
    EndOfStream(IncomingKind),
}

#[derive(Debug, Clone, Copy)]
enum IncomingKind {
    ActorMessage,
    TcpReader(ActorShutdown<ReadActorShutdownReason>),
    TcpWriter(ActorShutdown<WriteActorShutdownReason>),
    UdpReader(ActorShutdown<ReadActorShutdownReason>),
    UdpWriter(ActorShutdown<WriteActorShutdownReason>),
}
