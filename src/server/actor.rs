//! Server actor

use super::event;
use super::event::Event;
use super::peer_uid::PeerUid;
use super::MessageTypes;
use crate::actors::read_actor;
use crate::actors::read_actor::ReadActorHandle;
use crate::actors::read_actor::ReadActorShutdownReason;
use crate::actors::write_actor;
use crate::actors::write_actor::WriteActorHandle;
use crate::actors::write_actor::WriteActorShutdownReason;
use crate::actors::ActorShutdown;
use crate::actors::ErraticActorShutdown;
use crate::message_types::TcpFromServer;
use crate::message_types::TcpToServer;
use crate::message_types::UdpFromServer;
use crate::message_types::UdpToServer;
use crate::networking::tcp;
use crate::networking::udp;
use crate::server::event::DisconnectReason;
use crate::server::event::NewPeer;
use crate::stream_ext::StreamExt as _;
use core::pin::Pin;
use futures::channel::mpsc;
use futures::stream;
use futures::FutureExt as _;
use futures::SinkExt as _;
use futures::Stream;
use futures::StreamExt as _;
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::net::UdpSocket;
use uuid::Uuid;

/// Messages that a server actor handle may send to the server actor task.
#[derive(Debug)]
pub(crate) enum Message<M>
where
    M: MessageTypes,
{
    /// Send message to peer over TCP.
    ToPeerTcp { peer_uid: PeerUid, msg: M::TcpFromServer },

    /// Send message to peer over UDP.
    ToPeerUdp { peer_uid: PeerUid, msg: M::UdpFromServer },

    /// Send message to all connected peers over TCP.
    AnnounceTcp { msg: M::TcpFromServer },

    /// Send message to all connected peers over UDP.
    AnnounceUdp { msg: M::UdpFromServer },

    /// Get current number of connected clients.
    GetNumberOfConnectedPeers {
        /// Oneshot channel on which to send the reply.
        tx: futures::channel::oneshot::Sender<usize>,
    },
}

