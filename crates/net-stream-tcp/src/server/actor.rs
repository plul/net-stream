//! Server actor

use super::MessageTypes;
use super::PeerUid;
use crate::server::event;
use crate::MsgFromServer;
use crate::MsgToServer;
use core::pin::Pin;
use futures::channel::mpsc;
use futures::stream;
use futures::FutureExt as _;
use futures::SinkExt as _;
use futures::Stream;
use futures::StreamExt as _;
use net_stream::io_actors::read_actor;
use net_stream::io_actors::read_actor::ReadActorHandle;
use net_stream::io_actors::read_actor::ReadActorShutdownReason;
use net_stream::io_actors::write_actor;
use net_stream::io_actors::write_actor::WriteActorHandle;
use net_stream::io_actors::write_actor::WriteActorShutdownReason;
use net_stream::io_actors::ActorShutdown;
use net_stream::io_actors::ErraticActorShutdown;
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use type_toppings::StreamExt as _;
use uuid::Uuid;

/// Messages that a server actor handle may send to the server actor task.
#[derive(Debug)]
pub(crate) enum Message<M>
where
    M: MessageTypes,
{
    /// Send message to peer.
    ToPeer { peer_uid: PeerUid, msg: M::FromServer },

    /// Send message to all connected peers.
    Announce { msg: M::FromServer },

    /// Get current number of connected clients.
    GetNumberOfConnectedPeers {
        /// Oneshot channel on which to send the reply.
        tx: futures::channel::oneshot::Sender<usize>,
    },
}

