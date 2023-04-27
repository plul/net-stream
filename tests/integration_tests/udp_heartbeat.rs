use futures::StreamExt;
use net_stream::server::event::Event as ServerEvent;
use std::assert_matches::assert_matches;

type M = crate::StringMessages;

const HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);

#[tokio::test]
async fn udp_heartbeat() {
    crate::env_logger_setup();

    let server_host = "127.0.0.1:5100";
    let server_socket_addr = server_host.parse().unwrap();

    log::info!("Starting server...");
    let config = net_stream::server::Config::builder().udp_heartbeat_interval(HEARTBEAT_INTERVAL).build();
    let (_server_handle, mut server_rx) = net_stream::server::start::<M>(server_socket_addr, server_socket_addr, config)
        .await
        .expect("Server failed startup");
    log::info!("Started server");

    log::info!("Connecting client ...");
    let (mut client_handle, mut client_events) = net_stream::client::connect::<M>(server_host).await.unwrap();
    assert_matches!(server_rx.next().await.unwrap(), ServerEvent::NewPeer(_));
    log::info!("Connected client");

    log::info!("Expecting no immediate heartbeat...");
    let status = client_handle.get_status().await.unwrap();
    assert_matches!(status.latest_udp_heartbeat, None);

    crate::expect_udp_events_can_send_and_receive_udp_messages(&mut client_events).await;
    tokio::time::sleep(HEARTBEAT_INTERVAL + std::time::Duration::from_millis(10)).await;

    log::info!("Expecting to have received heartbeat...");
    let status = client_handle.get_status().await.unwrap();
    assert_matches!(status.latest_udp_heartbeat, Some(_));
}