/// Actor responsible for accepting new TCP connections and reading incoming UDP
/// datagrams from sockets.
///
/// This is the "broker" actor.
pub(crate) async fn actor<M>(
    // Receive messages to control the actor (forward messages to clients etc.)
    msg_receiver: mpsc::UnboundedReceiver<Message<M>>,

    // TCP Listener to listen for connecting clients.
    tcp_listener: TcpListener,

    // UDP socket for in and out going communications.
    udp_socket: UdpSocket,

    // Stream of server events to be handled by application server logic.
    event_sender: mpsc::UnboundedSender<Event<M>>,

    config: crate::server::Config,
) where
    M: MessageTypes,
{
    let (udp_reader_tx, udp_reader_rx) =
        futures::channel::mpsc::channel::<read_actor::ReadActorEvent<(Result<UdpToServer<M>, udp::Error>, SocketAddr)>>(256);

    // Prepare UDP
    let (udp_socket_stream, udp_socket_sink) = crate::networking::udp::new::<UdpToServer<M>, UdpFromServer<M>>(udp_socket);

    let udp_read_actor = crate::actors::read_actor::spawn_actor(udp_socket_stream, udp_reader_tx);
    let udp_write_actor = crate::actors::write_actor::spawn_actor(udp_socket_sink);

    // Terminate incoming streams with an EndOfStream message containing the reason for actor shutdown
    let msg_receiver = msg_receiver
        .map(|x| Incoming::ActorMessage(x))
        .chain_ready(Incoming::EndOfStream(IncomingKind::ActorMessage));
    let udp_reader_rx = udp_reader_rx.map(|x| Incoming::UdpReader(x)).chain_future(
        udp_read_actor
            .wait()
            .map(|shutdown_reason| Incoming::EndOfStream(IncomingKind::UdpReader(shutdown_reason))),
    );
    let udp_writer_rx = udp_write_actor
        .wait()
        .map(|shutdown_reason| Incoming::EndOfStream(IncomingKind::UdpWriter(shutdown_reason)))
        .into_stream();
    let tcp_listener = tokio_stream::wrappers::TcpListenerStream::new(tcp_listener)
        .map(|x| Incoming::TcpListener(x))
        .chain_ready(Incoming::EndOfStream(IncomingKind::TcpListener));

    let udp_heartbeat = {
        let interval = tokio::time::interval(config.udp_heartbeat_interval);
        tokio_stream::wrappers::IntervalStream::new(interval).map(|_| Incoming::UdpHeartbeat)
    };

    let streams: stream::SelectAll<Pin<Box<dyn Stream<Item = Incoming<M>> + Send>>> = stream::select_all([
        msg_receiver.boxed(),
        udp_reader_rx.boxed(),
        udp_writer_rx.boxed(),
        tcp_listener.boxed(),
        udp_heartbeat.boxed(),
    ]);

    let mut state = State {
        peers: HashMap::new(),
        config,
        next_peer_uid: PeerUid::default(),
        streams,
        peer_tcp_socket_addr_to_peer_uid: HashMap::new(),
        peer_udp_socket_addr_to_peer_uid: HashMap::new(),
        peer_handshake_uuid_to_peer_uid: HashMap::new(),
        event_sender,
        udp_write_actor,
    };

    log::info!("Starting server loop");

    let mut peers_to_drop: Vec<(PeerUid, event::DisconnectReason)> = Vec::new();
    loop {
        // Sanity check of invariants.
        #[cfg(debug_assertions)]
        state.assert_invariants();

        for (peer_uid, reason) in peers_to_drop.drain(..) {
            drop_peer(&mut state, peer_uid, reason);
        }

        match state.streams.next().await.unwrap() {
            Incoming::ActorMessage(msg) => {
                handle_actor_message(&mut state, msg);
            }
            Incoming::TcpListener(res) => {
                // New TCP connection
                let Ok(tcp_stream) = res else {
                    log::info!("Failed TCP connection attempt {res:?}");
                    continue;
                };
                handle_new_tcp_connection(&mut state, tcp_stream);
            }
            Incoming::TcpReader(event) => {
                let (message, peer_uid) = event;
                log::trace!("Message received from a TCP reader actor {peer_uid:?} {message:?}");

                match message {
                    read_actor::ReadActorEvent::StreamItem(item) => match item {
                        Ok(message) => {
                            handle_tcp_reader_stream_item(&mut state, message, peer_uid);
                        }
                        Err(err) => {
                            log::warn!("Received data from client on TCP that was not understood. {err}");
                            peers_to_drop.push((peer_uid, event::DisconnectReason::PeerSubmittedUnintelligibleData));
                        }
                    },
                }
            }
            Incoming::UdpReader(event) => match event {
                read_actor::ReadActorEvent::StreamItem((message, socket_addr)) => {
                    log::trace!("Message received from a UDP reader actor {socket_addr:?} {message:?}");
                    match message {
                        Ok(message) => {
                            handle_udp_reader_stream_item(&mut state, message, socket_addr);
                        }
                        Err(err) => {
                            log::warn!("Received data from peer on UDP that was not understood. {err}");
                            if let Some(peer_uid) = state.peer_udp_socket_addr_to_peer_uid.get(&socket_addr) {
                                peers_to_drop.push((*peer_uid, event::DisconnectReason::PeerSubmittedUnintelligibleData));
                            };
                        }
                    }
                }
            },
            Incoming::UdpHeartbeat => {
                announce_udp(&mut state, UdpFromServer::Heartbeat);
            }
            Incoming::EndOfStream(eof) => match eof {
                IncomingKind::ActorMessage => todo!(),
                IncomingKind::TcpListener => todo!(),
                IncomingKind::TcpReader(peer_uid, shutdown_reason) => match shutdown_reason {
                    ActorShutdown::Controlled(ReadActorShutdownReason::EndOfStream) => {
                        // Peer closed tcp connection.
                        peers_to_drop.push((peer_uid, DisconnectReason::PeerClosedConnection));
                    }
                    ActorShutdown::Controlled(ReadActorShutdownReason::Signalled) => {
                        // If it was signalled to shut down, we should already have removed the peer.
                        assert!(!state.peers.contains_key(&peer_uid));
                    }
                    ActorShutdown::Controlled(ReadActorShutdownReason::ActorHandlesDropped | ReadActorShutdownReason::ReceiverClosedChannel) => {
                        // We might not have been able to send a shutdown signal to actor when
                        // dropping the peer, if the actor message channel was full. Actor would then
                        // notice that all handles are dropped, or that its tx channel is closed, and
                        // will shut down. In this case, we should already have removed the peer.
                        assert!(!state.peers.contains_key(&peer_uid));
                    }
                    ActorShutdown::Erratic(ErraticActorShutdown::Cancelled) => {
                        log::error!("TCP read actor for {peer_uid:?} was cancelled.")
                    }
                    ActorShutdown::Erratic(ErraticActorShutdown::Panic) => {
                        log::error!("TCP read actor for {peer_uid:?} panicked.")
                    }
                },
                IncomingKind::TcpWriter(peer_uid, shutdown_reason) => match shutdown_reason {
                    ActorShutdown::Controlled(WriteActorShutdownReason::Signalled) => {
                        // If it was signalled to shut down, we should already have removed the peer.
                        assert!(!state.peers.contains_key(&peer_uid));
                    }
                    ActorShutdown::Controlled(WriteActorShutdownReason::SinkError) => {
                        // Presumably the peer closed at least the reader half of the TCP connection.
                        peers_to_drop.push((peer_uid, DisconnectReason::PeerClosedConnection));
                    }
                    ActorShutdown::Controlled(WriteActorShutdownReason::ActorHandlesDropped) => {
                        // Actor shut down because the broker stopped being interested in its output.
                        assert!(!state.peers.contains_key(&peer_uid));
                    }
                    ActorShutdown::Erratic(ErraticActorShutdown::Cancelled) => {
                        log::error!("TCP write actor for {peer_uid:?} was cancelled.")
                    }
                    ActorShutdown::Erratic(ErraticActorShutdown::Panic) => {
                        log::error!("TCP write actor for {peer_uid:?} panicked.")
                    }
                },
                IncomingKind::UdpReader(shutdown_reason) => match shutdown_reason {
                    ActorShutdown::Controlled(ReadActorShutdownReason::Signalled) => {}
                    ActorShutdown::Controlled(ReadActorShutdownReason::EndOfStream) => {
                        panic!("UDP socket reader actor shut down.");
                    }
                    ActorShutdown::Controlled(ReadActorShutdownReason::ActorHandlesDropped) => unreachable!(),
                    ActorShutdown::Controlled(ReadActorShutdownReason::ReceiverClosedChannel) => {
                        unreachable!()
                    }
                    ActorShutdown::Erratic(ErraticActorShutdown::Cancelled) => {
                        panic!("UDP read actor was cancelled.")
                    }
                    ActorShutdown::Erratic(ErraticActorShutdown::Panic) => panic!("UDP read actor panicked."),
                },
                IncomingKind::UdpWriter(shutdown_reason) => match shutdown_reason {
                    ActorShutdown::Controlled(WriteActorShutdownReason::Signalled) => {}
                    ActorShutdown::Controlled(WriteActorShutdownReason::SinkError) => {
                        panic!("UDP write actor sink error.");
                    }
                    ActorShutdown::Controlled(WriteActorShutdownReason::ActorHandlesDropped) => unreachable!(),
                    ActorShutdown::Erratic(ErraticActorShutdown::Cancelled) => {
                        panic!("UDP write actor was cancelled.")
                    }
                    ActorShutdown::Erratic(ErraticActorShutdown::Panic) => {
                        panic!("UDP write actor panicked.")
                    }
                },
            },
        }
    }
}

