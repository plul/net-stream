use assert_let_bind::assert_let;
use core::time::Duration;
use futures::StreamExt;
use net_stream_tcp::client;
use net_stream_tcp::server;
use net_stream_tcp::server::event;
use net_stream_tcp::server::event::Event;
use tokio::net::TcpStream;
use tokio::time::timeout;

#[derive(Debug, PartialEq, Eq, Hash)]
struct M;
impl net_stream_tcp::MessageTypes for M {
    type ToServer = String;
    type FromServer = String;
}

#[tokio::test]
async fn drop_raw_tcp_stream() {
    setup();

    let server_socket_addr = "127.0.0.1:0".parse().unwrap();
    let config = net_stream_tcp::server::Config::default();
    let mut server = net_stream_tcp::server::start::<M>(server_socket_addr, config)
        .await
        .expect("Server failed startup");

    // Peer Connect
    log::info!("Server local addr: {}", server.local_addr);
    let tcp_client = TcpStream::connect(server.local_addr).await.unwrap();

    let ev = server.event_receiver.next().await.unwrap();
    assert_let!(Event::NewPeer(event::NewPeer { peer_uid }), ev);

    drop(tcp_client);

    let ev = server.event_receiver.next().await.unwrap();
    assert_let!(Event::PeerDisconnect(peer_disconnect), ev);
    assert_eq!(peer_disconnect.peer_uid, peer_uid);
    assert!(matches!(peer_disconnect.disconnect_reason, event::DisconnectReason::PeerClosedConnection));
}

#[tokio::test]
async fn drop_events_stream() {
    setup();

    let server_socket_addr = "127.0.0.1:0".parse().unwrap();

    let config = net_stream_tcp::server::Config::default();
    let mut server = net_stream_tcp::server::start::<M>(server_socket_addr, config)
        .await
        .expect("Server failed startup");

    log::info!("Server local addr: {}", server.local_addr);
    let (mut client_1_handle, client_1_events) = net_stream_tcp::client::connect::<M>(&server.local_addr.to_string()).await.unwrap();
    assert!(matches!(server.event_receiver.next().await.unwrap(), server::event::Event::NewPeer(_)));

    drop(client_1_events);

    // Nothing happens yet, as client actor is not trying to write out anything, so
    // this times out:
    assert!(timeout(Duration::from_millis(100), server.event_receiver.next()).await.is_err());

    // Now the server communicates something
    server.actor_handle.announce(String::from("Hello!"));

    // The client TCP reader IO actor reads the message, forwards it to the client
    // actor, which tries to emit it, but that fails as the events stream is
    // dropped. This causes the client actor to exit, thereby dropping handles
    // to all IO actors. This in turn shuts down the raw TCP stream, telling the
    // server that the client has disconnected:

    assert!(matches!(
        server.event_receiver.next().await.unwrap(),
        server::event::Event::PeerDisconnect(_)
    ));

    // Trying to use the handle to forward a message to the client now fails
    let msg = "Hello from dead client";
    assert_let!(Err(err), client_1_handle.send_message(String::from(msg)));
    assert_let!(client::actor_handle::Error::ActorUnreachable(m), err);
    assert_eq!(m, msg);
}

static ENV_LOGGER_INIT: std::sync::Once = std::sync::Once::new();
fn setup() {
    ENV_LOGGER_INIT.call_once(|| {
        env_logger::builder()
            .filter(None, log::LevelFilter::Debug)
            .parse_default_env()
            .is_test(true)
            .init()
    });
}