/// Actor responsible for accepting new TCP connections.
///
/// This is the "broker" actor.
pub(crate) async fn actor<M>(
    // Receive messages to control the actor (forward messages to clients etc.)
    msg_receiver: mpsc::UnboundedReceiver<Message<M>>,

    // TCP Listener to listen for connecting clients.
    tcp_listener: TcpListener,

    // Stream of server events to be handled by application server logic.
    event_sender: mpsc::UnboundedSender<event::Event<M>>,

    config: crate::server::Config,
) where
    M: MessageTypes,
{
    // Terminate incoming streams with an EndOfStream message containing the reason for actor shutdown
    let msg_receiver = msg_receiver
        .map(|x| Incoming::ActorMessage(x))
        .chain_ready(Incoming::EndOfStream(IncomingKind::ActorMessage));
    let tcp_listener = tokio_stream::wrappers::TcpListenerStream::new(tcp_listener)
        .map(|x| Incoming::TcpListener(x))
        .chain_ready(Incoming::EndOfStream(IncomingKind::TcpListener));

    let streams: stream::SelectAll<Pin<Box<dyn Stream<Item = Incoming<M>> + Send>>> =
        stream::select_all([msg_receiver.boxed(), tcp_listener.boxed()]);

    let mut state = State {
        peers: HashMap::new(),
        config,
        next_peer_uid: PeerUid::default(),
        streams,
        peer_tcp_socket_addr_to_peer_uid: HashMap::new(),
        peer_handshake_uuid_to_peer_uid: HashMap::new(),
        event_sender,
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
            Incoming::EndOfStream(eof) => match eof {
                IncomingKind::ActorMessage => todo!(),
                IncomingKind::TcpListener => todo!(),
                IncomingKind::TcpReader(peer_uid, shutdown_reason) => match shutdown_reason {
                    ActorShutdown::Controlled(ReadActorShutdownReason::EndOfStream) => {
                        // Peer closed tcp connection.
                        peers_to_drop.push((peer_uid, event::DisconnectReason::PeerClosedConnection));
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
                        peers_to_drop.push((peer_uid, event::DisconnectReason::PeerClosedConnection));
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
            },
        }
    }
}

#[derive(Debug, Clone)]
struct Peer<M>
where
    M: MessageTypes,
{
    socket_addr: SocketAddr,
    read_actor_handle: ReadActorHandle,
    write_actor_handle: WriteActorHandle<crate::MsgFromServer<M>>,

    // TODO: what about this
    handshake_uuid: Uuid,
}

#[derive(Debug)]
enum Incoming<M: MessageTypes> {
    /// Actor message received
    ActorMessage(Message<M>),
    /// New TCP connection
    TcpListener(Result<TcpStream, std::io::Error>),
    /// Message received from the TCP reader actor
    TcpReader((read_actor::ReadActorEvent<Result<MsgToServer<M>, crate::codec::Error>>, PeerUid)),

    /// End of stream of one of the above,
    EndOfStream(IncomingKind),
}

#[derive(Debug, Clone, Copy)]
enum IncomingKind {
    ActorMessage,
    TcpListener,
    TcpReader(PeerUid, ActorShutdown<ReadActorShutdownReason>),
    TcpWriter(PeerUid, ActorShutdown<WriteActorShutdownReason>),
}

struct State<M: MessageTypes> {
    /// Connected peers
    peers: HashMap<PeerUid, Peer<M>>,
    config: crate::server::Config,
    next_peer_uid: PeerUid,
    streams: stream::SelectAll<Pin<Box<dyn Stream<Item = Incoming<M>> + Send>>>,
    peer_tcp_socket_addr_to_peer_uid: HashMap<SocketAddr, PeerUid>,
    peer_handshake_uuid_to_peer_uid: HashMap<Uuid, PeerUid>,
    event_sender: mpsc::UnboundedSender<event::Event<M>>,
}

impl<M: MessageTypes> State<M> {
    /// Check invariants.
    pub(crate) fn assert_invariants(&self) {
        // Should not hold onto socket addr -> peer UID mappings after peer is gone.
        assert!(self.peers.len() >= self.peer_tcp_socket_addr_to_peer_uid.len());

        // Should not hold onto peer "handshake UUID" -> peer UID mappings after peer is gone.
        assert!(self.peers.len() >= self.peer_handshake_uuid_to_peer_uid.len());
    }
}

/// New TCP connection
fn handle_new_tcp_connection<M: MessageTypes>(state: &mut State<M>, tcp_stream: TcpStream) {
    let peer_socket_addr = tcp_stream.peer_addr().expect("Failed to get SocketAddr from TcpStream");

    if state.peers.len() >= state.config.max_connections {
        log::info!("Rejecting TCP connection attempt, server is at capacity");
        drop(tcp_stream);
        return;
    }

    let peer_uid = state.next_peer_uid;
    state.next_peer_uid.increment();

    log::info!("New TCP connection from {peer_socket_addr} assigned {peer_uid:?}");

    let (stream, sink) = crate::codec::new(tcp_stream);

    let (tcp_reader_tx, tcp_reader_rx) =
        futures::channel::mpsc::channel::<(read_actor::ReadActorEvent<Result<MsgToServer<M>, crate::codec::Error>>, PeerUid)>(64);
    let reader_tx = tcp_reader_tx.with(move |x| futures::future::ok::<_, futures::channel::mpsc::SendError>((x, peer_uid)));
    let tcp_read_actor_handle = net_stream::io_actors::read_actor::spawn_actor(stream, reader_tx);
    let tcp_write_actor_handle = net_stream::io_actors::write_actor::spawn_actor(sink);

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
        socket_addr: peer_socket_addr,
        read_actor_handle: tcp_read_actor_handle,
        write_actor_handle: tcp_write_actor_handle,

        handshake_uuid,
    };

    state.peer_tcp_socket_addr_to_peer_uid.insert(peer_socket_addr, peer_uid);
    state.peer_handshake_uuid_to_peer_uid.insert(handshake_uuid, peer_uid);

    state.peers.insert(peer_uid, peer);

    log::info!("Connected peers: {}", state.peers.len());

    match state.event_sender.unbounded_send(event::Event::NewPeer(event::NewPeer { peer_uid })) {
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

    state.peer_tcp_socket_addr_to_peer_uid.remove(&peer.socket_addr);
    state.peer_handshake_uuid_to_peer_uid.remove(&peer.handshake_uuid);

    if let Err(err) = peer.read_actor_handle.signal_shutdown() {
        match err {
            read_actor::ReadActorError::ChannelFull => {
                log::warn!(
                    "Cannot send shutdown signal to TCP read actor for {peer_uid:?} as message channel is full. Dropping actor handle and realying on actor to shut itself down."
                );
                drop(peer.read_actor_handle);
            }
            read_actor::ReadActorError::ChannelClosed => {
                // Actor already shut down.
            }
        }
    }
    if let Err(err) = peer.write_actor_handle.signal_shutdown() {
        match err {
            write_actor::WriteActorError::ChannelFull => {
                log::warn!(
                    "Cannot send shutdown signal to TCP write actor for {peer_uid:?} as message channel is full. Dropping actor handle and realying on actor to shut itself down."
                );
                drop(peer.write_actor_handle);
            }
            write_actor::WriteActorError::ChannelClosed => {
                // Actor already shut down.
            }
        }
    }

    let ev = event::Event::PeerDisconnect(event::PeerDisconnect {
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
        Message::ToPeer { peer_uid, msg } => match state.peers.get_mut(&peer_uid) {
            Some(peer) => match peer.write_actor_handle.send(MsgFromServer::ApplicationLogic(msg)) {
                Ok(()) => {}
                Err(err) => {
                    log::error!("Failed to forward msg to TCP write actor: {err}");
                }
            },
            None => {
                log::warn!("No peer for peer UID {peer_uid:?}");
            }
        },
        Message::Announce { msg } => {
            announce(state, MsgFromServer::ApplicationLogic(msg));
        }
        Message::GetNumberOfConnectedPeers { tx } => {
            let _ = tx.send(state.peers.len());
        }
    }
}

fn announce<M: MessageTypes>(state: &mut State<M>, msg: MsgFromServer<M>) {
    log::debug!("Announcing on TCP");
    for (peer_uid, peer) in state.peers.iter_mut() {
        log::trace!("Announcing to peer {peer_uid:?} at {}", peer.socket_addr);
        match peer.write_actor_handle.send(msg.clone()) {
            Ok(()) => {}
            Err(err) => {
                log::error!("Failed to forward msg to TCP write actor: {err}");
            }
        }
    }
}

fn handle_tcp_reader_stream_item<M: MessageTypes>(state: &mut State<M>, message: MsgToServer<M>, peer_uid: PeerUid) {
    match message {
        MsgToServer::ApplicationLogic(msg) => {
            let ev = event::Event::Message(event::Message {
                from: peer_uid,
                message: msg,
            });
            if let Err(err) = state.event_sender.unbounded_send(ev) {
                todo!("Failed to emit event: {err}");
            }
        }
    }
}