#[derive(Debug, Clone)]
struct Peer<M>
where
    M: MessageTypes,
{
    tcp_socket_addr: SocketAddr,
    udp_socket_addr: Option<SocketAddr>,
    tcp_read_actor_handle: ReadActorHandle,
    tcp_write_actor_handle: WriteActorHandle<crate::message_types::TcpFromServer<M>>,

    handshake_uuid: Uuid,
}

#[derive(Debug)]
enum Incoming<M: MessageTypes> {
    /// Actor message received
    ActorMessage(Message<M>),
    /// New TCP connection
    TcpListener(Result<TcpStream, std::io::Error>),
    /// Message received from the TCP reader actor
    TcpReader((read_actor::ReadActorEvent<Result<TcpToServer<M>, tcp::Error>>, PeerUid)),
    /// Message received from the UDP reader actor
    UdpReader(read_actor::ReadActorEvent<(Result<UdpToServer<M>, udp::Error>, SocketAddr)>),

    /// Emit UDP heartbeat
    UdpHeartbeat,

    /// End of stream of one of the above,
    EndOfStream(IncomingKind),
}

#[derive(Debug, Clone, Copy)]
enum IncomingKind {
    ActorMessage,
    TcpListener,
    TcpReader(PeerUid, ActorShutdown<ReadActorShutdownReason>),
    TcpWriter(PeerUid, ActorShutdown<WriteActorShutdownReason>),
    UdpReader(ActorShutdown<ReadActorShutdownReason>),
    UdpWriter(ActorShutdown<WriteActorShutdownReason>),
}

