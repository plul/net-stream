use assert_let_bind::assert_let;
use client::event::Event as ClientEvent;
use futures::StreamExt;
use net_stream::client;
use net_stream::server;
use server::event::Event as ServerEvent;
use std::assert_matches::assert_matches;

type M = crate::StringMessages;

#[tokio::test]
async fn announce_udp() {
    crate::env_logger_setup();

    let server_host = "127.0.0.1:5100";
    let server_socket_addr = server_host.parse().unwrap();

    log::info!("Starting server...");
    let config = net_stream::server::Config::default();
    let (server_handle, mut server_rx) = net_stream::server::start::<M>(server_socket_addr, server_socket_addr, config)
        .await
        .expect("Server failed startup");
    log::info!("Started server");

    log::info!("Connecting client 1...");
    let (_client_1_handle, mut client_1_events) = net_stream::client::connect::<M>(server_host).await.unwrap();
    assert_matches!(server_rx.next().await.unwrap(), ServerEvent::NewPeer(_));
    crate::expect_udp_events_can_send_and_receive_udp_messages(&mut client_1_events).await;
    log::info!("OK: Connected client 1");

    log::info!("Connecting client 2...");
    let (_client_2_handle, mut client_2_events) = net_stream::client::connect::<M>(server_host).await.unwrap();
    assert_matches!(server_rx.next().await.unwrap(), ServerEvent::NewPeer(_));
    crate::expect_udp_events_can_send_and_receive_udp_messages(&mut client_2_events).await;
    log::info!("OK: Connected client 2");

    server_handle.announce_udp(String::from("Hello everybody!"));
    {
        let event = client_1_events.next().await.unwrap();
        assert_let!(ClientEvent::<M>::UdpMessage(msg), event);
        assert_eq!(msg, "Hello everybody!");
    }
    {
        let event = client_2_events.next().await.unwrap();
        assert_let!(ClientEvent::<M>::UdpMessage(msg), event);
        assert_eq!(msg, "Hello everybody!");
    }
}
