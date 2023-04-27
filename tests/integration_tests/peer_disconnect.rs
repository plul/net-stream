use assert_let_bind::assert_let;
use futures::StreamExt;
use net_stream::client;
use net_stream::server;
use net_stream::server::event::Event;
use net_stream::server::event::{self};
use std::assert_matches::assert_matches;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;

#[tokio::test]
async fn drop_raw_tcp_stream() {
    crate::env_logger_setup();

    let server_socket_addr = "127.0.0.1:5100".parse().unwrap();
    let config = net_stream::server::Config::default();
    let (_server_handle, mut server_rx) = net_stream::server::start::<crate::StringMessages>(server_socket_addr, server_socket_addr, config)
        .await
        .expect("Server failed startup");

    // Peer Connect
    let tcp_client = TcpStream::connect(server_socket_addr).await.unwrap();

    let ev = server_rx.next().await.unwrap();
    assert_let!(Event::NewPeer(event::NewPeer { peer_uid }), ev);

    drop(tcp_client);

    let ev = server_rx.next().await.unwrap();
    assert_let!(Event::PeerDisconnect(peer_disconnect), ev);
    assert_eq!(peer_disconnect.peer_uid, peer_uid);
    assert_matches!(peer_disconnect.disconnect_reason, event::DisconnectReason::PeerClosedConnection);
}

#[tokio::test]
async fn drop_events_stream() {
    crate::env_logger_setup();

    let server_host = "127.0.0.1:5100";
    let server_socket_addr = server_host.parse().unwrap();

    let config = net_stream::server::Config::default();
    let (server_handle, mut server_rx) = net_stream::server::start::<crate::StringMessages>(server_socket_addr, server_socket_addr, config)
        .await
        .expect("Server failed startup");

    let (mut client_1_handle, client_1_events) = net_stream::client::connect::<crate::StringMessages>(server_host).await.unwrap();
    assert_matches!(server_rx.next().await.unwrap(), server::event::Event::NewPeer(_));

    drop(client_1_events);

    // Nothing happens yet, as client actor is not trying to write out anything, so
    // this times out:
    assert!(timeout(Duration::from_millis(100), server_rx.next()).await.is_err());

    // Now the server communicates something
    server_handle.announce_tcp(String::from("Hello!"));

    // The client TCP reader IO actor reads the message, forwards it to the client
    // actor, which tries to emit it, but that fails as the events stream is
    // dropped. This causes the client actor to exit, thereby dropping handles
    // to all IO actors. This in turn shuts down the raw TCP stream, telling the
    // server that the client has disconnected:

    assert_matches!(server_rx.next().await.unwrap(), server::event::Event::PeerDisconnect(_));

    // Trying to use the handle to forward a message to the client now fails
    let msg = "Hello from dead client";
    assert_let!(Err(err), client_1_handle.send_message_tcp(String::from(msg)));
    assert_let!(client::actor_handle::Error::ActorUnreachable(m), err);
    assert_eq!(m, msg);
}