struct State<M: MessageTypes> {
    /// Connected peers
    peers: HashMap<PeerUid, Peer<M>>,
    config: crate::server::Config,
    next_peer_uid: PeerUid,
    streams: stream::SelectAll<Pin<Box<dyn Stream<Item = Incoming<M>> + Send>>>,
    peer_tcp_socket_addr_to_peer_uid: HashMap<SocketAddr, PeerUid>,
    peer_udp_socket_addr_to_peer_uid: HashMap<SocketAddr, PeerUid>,
    peer_handshake_uuid_to_peer_uid: HashMap<Uuid, PeerUid>,
    event_sender: mpsc::UnboundedSender<Event<M>>,
    udp_write_actor: WriteActorHandle<(UdpFromServer<M>, SocketAddr)>,
}

impl<M: MessageTypes> State<M> {
    /// Check invariants.
    pub(crate) fn assert_invariants(&self) {
        // Should be at least as many TCP connections as we have known UDP sockets for the respective connected clients.
        assert!(self.peer_tcp_socket_addr_to_peer_uid.len() >= self.peer_udp_socket_addr_to_peer_uid.len());

        // Should not hold onto socket addr -> peer UID mappings after peer is gone.
        assert!(self.peers.len() >= self.peer_tcp_socket_addr_to_peer_uid.len());
        assert!(self.peers.len() >= self.peer_udp_socket_addr_to_peer_uid.len());

        // Should not hold onto peer "handshake UUID" -> peer UID mappings after peer is gone.
        assert!(self.peers.len() >= self.peer_handshake_uuid_to_peer_uid.len());
    }
}

/// New TCP connection
fn handle_new_tcp_connection<M: MessageTypes>(state: &mut State<M>, tcp_stream: TcpStream) {
    let peer_tcp_socket_addr = tcp_stream.peer_addr().expect("Failed to get SocketAddr from TcpStream");

    if state.peers.len() >= state.config.max_connections {
        log::info!("Rejecting TCP connection attempt, server is at capacity");
        drop(tcp_stream);
        return;
    }

    let peer_uid = state.next_peer_uid;
    state.next_peer_uid.increment();

    log::info!("New TCP connection from {peer_tcp_socket_addr} assigned {peer_uid:?}");

    let (stream, sink) = crate::networking::tcp::new(tcp_stream);

    let (tcp_reader_tx, tcp_reader_rx) =
        futures::channel::mpsc::channel::<(read_actor::ReadActorEvent<Result<TcpToServer<M>, tcp::Error>>, PeerUid)>(64);
    let reader_tx = tcp_reader_tx.with(move |x| futures::future::ok::<_, futures::channel::mpsc::SendError>((x, peer_uid)));
    let tcp_read_actor_handle = crate::actors::read_actor::spawn_actor(stream, reader_tx);
    let tcp_write_actor_handle = crate::actors::write_actor::spawn_actor(sink);

    let tcp_reader_rx = tcp_reader_rx.map(|x| Incoming::TcpReader(x)).chain_future(
        tcp_read_actor_handle
            .wait()
            .map(move |shutdown_reason| Incoming::EndOfStream(IncomingKind::TcpReader(peer_uid, shutdown_reason))),
    );
    let tcp_writer_rx = tcp_write_actor_handle
        .wait()
        .map(move |shutdown_reason| Incoming::EndOfStream(IncomingKind::TcpWriter(peer_uid, shutdown_reason)))
        .into_stream();

    state.streams.push(tcp_reader_rx.boxed());
    state.streams.push(tcp_writer_rx.boxed());

    let handshake_uuid = Uuid::new_v4();
    let mut peer = Peer {
        tcp_socket_addr: peer_tcp_socket_addr,
        udp_socket_addr: None,
        tcp_read_actor_handle,
        tcp_write_actor_handle,

        handshake_uuid,
    };

    state.peer_tcp_socket_addr_to_peer_uid.insert(peer_tcp_socket_addr, peer_uid);
    state.peer_handshake_uuid_to_peer_uid.insert(handshake_uuid, peer_uid);

    // Send welcome message to peer.
    if let Err(err) = peer.tcp_write_actor_handle.send(TcpFromServer::Welcome { handshake_uuid }) {
        log::warn!("Send welcome message to peer {peer_uid:?}: {err:?}");
    }

    state.peers.insert(peer_uid, peer);

    log::info!("Connected peers: {}", state.peers.len());

    match state.event_sender.unbounded_send(Event::NewPeer(NewPeer { peer_uid })) {
        Ok(()) => {}
        Err(err) => {
            todo!("Failed to emit event: {err}");
        }
    }
}

