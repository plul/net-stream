//! Server actor

use super::event;
use super::MessageTypes;
use super::PeerUid;
use crate::MsgFromServer;
use crate::MsgToServer;
use core::pin::Pin;
use futures::channel::mpsc;
use futures::stream;
use futures::FutureExt as _;
use futures::Stream;
use futures::StreamExt as _;
use net_stream::io_actors::read_actor;
use net_stream::io_actors::read_actor::ReadActorShutdownReason;
use net_stream::io_actors::write_actor::WriteActorHandle;
use net_stream::io_actors::write_actor::WriteActorShutdownReason;
use net_stream::io_actors::ActorShutdown;
use net_stream::io_actors::ErraticActorShutdown;
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::net::UdpSocket;
use type_toppings::StreamExt as _;
use uuid::Uuid;

/// Messages that a server actor handle may send to the server actor task.
#[derive(Debug)]
pub(crate) enum Message<M>
where
    M: MessageTypes,
{
    /// Send message to peer over UDP.
    ToPeer { peer_uid: PeerUid, msg: M::FromServer },

    /// Send message to all connected peers over UDP.
    Announce { msg: M::FromServer },

    /// Get current number of connected clients.
    GetNumberOfConnectedPeers {
        /// Oneshot channel on which to send the reply.
        tx: futures::channel::oneshot::Sender<usize>,
    },
}

