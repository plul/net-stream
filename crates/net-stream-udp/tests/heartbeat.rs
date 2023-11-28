use futures::StreamExt;
use net_stream_udp::client::event::Event as ClientEvent;
use net_stream_udp::server::event::Event as ServerEvent;

#[derive(Debug, PartialEq, Eq, Hash)]
struct M;
impl net_stream_udp::MessageTypes for M {
    type ToServer = String;
    type FromServer = String;
}

const HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);

#[tokio::test]
async fn udp_heartbeat() {
    env_logger::builder()
        .filter(None, log::LevelFilter::Debug)
        .parse_default_env()
        .is_test(true)
        .init();

    let server_host = "127.0.0.1:5100";
    let server_socket_addr = server_host.parse().unwrap();

    log::info!("Starting server...");
    let config = net_stream_udp::server::Config {
        heartbeat_interval: HEARTBEAT_INTERVAL,
        ..Default::default()
    };
    let mut server = net_stream_udp::server::start::<M>(server_socket_addr, config)
        .await
        .expect("Server failed startup");
    log::info!("Started server on {}", server.local_addr);

    log::info!("Connecting client ...");
    let client_config = net_stream_udp::client::Config::default();
    let (mut client_handle, mut client_events) = net_stream_udp::client::connect::<M>(server_host, client_config).await.unwrap();
    assert!(matches!(server.event_receiver.next().await.unwrap(), ServerEvent::NewPeer(_)));
    log::info!("Connected client");

    log::info!("Expecting no immediate heartbeat...");
    let status = client_handle.get_status().await.unwrap();
    assert!(status.latest_heartbeat.is_none());

    assert!(matches!(client_events.next().await.unwrap(), ClientEvent::CanReceiveMessages));
    tokio::time::sleep(HEARTBEAT_INTERVAL + std::time::Duration::from_millis(10)).await;

    log::info!("Expecting to have received heartbeat...");
    let status = client_handle.get_status().await.unwrap();
    assert!(status.latest_heartbeat.is_some());
}