fn drop_peer<M: MessageTypes>(state: &mut State<M>, peer_uid: PeerUid, reason: event::DisconnectReason) {
    log::info!("Dropping peer: {peer_uid:?}");

    let Some(mut peer) = state.peers.remove(&peer_uid) else {
        return;
    };

    state.peer_tcp_socket_addr_to_peer_uid.remove(&peer.tcp_socket_addr);
    if let Some(peer_udp_socket_addr) = &peer.udp_socket_addr {
        state.peer_udp_socket_addr_to_peer_uid.remove(peer_udp_socket_addr);
    }
    state.peer_handshake_uuid_to_peer_uid.remove(&peer.handshake_uuid);

    if let Err(err) = peer.tcp_read_actor_handle.signal_shutdown() {
        match err {
            read_actor::ReadActorError::ChannelFull => {
                log::warn!(
                    "Cannot send shutdown signal to TCP read actor for {peer_uid:?} as message channel is full. Dropping actor handle and realying on actor to shut itself down."
                );
                drop(peer.tcp_read_actor_handle);
            }
            read_actor::ReadActorError::ChannelClosed => {
                // Actor already shut down.
            }
        }
    }
    if let Err(err) = peer.tcp_write_actor_handle.signal_shutdown() {
        match err {
            write_actor::WriteActorError::ChannelFull => {
                log::warn!(
                    "Cannot send shutdown signal to TCP write actor for {peer_uid:?} as message channel is full. Dropping actor handle and realying on actor to shut itself down."
                );
                drop(peer.tcp_write_actor_handle);
            }
            write_actor::WriteActorError::ChannelClosed => {
                // Actor already shut down.
            }
        }
    }

    let ev = Event::PeerDisconnect(event::PeerDisconnect {
        peer_uid,
        disconnect_reason: reason,
    });
    if let Err(err) = state.event_sender.unbounded_send(ev) {
        todo!("Failed to emit event: {err}");
    }
}

fn handle_actor_message<M: MessageTypes>(state: &mut State<M>, msg: Message<M>) {
    log::trace!("Actor msg {msg:?}");

    match msg {
        Message::ToPeerTcp { peer_uid, msg } => match state.peers.get_mut(&peer_uid) {
            Some(peer) => match peer.tcp_write_actor_handle.send(TcpFromServer::ApplicationLogic(msg)) {
                Ok(()) => {}
                Err(err) => {
                    log::error!("Failed to forward msg to TCP write actor: {err}");
                }
            },
            None => {
                log::warn!("No peer for peer UID {peer_uid:?}");
            }
        },
        Message::ToPeerUdp { peer_uid, msg } => match state.peers.get(&peer_uid) {
            Some(peer) => {
                let Some(peer_udp_socket_addr) = peer.udp_socket_addr else {
                    log::debug!("Cannot send message to peer {peer_uid:?} on UDP: no peer UDP socket address");
                    return;
                };
                match state.udp_write_actor.send((UdpFromServer::ApplicationLogic(msg), peer_udp_socket_addr)) {
                    Ok(()) => {}
                    Err(err) => {
                        log::error!("Failed to forward msg to UDP write actor: {err}");
                    }
                }
            }
            None => {
                log::warn!("No peer for peer UID {peer_uid:?}");
            }
        },
        Message::AnnounceTcp { msg } => {
            announce_tcp(state, TcpFromServer::ApplicationLogic(msg));
        }
        Message::AnnounceUdp { msg } => {
            announce_udp(state, UdpFromServer::ApplicationLogic(msg));
        }
        Message::GetNumberOfConnectedPeers { tx } => {
            let _ = tx.send(state.peers.len());
        }
    }
}