/// Actor responsible for reading incoming UDP datagrams from sockets.
///
/// This is the "broker" actor.
pub(crate) async fn actor<M>(
    // Receive messages to control the actor (forward messages to clients etc.)
    msg_receiver: mpsc::UnboundedReceiver<Message<M>>,

    // UDP socket for in and out going communications.
    udp_socket: UdpSocket,

    // Stream of server events to be handled by application server logic.
    event_sender: mpsc::UnboundedSender<event::Event<M>>,

    config: crate::server::Config,
) where
    M: MessageTypes,
{
    let (reader_tx, reader_rx) =
        futures::channel::mpsc::channel::<read_actor::ReadActorEvent<(Result<MsgToServer<M>, crate::codec::Error>, SocketAddr)>>(256);

    // Prepare UDP
    let (socket_stream, socket_sink) = crate::codec::new::<MsgToServer<M>, MsgFromServer<M>>(udp_socket);

    let read_actor = net_stream::io_actors::read_actor::spawn_actor(socket_stream, reader_tx);
    let write_actor = net_stream::io_actors::write_actor::spawn_actor(socket_sink);

    // Terminate incoming streams with an EndOfStream message containing the reason for actor shutdown
    let msg_receiver = msg_receiver
        .map(|x| Incoming::ActorMessage(x))
        .chain_ready(Incoming::EndOfStream(IncomingKind::ActorMessage));
    let reader_rx = reader_rx.map(|x| Incoming::Reader(x)).chain_future(
        read_actor
            .wait()
            .map(|shutdown_reason| Incoming::EndOfStream(IncomingKind::Reader(shutdown_reason))),
    );
    let writer_rx = write_actor
        .wait()
        .map(|shutdown_reason| Incoming::EndOfStream(IncomingKind::Writer(shutdown_reason)))
        .into_stream();

    let heartbeat = {
        let interval = tokio::time::interval(config.heartbeat_interval);
        tokio_stream::wrappers::IntervalStream::new(interval).map(|_| Incoming::Heartbeat)
    };

    let streams: stream::SelectAll<Pin<Box<dyn Stream<Item = Incoming<M>> + Send>>> =
        stream::select_all([msg_receiver.boxed(), reader_rx.boxed(), writer_rx.boxed(), heartbeat.boxed()]);

    let mut state = State {
        peers: HashMap::new(),
        config,
        streams,
        peer_socket_addr_to_peer_uid: HashMap::new(),
        event_sender,
        write_actor,
    };

    log::info!("Starting server loop");

    loop {
        // Sanity check of invariants.
        #[cfg(debug_assertions)]
        state.assert_invariants();

        // Match next server event
        match state.streams.next().await.unwrap() {
            Incoming::ActorMessage(msg) => {
                handle_actor_message(&mut state, msg);
            }
            Incoming::Reader(event) => match event {
                read_actor::ReadActorEvent::StreamItem((message, socket_addr)) => {
                    log::trace!("Message received from a UDP reader actor {socket_addr:?} {message:?}");
                    match message {
                        Ok(message) => {
                            handle_reader_stream_item(&mut state, message, socket_addr);
                        }
                        Err(err) => {
                            log::warn!("Received data from peer on UDP that was not understood. {err}");
                            if let Some(&peer_uid) = state.peer_socket_addr_to_peer_uid.get(&socket_addr) {
                                let peer = state.peers.get_mut(&peer_uid).unwrap();
                                if !peer.unintelligible {
                                    peer.unintelligible = true;
                                    let ev = event::Event::UnintelligiblePeer(event::UnintelligiblePeer { peer_uid });
                                    if let Err(err) = state.event_sender.unbounded_send(ev) {
                                        todo!("Failed to emit event: {err}");
                                    }
                                }
                            };
                        }
                    }
                }
            },
            Incoming::Heartbeat => {
                announce(&mut state, MsgFromServer::Heartbeat);
            }
            Incoming::EndOfStream(eof) => match eof {
                IncomingKind::ActorMessage => todo!(),
                IncomingKind::Reader(shutdown_reason) => match shutdown_reason {
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
                IncomingKind::Writer(shutdown_reason) => match shutdown_reason {
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
struct Peer {
    socket_addr: SocketAddr,

    /// One or more un-deserializable payloads were received from the peer.
    unintelligible: bool,
}
impl Peer {
    fn new(peer_socket_addr: SocketAddr) -> Peer {
        Peer {
            socket_addr: peer_socket_addr,
            unintelligible: false,
        }
    }
}

#[derive(Debug)]
enum Incoming<M: MessageTypes> {
    /// Actor message received
    ActorMessage(Message<M>),

    /// Message received from the UDP reader actor
    Reader(read_actor::ReadActorEvent<(Result<MsgToServer<M>, crate::codec::Error>, SocketAddr)>),

    /// Emit UDP heartbeat
    Heartbeat,

    /// End of stream of one of the above,
    EndOfStream(IncomingKind),
}

#[derive(Debug, Clone, Copy)]
enum IncomingKind {
    ActorMessage,
    Reader(ActorShutdown<ReadActorShutdownReason>),
    Writer(ActorShutdown<WriteActorShutdownReason>),
}

struct State<M: MessageTypes> {
    /// Connected peers
    peers: HashMap<PeerUid, Peer>,
    peer_socket_addr_to_peer_uid: HashMap<SocketAddr, PeerUid>,
    config: crate::server::Config,
    streams: stream::SelectAll<Pin<Box<dyn Stream<Item = Incoming<M>> + Send>>>,
    event_sender: mpsc::UnboundedSender<event::Event<M>>,
    write_actor: WriteActorHandle<(MsgFromServer<M>, SocketAddr)>,
}

impl<M: MessageTypes> State<M> {
    /// Check invariants.
    pub(crate) fn assert_invariants(&self) {
        // Should not hold onto socket addr -> peer UID mappings after peer is gone.
        assert!(self.peers.len() >= self.peer_socket_addr_to_peer_uid.len());
    }
}

fn handle_actor_message<M: MessageTypes>(state: &mut State<M>, msg: Message<M>) {
    log::trace!("Actor msg {msg:?}");

    match msg {
        Message::ToPeer { peer_uid, msg } => match state.peers.get(&peer_uid) {
            Some(peer) => match state.write_actor.send((MsgFromServer::ApplicationLogic(msg), peer.socket_addr)) {
                Ok(()) => {}
                Err(err) => {
                    log::error!("Failed to forward msg to UDP write actor: {err}");
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

/// Sends message out to peer
// TODO: Propagate errors?
fn send_msg<M: MessageTypes>(state: &mut State<M>, msg: MsgFromServer<M>, peer_socket_addr: SocketAddr) {
    if let Err(err) = state.write_actor.send((msg, peer_socket_addr)) {
        log::error!("Failed to forward msg to UDP write actor: {err}");
    }
}

/// Sends message out to all peers
// TODO: Propagate errors?
fn announce<M: MessageTypes>(state: &mut State<M>, msg: MsgFromServer<M>) {
    log::trace!("Announcing on UDP");
    for (peer_uid, peer) in state.peers.iter() {
        log::trace!("Announcing to peer {peer_uid:?} at {}", peer.socket_addr);
        if let Err(err) = state.write_actor.feed((msg.clone(), peer.socket_addr)) {
            log::error!("Failed to forward msg to UDP write actor: {err}");
        }
    }
    if let Err(err) = state.write_actor.flush() {
        log::error!("Failed to flush UDP write actor: {err}");
    }
}

fn handle_reader_stream_item<M: MessageTypes>(state: &mut State<M>, message: MsgToServer<M>, peer_socket_addr: SocketAddr) {
    log::trace!("Message received from UDP reader actor {:?} {:?}", peer_socket_addr, message);

    let peer_uid: PeerUid = match state.peer_socket_addr_to_peer_uid.entry(peer_socket_addr) {
        std::collections::hash_map::Entry::Occupied(entry) => {
            let peer_uid = *entry.get();
            peer_uid
        }
        std::collections::hash_map::Entry::Vacant(entry) => {
            let peer_uid = PeerUid(Uuid::new_v4());
            entry.insert(peer_uid);

            // Emit New Peer event
            let ev = event::Event::NewPeer(event::NewPeer { peer_uid });
            if let Err(err) = state.event_sender.unbounded_send(ev) {
                todo!("Failed to emit event: {err}");
            }

            // Send immediate heartbeat to peer
            send_msg(state, MsgFromServer::Heartbeat, peer_socket_addr);

            let peer = Peer::new(peer_socket_addr);
            state.peers.insert(peer_uid, peer);

            peer_uid
        }
    };

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
        MsgToServer::Heartbeat => {
            // TODO:
            // This means the client is still there. Should there be a timeout which, with the reception of this event is reset? Should that be left to application level logic?
            // Should we merely store the instant at which the latest heartbeat was received, and let the user query it from the actor handle?
        }
    }
}
