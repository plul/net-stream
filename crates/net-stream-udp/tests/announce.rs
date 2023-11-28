use assert_let_bind::assert_let;
use futures::StreamExt;
use net_stream_udp::client::event::Event as ClientEvent;
use net_stream_udp::server;
use server::event::Event as ServerEvent;

#[derive(Debug, PartialEq, Eq, Hash)]
struct M;
impl net_stream_udp::MessageTypes for M {
    type ToServer = String;
    type FromServer = String;
}

#[tokio::test]
async fn announce() {
    env_logger::builder()
        .filter(None, log::LevelFilter::Debug)
        .parse_default_env()
        .is_test(true)
        .init();

    log::info!("Starting server...");
    let server_config = net_stream_udp::server::Config::default();
    let mut server = net_stream_udp::server::start::<M>("127.0.0.1:0".parse().unwrap(), server_config)
        .await
        .expect("Server failed startup");
    log::info!("Started server");
    let server_host = &server.local_addr.to_string();

    log::info!("Connecting client 1...");
    let client_config = net_stream_udp::client::Config::default();
    let (_client_1_handle, mut client_1_events) = net_stream_udp::client::connect::<M>(server_host, client_config.clone()).await.unwrap();
    assert!(matches!(server.event_receiver.next().await.unwrap(), ServerEvent::NewPeer(_)));
    assert!(matches!(client_1_events.next().await.unwrap(), ClientEvent::CanReceiveMessages));
    log::info!("OK: Connected client 1");

    log::info!("Connecting client 2...");
    let (_client_2_handle, mut client_2_events) = net_stream_udp::client::connect::<M>(server_host, client_config).await.unwrap();
    assert!(matches!(server.event_receiver.next().await.unwrap(), ServerEvent::NewPeer(_)));
    assert!(matches!(client_2_events.next().await.unwrap(), ClientEvent::CanReceiveMessages));
    log::info!("OK: Connected client 2");

    server.actor_handle.announce(String::from("Hello everybody!"));
    {
        let event = client_1_events.next().await.unwrap();
        assert_let!(ClientEvent::<M>::Message(msg), event);
        assert_eq!(msg, "Hello everybody!");
    }
    {
        let event = client_2_events.next().await.unwrap();
        assert_let!(ClientEvent::<M>::Message(msg), event);
        assert_eq!(msg, "Hello everybody!");
    }
}