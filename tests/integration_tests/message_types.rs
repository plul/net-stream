use assert_let_bind::assert_let;
use client::event::Event as ClientEvent;
use futures::StreamExt;
use net_stream::client;
use net_stream::server;
use server::event::Event as ServerEvent;

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct MessageTypes;

impl net_stream::MessageTypes for MessageTypes {
    type TcpToServer = u8;
    type TcpFromServer = u16;
    type UdpToServer = u32;
    type UdpFromServer = u64;
}

#[tokio::test]
async fn message_types() {
    crate::env_logger_setup();

    let server_host = "127.0.0.1:5100";
    let server_socket_addr = server_host.parse().unwrap();

    let config = net_stream::server::Config::default();
    let (server_handle, mut server_rx) = net_stream::server::start::<MessageTypes>(server_socket_addr, server_socket_addr, config)
        .await
        .expect("Server failed startup");

    let (mut client_handle, mut client_events) = net_stream::client::connect::<MessageTypes>(server_host).await.unwrap();
    crate::expect_udp_events_can_send_and_receive_udp_messages(&mut client_events).await;

    assert_let!(ServerEvent::NewPeer(new_peer), server_rx.next().await.unwrap());
    let peer_uid = new_peer.peer_uid;

    log::info!("Peer to server TCP");
    client_handle.send_message_tcp(1_u8).unwrap();
    assert_let!(ServerEvent::TcpMessage(msg), server_rx.next().await.unwrap());
    assert_eq!(msg.from, peer_uid);
    assert_eq!(msg.message, 1_u8);

    log::info!("Peer to server UDP");
    client_handle.send_message_udp(2_u32).unwrap();
    assert_let!(ServerEvent::UdpMessage(msg), server_rx.next().await.unwrap());
    assert_eq!(msg.from, peer_uid);
    assert_eq!(msg.message, 2_u32);

    log::info!("Server to peer TCP");
    server_handle.message_peer_tcp(peer_uid, 3_u16);
    assert_let!(ClientEvent::TcpMessage(msg), client_events.next().await.unwrap());
    assert_eq!(msg, 3_u16);

    log::info!("Server to peer UDP");
    server_handle.message_peer_udp(peer_uid, 4_u64);
    assert_let!(ClientEvent::UdpMessage(msg), client_events.next().await.unwrap());
    assert_eq!(msg, 4_u64);
}
