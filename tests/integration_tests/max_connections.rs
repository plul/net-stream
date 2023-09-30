use core::time::Duration;
use futures::StreamExt;
use net_stream::server;
use net_stream::server::event::DisconnectReason;
use net_stream::server::event::NewPeer;
use net_stream::server::event::PeerDisconnect;
use net_stream::server::PeerUid;
use tokio::time::timeout;

type M = crate::StringMessages;

#[tokio::test]
async fn max_connections() {
    crate::env_logger_setup();

    let server_host = "127.0.0.1:5100";
    let server_socket_addr = server_host.parse().unwrap();

    log::info!("Starting server...");
    let config = net_stream::server::Config {
        max_connections: 2,
        ..Default::default()
    };
    let (server_handle, mut server_rx) = net_stream::server::start::<M>(server_socket_addr, server_socket_addr, config)
        .await
        .expect("Server failed startup");
    log::info!("Started server");

    log::info!("Connecting peer 1...");
    let (client_1_handle, _client_1_events) = net_stream::client::connect::<M>(server_host).await.unwrap();
    assert!(matches!(
        server_rx.next().await.unwrap(),
        server::event::Event::NewPeer(NewPeer { peer_uid: PeerUid(0) })
    ));

    assert_eq!(server_handle.get_number_of_connected_peers().await, 1);

    log::info!("Connecting peer 2...");
    let (_client_2_handle, _client_2_events) = net_stream::client::connect::<M>(server_host).await.unwrap();
    assert!(matches!(
        server_rx.next().await.unwrap(),
        server::event::Event::NewPeer(NewPeer { peer_uid: PeerUid(1) })
    ));

    assert_eq!(server_handle.get_number_of_connected_peers().await, 2);

    log::info!("Connecting peer 3...");
    let _ = net_stream::client::connect::<M>(server_host).await.expect(
        "server expected to be at maximum capacity. client connection attempt succeeds momentarily but will be immediately dropped by server",
    );
    {
        let timeout_res = timeout(Duration::from_millis(100), server_rx.next()).await;
        assert!(timeout_res.is_err(), "expect no event on server as server is at max connections");
        assert_eq!(server_handle.get_number_of_connected_peers().await, 2);
    }

    log::info!("Dropping client 1...");
    drop(client_1_handle);
    assert!(matches!(
        server_rx.next().await.unwrap(),
        server::event::Event::PeerDisconnect(PeerDisconnect {
            peer_uid: PeerUid(0),
            disconnect_reason: DisconnectReason::PeerClosedConnection
        })
    ));

    // Should then accept new peer...
    log::info!("Retrying connecting peer 3...");
    net_stream::client::connect::<M>(server_host).await.unwrap();
    assert!(matches!(
        server_rx.next().await.unwrap(),
        server::event::Event::NewPeer(NewPeer { peer_uid: PeerUid(2) })
    ));
    assert_eq!(server_handle.get_number_of_connected_peers().await, 2);
}