fn announce_tcp<M: MessageTypes>(state: &mut State<M>, msg: TcpFromServer<M>) {
    log::debug!("Announcing on TCP");
    for (peer_uid, peer) in state.peers.iter_mut() {
        log::trace!("Announcing to peer {peer_uid:?} at {}", peer.tcp_socket_addr);
        match peer.tcp_write_actor_handle.send(msg.clone()) {
            Ok(()) => {}
            Err(err) => {
                log::error!("Failed to forward msg to TCP write actor: {err}");
            }
        }
    }
}

fn announce_udp<M: MessageTypes>(state: &mut State<M>, msg: UdpFromServer<M>) {
    log::debug!("Announcing on UDP");
    for (peer_uid, peer) in state.peers.iter() {
        log::trace!("Announcing to peer {peer_uid:?} at {}", peer.tcp_socket_addr);
        match state.udp_write_actor.feed((msg.clone(), peer.tcp_socket_addr)) {
            Ok(()) => {}
            Err(err) => {
                log::error!("Failed to forward msg to UDP write actor: {err}");
            }
        }
    }
    match state.udp_write_actor.flush() {
        Ok(()) => {}
        Err(err) => {
            log::error!("Failed to flush UDP write actor: {err}");
        }
    };
}

fn handle_udp_reader_stream_item<M: MessageTypes>(state: &mut State<M>, message: UdpToServer<M>, socket_addr: SocketAddr) {
    log::trace!("Message received from a UDP reader actor {socket_addr:?} {message:?}");

    match message {
        UdpToServer::Hello { handshake_uuid } => {
            let Some(peer_uid) = state.peer_handshake_uuid_to_peer_uid.get(&handshake_uuid) else {
                log::warn!("Received UDP Hello message from {socket_addr} with unrecognized handshake UUID {handshake_uuid}");
                return;
            };
            log::info!("Received UDP Hello message from {socket_addr} matching handshake_uuid for peer {peer_uid:?}");
            let Some(peer) = state.peers.get_mut(peer_uid) else {
                log::warn!("Received UDP Hello message from {socket_addr} with handshake_uuid matching peer {peer_uid:?} but peer is not connected.");
                return;
            };
            peer.udp_socket_addr = Some(socket_addr);
            state.peer_udp_socket_addr_to_peer_uid.insert(socket_addr, *peer_uid);
            if let Err(err) = peer.tcp_write_actor_handle.send(TcpFromServer::UdpHandshakeAcknowledged) {
                log::warn!("Send UDP handshake acknowledgement to peer {peer_uid:?}: {err:?}");
            }
            if let Err(err) = state.udp_write_actor.send((UdpFromServer::Welcome, socket_addr)) {
                log::warn!("Send UDP welcome to peer {peer_uid:?}: {err:?}");
            }
        }
        UdpToServer::ApplicationLogic(msg) => {
            let Some(peer_uid) = state.peer_udp_socket_addr_to_peer_uid.get(&socket_addr) else {
                log::debug!("Received UDP message from unconnected client {socket_addr}");
                return;
            };
            let ev = Event::UdpMessage(event::Message {
                from: *peer_uid,
                message: msg,
            });
            if let Err(err) = state.event_sender.unbounded_send(ev) {
                todo!("Failed to emit event: {err}");
            }
        }
    }
}

fn handle_tcp_reader_stream_item<M: MessageTypes>(state: &mut State<M>, message: TcpToServer<M>, peer_uid: PeerUid) {
    match message {
        TcpToServer::ApplicationLogic(msg) => {
            let ev = Event::TcpMessage(event::Message {
                from: peer_uid,
                message: msg,
            });
            if let Err(err) = state.event_sender.unbounded_send(ev) {
                todo!("Failed to emit event: {err}");
            }
        }
    }
}
